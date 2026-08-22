// DeepSeek Harness 局域网转发器（鉴权 + Cookie 会话版）
// 监听局域网 8787 端口，转发到 127.0.0.1:3080，并把 Host 与 Origin 重写为
// 127.0.0.1:3080，以通过 dsh 的 /api browser-trust fence。支持 HTTPS 与 WebSocket。
//
// 安全要求（缺一即退出，绝不裸奔）：
//  1. FORWARD_PFX / FORWARD_PFX_PASS 必须提供 HTTPS 证书——不做 HTTP 明文回退
//  2. FORWARD_TOKEN 访问令牌必须设置。校验顺序：Cookie → ?token= → x-dsh-token 头
//     - 首次通过 query/header 校验后自动下发会话 Cookie，
//       之后页面所有子资源请求与 WebSocket 由浏览器自动携带，不再逐个带 token
//     - 全部未通过 → 403；token 校验通过后从转发 URL 中剥离，不泄漏给 dsh
const http = require("node:http");
const https = require("node:https");
const net = require("node:net");
const fs = require("node:fs");

const TARGET_HOST = "127.0.0.1";
const TARGET_PORT = Number(process.env.DSH_PORT || 3080);
const LISTEN_PORT = Number(process.env.FORWARD_PORT || 8787);
const COOKIE_NAME = "dsh_fwd";

const pfxPath = process.env.FORWARD_PFX;
const pfxPass = process.env.FORWARD_PFX_PASS || "";
const TOKEN = process.env.FORWARD_TOKEN || "";

if (!TOKEN) {
  console.error("[forwarder] 缺少 FORWARD_TOKEN，拒绝启动");
  process.exit(1);
}
if (!pfxPath || !fs.existsSync(pfxPath)) {
  console.error("[forwarder] 缺少 HTTPS 证书 (FORWARD_PFX)，拒绝明文回退，退出");
  process.exit(1);
}

/** 从 URL 中剥离 token 查询参数（校验通过后调用，避免 token 泄漏到 dsh 日志）。 */
function stripToken(url) {
  const i = url.indexOf("?");
  if (i === -1) return url;
  const base = url.slice(0, i);
  const params = new URLSearchParams(url.slice(i + 1));
  if (!params.has("token")) return url;
  params.delete("token");
  const q = params.toString();
  return q ? `${base}?${q}` : base;
}

/** 从 Cookie 头里取指定名字的值。 */
function getCookie(req, name) {
  const raw = req.headers.cookie;
  if (!raw) return null;
  for (const pair of raw.split(";")) {
    const eq = pair.indexOf("=");
    if (eq === -1) continue;
    if (pair.slice(0, eq).trim() === name) return pair.slice(eq + 1).trim();
  }
  return null;
}

/**
 * 校验请求凭据。返回 "cookie"（已带会话）或 "query"/"header"（凭 URL/头通过，
 * 需要下发 Cookie）或 null（拒绝）。
 */
function authKind(req) {
  if (getCookie(req, COOKIE_NAME) === TOKEN) return "cookie";
  try {
    const u = new URL(req.url, "http://localhost");
    if (u.searchParams.get("token") === TOKEN) return "query";
  } catch {}
  if (req.headers["x-dsh-token"] === TOKEN) return "header";
  return null;
}

function deny(res) {
  res.writeHead(403, { "Content-Type": "text/plain; charset=utf-8" });
  res.end("Forbidden\n");
}

function rewriteHeaders(headers) {
  const out = { ...headers, host: `${TARGET_HOST}:${TARGET_PORT}` };
  delete out["x-dsh-token"];
  if (out.origin !== undefined) out.origin = `http://${TARGET_HOST}:${TARGET_PORT}`;
  return out;
}

function onRequest(req, res) {
  const kind = authKind(req);
  if (!kind) return deny(res);
  // 凭 query/header 首次通过时下发会话 Cookie（30 天，同站）
  if (kind !== "cookie") {
    res.setHeader(
      "Set-Cookie",
      `${COOKIE_NAME}=${TOKEN}; Path=/; Max-Age=2592000; SameSite=Lax`
    );
  }
  const proxyReq = http.request(
    {
      host: TARGET_HOST,
      port: TARGET_PORT,
      method: req.method,
      path: stripToken(req.url),
      headers: rewriteHeaders(req.headers),
    },
    (proxyRes) => {
      res.writeHead(proxyRes.statusCode, proxyRes.headers);
      proxyRes.pipe(res);
    }
  );
  proxyReq.on("error", () => {
    if (!res.headersSent) res.writeHead(502);
    res.end();
  });
  req.on("error", () => proxyReq.destroy());
  req.pipe(proxyReq);
}

function onUpgrade(req, socket, head) {
  if (!authKind(req)) {
    socket.write("HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n");
    socket.destroy();
    return;
  }
  const proxySocket = net.connect(TARGET_PORT, TARGET_HOST, () => {
    const lines = [
      `${req.method} ${stripToken(req.url)} HTTP/1.1`,
      `Host: ${TARGET_HOST}:${TARGET_PORT}`,
    ];
    for (const [k, v] of Object.entries(req.headers)) {
      if (k === "host" || k === "x-dsh-token") continue;
      let value = v;
      if (k === "origin") value = `http://${TARGET_HOST}:${TARGET_PORT}`;
      lines.push(`${k}: ${value}`);
    }
    proxySocket.write(lines.join("\r\n") + "\r\n\r\n");
    if (head && head.length) proxySocket.write(head);
    proxySocket.pipe(socket);
    socket.pipe(proxySocket);
  });
  proxySocket.on("error", () => socket.destroy());
  socket.on("error", () => proxySocket.destroy());
}

const server = https.createServer({
  pfx: fs.readFileSync(pfxPath),
  passphrase: pfxPass,
});
server.on("request", onRequest);
server.on("upgrade", onUpgrade);
server.listen(LISTEN_PORT, "0.0.0.0", () => {
  console.log(`[forwarder] HTTPS(鉴权+Cookie会话) on 0.0.0.0:${LISTEN_PORT} -> ${TARGET_HOST}:${TARGET_PORT}`);
});

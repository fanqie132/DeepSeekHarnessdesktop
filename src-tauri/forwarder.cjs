// DeepSeek Harness 局域网转发器（带鉴权版）
// 监听局域网 8787 端口，转发到 127.0.0.1:3080，并把 Host 与 Origin 重写为
// 127.0.0.1:3080，以通过 dsh 的 /api browser-trust fence。支持 HTTP/HTTPS 与 WebSocket。
//
// 安全要求（缺一即退出，绝不裸奔）：
//  1. FORWARD_PFX / FORWARD_PFX_PASS 必须提供 HTTPS 证书——不做 HTTP 明文回退
//  2. FORWARD_TOKEN 访问令牌必须设置；请求须携带 ?token= 或 x-dsh-token 头，
//     否则一律 403（token 校验通过后会在转发前从 URL 中剥离，不泄漏给 dsh）
const http = require("node:http");
const https = require("node:https");
const net = require("node:net");
const fs = require("node:fs");

const TARGET_HOST = "127.0.0.1";
const TARGET_PORT = Number(process.env.DSH_PORT || 3080);
const LISTEN_PORT = Number(process.env.FORWARD_PORT || 8787);

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

/** 校验请求是否携带正确 token（query 参数或 header）。 */
function authorized(req) {
  const u = new URL(req.url, "http://localhost");
  if (u.searchParams.get("token") === TOKEN) return true;
  return req.headers["x-dsh-token"] === TOKEN;
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
  if (!authorized(req)) return deny(res);
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
  if (!authorized(req)) {
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
  console.log(`[forwarder] HTTPS(鉴权) on 0.0.0.0:${LISTEN_PORT} -> ${TARGET_HOST}:${TARGET_PORT}`);
});

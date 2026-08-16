// DeepSeek Harness 局域网转发器
// 监听局域网 8787 端口，转发到 127.0.0.1:3080，并把 Host 与 Origin 重写为
// 127.0.0.1:3080，以通过 dsh 的 /api browser-trust fence。支持 HTTP/HTTPS 与 WebSocket。
// HTTPS 证书通过环境变量 FORWARD_PFX / FORWARD_PFX_PASS 提供（提供证书时用 HTTPS，
// 否则回退 HTTP）。HTTPS 使页面处于安全上下文，crypto.randomUUID 等 API 才可用。
const http = require("node:http");
const https = require("node:https");
const net = require("node:net");
const fs = require("node:fs");

const TARGET_HOST = "127.0.0.1";
const TARGET_PORT = Number(process.env.DSH_PORT || 3080);
const LISTEN_PORT = Number(process.env.FORWARD_PORT || 8787);

const REWRITE = `${TARGET_HOST}:${TARGET_PORT}`;
const REWRITE_ORIGIN = `http://${REWRITE}`;

function rewriteHeaders(headers) {
  const out = { ...headers, host: REWRITE };
  if (out.origin !== undefined) out.origin = REWRITE_ORIGIN;
  return out;
}

function onRequest(req, res) {
  const proxyReq = http.request(
    {
      host: TARGET_HOST,
      port: TARGET_PORT,
      method: req.method,
      path: req.url,
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
  const proxySocket = net.connect(TARGET_PORT, TARGET_HOST, () => {
    const lines = [
      `${req.method} ${req.url} HTTP/1.1`,
      `Host: ${REWRITE}`,
    ];
    for (const [k, v] of Object.entries(req.headers)) {
      if (k === "host") continue;
      let value = v;
      if (k === "origin") value = REWRITE_ORIGIN;
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

const pfxPath = process.env.FORWARD_PFX;
const pfxPass = process.env.FORWARD_PFX_PASS || "";

if (pfxPath && fs.existsSync(pfxPath)) {
  const server = https.createServer({
    pfx: fs.readFileSync(pfxPath),
    passphrase: pfxPass,
  });
  server.on("request", onRequest);
  server.on("upgrade", onUpgrade);
  server.listen(LISTEN_PORT, "0.0.0.0", () => {
    console.log(`[forwarder] HTTPS on 0.0.0.0:${LISTEN_PORT} -> ${TARGET_HOST}:${TARGET_PORT}`);
  });
} else {
  const server = http.createServer();
  server.on("request", onRequest);
  server.on("upgrade", onUpgrade);
  server.listen(LISTEN_PORT, "0.0.0.0", () => {
    console.log(`[forwarder] HTTP on 0.0.0.0:${LISTEN_PORT} -> ${TARGET_HOST}:${TARGET_PORT}`);
  });
}

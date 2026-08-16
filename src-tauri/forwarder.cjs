// DeepSeek Harness 局域网转发器
// 监听局域网 8787 端口，转发到 127.0.0.1:3080，并把 Host 重写为 127.0.0.1:3080
// 以通过 dsh 的 /api loopback trust-fence。支持 HTTP 与 WebSocket。
const http = require("node:http");
const net = require("node:net");

const TARGET_HOST = "127.0.0.1";
const TARGET_PORT = Number(process.env.DSH_PORT || 3080);
const LISTEN_PORT = Number(process.env.FORWARD_PORT || 8787);

function rewriteHost(host) {
  return `${TARGET_HOST}:${TARGET_PORT}`;
}

const server = http.createServer((req, res) => {
  const headers = { ...req.headers, host: rewriteHost(req.headers.host) };
  const proxyReq = http.request(
    {
      host: TARGET_HOST,
      port: TARGET_PORT,
      method: req.method,
      path: req.url,
      headers,
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
});

// WebSocket 转发（dsh 实时推送用）
server.on("upgrade", (req, socket, head) => {
  const proxySocket = net.connect(TARGET_PORT, TARGET_HOST, () => {
    const lines = [
      `${req.method} ${req.url} HTTP/1.1`,
      `Host: ${rewriteHost(req.headers.host)}`,
    ];
    for (const [k, v] of Object.entries(req.headers)) {
      if (k === "host") continue;
      lines.push(`${k}: ${v}`);
    }
    proxySocket.write(lines.join("\r\n") + "\r\n\r\n");
    if (head && head.length) proxySocket.write(head);
    proxySocket.pipe(socket);
    socket.pipe(proxySocket);
  });
  proxySocket.on("error", () => socket.destroy());
  socket.on("error", () => proxySocket.destroy());
});

server.listen(LISTEN_PORT, "0.0.0.0", () => {
  console.log(`[forwarder] listening on 0.0.0.0:${LISTEN_PORT} -> ${TARGET_HOST}:${TARGET_PORT}`);
});

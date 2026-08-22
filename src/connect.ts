import QRCode from "qrcode";
import { listen } from "@tauri-apps/api/event";

interface ConnectInfo {
  url: string; // https://<ip>:8787/?token=xxx
}

const qrEl = document.getElementById("qr") as HTMLElement;
const addrEl = document.getElementById("addr") as HTMLElement;
const copyBtn = document.getElementById("btn-copy") as HTMLButtonElement;
const errorEl = document.getElementById("error") as HTMLElement;

async function apply(info: ConnectInfo) {
  errorEl.style.display = "none";
  addrEl.textContent = info.url;
  try {
    // 成熟库渲染，canvas 输出；纠错等级 M，留白 2 模块
    await QRCode.toCanvas(qrEl, info.url, {
      width: 216,
      margin: 0,
      errorCorrectionLevel: "M",
      color: { dark: "#111827", light: "#ffffff" },
    });
  } catch (e) {
    qrEl.textContent = "二维码生成失败";
  }
  copyBtn.onclick = async () => {
    try {
      await navigator.clipboard.writeText(info.url);
      copyBtn.textContent = "已复制";
      copyBtn.classList.add("copied");
      setTimeout(() => {
        copyBtn.textContent = "复制地址";
        copyBtn.classList.remove("copied");
      }, 1500);
    } catch {
      // 剪贴板不可用时用户仍可手动选中 #addr 文本
    }
  };
}

void listen<ConnectInfo>("connect-info", (e) => {
  void apply(e.payload);
});

void listen<string>("connect-error", (e) => {
  qrEl.innerHTML = "";
  errorEl.textContent = e.payload;
  errorEl.style.display = "block";
});

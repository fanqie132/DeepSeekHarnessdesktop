import { listen } from "@tauri-apps/api/event";

interface ConnectInfo {
  qrSvg: string; // Rust 生成的二维码 SVG
  url: string; // https://<ip>:8787/?token=xxx
}

const qrEl = document.getElementById("qr") as HTMLElement;
const addrEl = document.getElementById("addr") as HTMLElement;
const copyBtn = document.getElementById("btn-copy") as HTMLButtonElement;
const errorEl = document.getElementById("error") as HTMLElement;

function apply(info: ConnectInfo) {
  qrEl.innerHTML = info.qrSvg;
  const svg = qrEl.querySelector("svg");
  if (svg) {
    svg.removeAttribute("width");
    svg.removeAttribute("height");
    svg.style.width = "100%";
    svg.style.height = "100%";
  }
  addrEl.textContent = info.url;
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
  errorEl.style.display = "none";
  apply(e.payload);
});

void listen<string>("connect-error", (e) => {
  qrEl.innerHTML = "";
  errorEl.textContent = e.payload;
  errorEl.style.display = "block";
});

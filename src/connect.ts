import QRCode from "qrcode";
import { listen } from "@tauri-apps/api/event";

interface ConnectInfo {
  url: string; // https://<ip>:8787/?token=xxx
}

const qrEl = document.getElementById("qr") as HTMLElement;
const addrEl = document.getElementById("addr") as HTMLElement;
const copyBtn = document.getElementById("btn-copy") as HTMLButtonElement;

async function apply(info: ConnectInfo) {
  addrEl.textContent = info.url;
  try {
    // toDataURL 直接产出图片数据，避开 canvas 元素类型坑
    const dataUrl = await QRCode.toDataURL(info.url, {
      width: 352, // 2x 输出，页面显示约 176px 保持锐利
      margin: 0,
      errorCorrectionLevel: "M",
      color: { dark: "#111827", light: "#ffffff" },
    });
    qrEl.innerHTML = `<img src="${dataUrl}" alt="二维码" />`;
  } catch {
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

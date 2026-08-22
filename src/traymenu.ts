import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const menu = document.getElementById("menu") as HTMLElement;
const miLan = document.getElementById("mi-lan") as HTMLElement;
const miQr = document.getElementById("mi-qr") as HTMLElement;

// Rust 在显示面板前推送最新状态
void listen<{ lanOn: boolean }>("tray-state", (e) => {
  miLan.classList.toggle("on", e.payload.lanOn);
});

for (const el of [miLan, miQr]) {
  // 防止点击穿透到拖拽/失焦逻辑
  el.addEventListener("click", (ev) => ev.stopPropagation());
}

document.querySelectorAll<HTMLElement>(".mi[data-id]").forEach((el) => {
  el.addEventListener("click", () => {
    void invoke("tray_menu_action", { id: el.dataset.id });
  });
});

// 兜底：Rust 显示前也会 emit；此处仅处理初始渲染
menu.style.opacity = "1";

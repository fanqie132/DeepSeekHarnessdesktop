import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

const titleEl = document.getElementById("title") as HTMLElement;
const subtitleEl = document.getElementById("subtitle") as HTMLElement;
const fillEl = document.getElementById("progress-fill") as HTMLElement;
const textEl = document.getElementById("progress-text") as HTMLElement;
const hintEl = document.getElementById("progress-hint") as HTMLElement;
const updateBtn = document.getElementById("btn-update") as HTMLButtonElement;
const laterBtn = document.getElementById("btn-later") as HTMLButtonElement;
const errorEl = document.getElementById("error") as HTMLElement;
const wrapEl = document.getElementById("progress-wrap") as HTMLElement;

interface UpdateInfo {
  latest: string;
  current: string;
}
interface ProgressPayload {
  stage: "download" | "extract";
  current?: number;
  total?: number;
  message?: string;
}

let info: UpdateInfo | null = null;

function setProgress(stage: string, percent: number, hint: string) {
  fillEl.style.width = `${percent}%`;
  textEl.textContent = stage;
  hintEl.textContent = hint;
}

async function init() {
  // 接收主进程传来的版本信息（通过事件或启动参数）
  await listen<UpdateInfo>("updater-info", (e) => {
    info = e.payload;
    titleEl.textContent = `发现新版本 v${info.latest}`;
    subtitleEl.textContent = `当前 v${info.current}，点击立即更新将下载约 70MB`;
    wrapEl.style.display = "block";
    setProgress("准备更新", 0, "点击下方按钮开始");
  });

  // 也兼容 Rust 在创建窗口时通过 URL 参数传递
  const params = new URLSearchParams(window.location.search);
  const latest = params.get("latest");
  const current = params.get("current");
  if (latest && current) {
    info = { latest, current };
    titleEl.textContent = `发现新版本 v${latest}`;
    subtitleEl.textContent = `当前 v${current}，点击立即更新将下载约 70MB`;
    wrapEl.style.display = "block";
    setProgress("准备更新", 0, "点击下方按钮开始");
  }

  await listen<ProgressPayload>("runtime-progress", (e) => {
    const p = e.payload;
    updateBtn.classList.add("hidden");
    laterBtn.classList.add("hidden");
    if (p.stage === "download") {
      titleEl.textContent = "正在下载";
      subtitleEl.textContent = "首次更新约 70MB，请保持网络畅通";
      setProgress("正在下载...", 0, "约 70MB，请耐心等待");
      fillEl.style.width = "30%";
    } else if (p.stage === "extract" && p.total) {
      const percent = Math.min(100, Math.round(((p.current ?? 0) / p.total) * 100));
      titleEl.textContent = "正在解压";
      subtitleEl.textContent = `正在解压 ${p.current}/${p.total}`;
      fillEl.style.width = `${30 + Math.round(percent * 0.7)}%`;
      textEl.textContent = `解压中 ${percent}%`;
      hintEl.textContent = "即将完成，请稍候";
      if (percent >= 100) {
        titleEl.textContent = "即将完成";
        subtitleEl.textContent = "正在替换文件...";
      }
    }
  });

  await listen<string>("updater-error", (e) => {
    titleEl.textContent = "更新失败";
    subtitleEl.textContent = "请查看错误信息后重试";
    errorEl.textContent = e.payload;
    errorEl.style.display = "block";
    fillEl.style.width = "0%";
    textEl.textContent = "";
    updateBtn.textContent = "重试";
    updateBtn.classList.remove("hidden");
    laterBtn.classList.remove("hidden");
  });

  await listen<string>("updater-done", (e) => {
    titleEl.textContent = "更新完成";
    subtitleEl.textContent = e.payload || "即将重启主程序...";
    fillEl.style.width = "100%";
    textEl.textContent = "100%";
    hintEl.textContent = "2秒后自动关闭";
    updateBtn.classList.add("hidden");
    laterBtn.classList.add("hidden");
    setTimeout(async () => {
      await getCurrentWindow().close();
    }, 2000);
  });

  updateBtn.addEventListener("click", async () => {
    if (errorEl.style.display === "block") {
      errorEl.style.display = "none";
    }
    updateBtn.classList.add("hidden");
    laterBtn.classList.add("hidden");
    titleEl.textContent = "正在更新";
    subtitleEl.textContent = "正在准备...";
    setProgress("开始下载", 5, "请稍候");
    try {
      await invoke("do_update");
    } catch (e) {
      errorEl.textContent = String(e);
      errorEl.style.display = "block";
      titleEl.textContent = "更新失败";
      updateBtn.classList.remove("hidden");
      laterBtn.classList.remove("hidden");
    }
  });

  laterBtn.addEventListener("click", async () => {
    await getCurrentWindow().close();
  });
}

void init();

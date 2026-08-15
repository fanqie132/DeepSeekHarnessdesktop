import { listen } from "@tauri-apps/api/event";

const DSH_URL = "http://127.0.0.1:3080";
const POLL_INTERVAL_MS = 500;
const MAX_ATTEMPTS = 600;

const statusEl = document.getElementById("status") as HTMLElement;
const progressWrapEl = document.getElementById("progress-wrap") as HTMLElement;
const progressFillEl = document.getElementById("progress-fill") as HTMLElement;
const progressTextEl = document.getElementById("progress-text") as HTMLElement;

interface ProgressPayload {
  stage: "download" | "extract";
  current?: number;
  total?: number;
  message?: string;
}

let attempts = 0;

function showProgress() {
  progressWrapEl.style.display = "block";
}

function fail(reason: string) {
  statusEl.textContent = reason;
}

async function checkReady(): Promise<boolean> {
  try {
    await fetch(DSH_URL, { mode: "no-cors" });
    return true;
  } catch {
    return false;
  }
}

async function poll() {
  attempts += 1;
  if (await checkReady()) {
    window.location.href = DSH_URL;
    return;
  }
  if (attempts >= MAX_ATTEMPTS) {
    fail(`启动超时（${Math.round((attempts * POLL_INTERVAL_MS) / 1000)}s）。请检查后重新启动应用。`);
    return;
  }
  // 若已有明确的运行时进度提示，则轮询期间不覆盖其文案
  if (progressWrapEl.style.display === "none") {
    statusEl.textContent = `正在启动 DeepSeek Harness... (${Math.round((attempts * POLL_INTERVAL_MS) / 1000)}s)`;
  }
  setTimeout(poll, POLL_INTERVAL_MS);
}

// 接收 Rust 侧运行时下载/解压进度
void listen<ProgressPayload>("runtime-progress", (event) => {
  const p = event.payload;
  if (p.stage === "download") {
    showProgress();
    statusEl.textContent = p.message ?? "正在下载运行时...";
    progressTextEl.textContent = "";
    progressFillEl.style.width = "0%";
  } else if (p.stage === "extract" && p.total) {
    showProgress();
    const percent = Math.min(100, Math.round(((p.current ?? 0) / p.total) * 100));
    progressFillEl.style.width = `${percent}%`;
    progressTextEl.textContent = `正在安装运行时 ${percent}%`;
  }
});

void poll();

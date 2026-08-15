const DSH_URL = "http://127.0.0.1:3080";
const POLL_INTERVAL_MS = 500;
const MAX_ATTEMPTS = 120;

const statusEl = document.getElementById("status") as HTMLElement;
let attempts = 0;

async function checkReady(): Promise<boolean> {
  try {
    await fetch(DSH_URL, { mode: "no-cors" });
    return true;
  } catch {
    return false;
  }
}

function fail(reason: string) {
  statusEl.textContent = reason;
}

async function poll() {
  attempts += 1;
  if (await checkReady()) {
    window.location.href = DSH_URL;
    return;
  }
  if (attempts >= MAX_ATTEMPTS) {
    fail(`dsh 启动超时（${Math.round((attempts * POLL_INTERVAL_MS) / 1000)}s）。请检查后重新启动应用。`);
    return;
  }
  statusEl.textContent = `正在启动 DeepSeek Harness... (${Math.round((attempts * POLL_INTERVAL_MS) / 1000)}s)`;
  setTimeout(poll, POLL_INTERVAL_MS);
}

void poll();

import { listen } from "@tauri-apps/api/event";

const DSH_URL = "http://127.0.0.1:3080";
const POLL_INTERVAL_MS = 500;
const MAX_ATTEMPTS = 600;

const statusEl = document.getElementById("status") as HTMLElement;
const quoteEl = document.getElementById("quote") as HTMLElement;
const progressWrapEl = document.getElementById("progress-wrap") as HTMLElement;
const progressFillEl = document.getElementById("progress-fill") as HTMLElement;
const progressTextEl = document.getElementById("progress-text") as HTMLElement;

// 日常启动文案库：每次启动随机抽取一条
const QUOTES = [
  // 暖系
  "都收拾好了，放心开干",
  "你的东西都还在，鲸鱼替你守着",
  "慢慢来，鲸鱼不急",
  "今天也辛苦你了",
  "泡杯热茶，咱们慢慢来",
  "别慌，这一步鲸鱼替你盯着",
  // 俏系
  "打开得有点快，要不要重新来一次？",
  "又见面了，今天想干点啥？",
  "你来得正好，鲸鱼正要问中午吃啥",
  "老板没看见，放心",
  "今天也是想准时下班的一天",
  "摸鱼一时爽，一直摸鱼一直爽",
  // 劲系
  "开工大吉，今天干点大事",
  "问题不大，鲸鱼罩着你",
  "冲！鲸鱼已经把路探好了",
  "今天能行，你昨天就行过",
  "深呼吸，搞定它",
  // 知系
  "蓝鲸的心脏，有一辆小汽车那么大",
  "北极熊的皮肤其实是黑色的",
  "章鱼有三颗心脏，你才一颗",
  "蜂蜜永远不会变质",
  "你手机的算力，比登月电脑强几百万倍",
  "地球每秒自转 465 米，你正坐着一艘超音速飞船",
  "蜗牛能睡三年，你只睡八小时",
  // 梗系
  "正在把进度条偷偷调快",
  "加载中，别眨眼，错过可别怪我",
  "鲸鱼：我不是慢，是优雅",
  "已经启动好了，快得连进度条都没反应过来",
  "鲸鱼去泡了个澡，马上回",
  "温馨提示：这里没有进度条，因为你太快了",
];

// 解压进度叙事文案（按完成度分段）
function extractText(percent: number): string {
  if (percent < 20) return "正在给鲸鱼腾出海洋…";
  if (percent < 40) return "鲸鱼正在安家…";
  if (percent < 60) return "正在布置鲸鱼的新房间…";
  if (percent < 80) return "鲸鱼在窗边晒太阳…";
  return "马上就好，鲸鱼在穿正装…";
}

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
  quoteEl.textContent = "";
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
  if (progressWrapEl.style.display === "none") {
    statusEl.textContent = "正在启动 DeepSeek Harness...";
  }
  setTimeout(poll, POLL_INTERVAL_MS);
}

// 启动时随机抽取一句文案
quoteEl.textContent = QUOTES[Math.floor(Math.random() * QUOTES.length)];

// 接收 Rust 侧运行时下载/解压进度
void listen<ProgressPayload>("runtime-progress", (event) => {
  const p = event.payload;
  if (p.stage === "download") {
    showProgress();
    quoteEl.textContent = "";
    statusEl.textContent = "正在把鲸鱼运到你的电脑…";
    progressTextEl.textContent = "";
    progressFillEl.style.width = "0%";
  } else if (p.stage === "extract" && p.total) {
    showProgress();
    const percent = Math.min(100, Math.round(((p.current ?? 0) / p.total) * 100));
    progressFillEl.style.width = `${percent}%`;
    progressTextEl.textContent = `${extractText(percent)} ${percent}%`;
  }
});

void poll();

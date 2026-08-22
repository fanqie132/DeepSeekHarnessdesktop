import { listen } from "@tauri-apps/api/event";

const DSH_URL = "http://127.0.0.1:3080";
const POLL_INTERVAL_MS = 500;
const MAX_ATTEMPTS = 600;

// 场景区分：托盘"重启服务"会带 ?restart=1 导航回本页
const isRestart = new URLSearchParams(window.location.search).has("restart");
const TEXT_STARTING = isRestart ? "正在重启服务..." : "正在启动 DeepSeek Harness...";
const TEXT_TIMEOUT = (sec: number) =>
  isRestart
    ? `重启超时（${sec}s）。请检查后重新启动应用。`
    : `启动超时（${sec}s）。请检查后重新启动应用。`;

const statusEl = document.getElementById("status") as HTMLElement;
const quoteEl = document.getElementById("quote") as HTMLElement;
const progressWrapEl = document.getElementById("progress-wrap") as HTMLElement;
const progressFillEl = document.getElementById("progress-fill") as HTMLElement;
const progressTextEl = document.getElementById("progress-text") as HTMLElement;
const progressHintEl = document.getElementById("progress-hint") as HTMLElement;

// 日常趣味文案库（副文案，定时轮换）
const QUOTES = [
  "都收拾好了，放心开干",
  "你的东西都还在，鲸鱼替你守着",
  "慢慢来，鲸鱼不急",
  "今天也辛苦你了",
  "泡杯热茶，咱们慢慢来",
  "别慌，这一步鲸鱼替你盯着",
  "打开得有点快，要不要重新来一次？",
  "又见面了，今天想干点啥？",
  "你来得正好，鲸鱼正要问中午吃啥",
  "老板没看见，放心",
  "今天也是想准时下班的一天",
  "摸鱼一时爽，一直摸鱼一直爽",
  "开工大吉，今天干点大事",
  "问题不大，鲸鱼罩着你",
  "冲！鲸鱼已经把路探好了",
  "今天能行，你昨天就行过",
  "深呼吸，搞定它",
  "蓝鲸的心脏，有一辆小汽车那么大",
  "北极熊的皮肤其实是黑色的",
  "章鱼有三颗心脏，你才一颗",
  "蜂蜜永远不会变质",
  "你手机的算力，比登月电脑强几百万倍",
  "地球每秒自转 465 米，你正坐着一艘超音速飞船",
  "蜗牛能睡三年，你只睡八小时",
  "正在把进度条偷偷调快",
  "加载中，别眨眼，错过可别怪我",
  "鲸鱼：我不是慢，是优雅",
  "已经启动好了，快得连进度条都没反应过来",
  "鲸鱼去泡了个澡，马上回",
  "温馨提示：这里没有进度条，因为你太快了",
];

// 进度叙事文案：每 10% 固定一句，顺序对应进度位置（保证全部展示）
const EXTRACT_TEXTS = [
  "正在给鲸鱼腾出海洋…", // 0-10%
  "鲸鱼正在搬运行李…", // 10-20%
  "鲸鱼在装修新家…", // 20-30%
  "给鲸鱼装上门窗…", // 30-40%
  "鲸鱼在布置家具…", // 40-50%
  "正在挂上鲸鱼的相框…", // 50-60%
  "给鲸鱼接通水电…", // 60-70%
  "鲸鱼在做大扫除…", // 70-80%
  "鲸鱼在窗边晒太阳…", // 80-90%
  "马上就好，鲸鱼在穿正装…", // 90-100%
];

const QUOTE_MS = 4000; // 副文案轮换间隔

function randomFrom(arr: string[]): string {
  return arr[Math.floor(Math.random() * arr.length)];
}

function randomQuote(): string {
  let q = randomFrom(QUOTES);
  if (QUOTES.length > 1) {
    while (q === quoteEl.textContent) q = randomFrom(QUOTES);
  }
  return q;
}

function extractIndex(percent: number): number {
  return Math.min(9, Math.floor(percent / 10));
}

interface ProgressPayload {
  stage: "download" | "extract";
  current?: number;
  total?: number;
  message?: string;
}

let attempts = 0;
let quoteTimer: number | null = null;

function showProgress() {
  progressWrapEl.style.display = "block";
}

function fail(reason: string) {
  statusEl.textContent = reason;
  quoteEl.textContent = "";
  if (quoteTimer) clearInterval(quoteTimer);
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
    fail(TEXT_TIMEOUT(Math.round((attempts * POLL_INTERVAL_MS) / 1000)));
    return;
  }
  if (progressWrapEl.style.display === "none") {
    statusEl.textContent = TEXT_STARTING;
  }
  setTimeout(poll, POLL_INTERVAL_MS);
}

// 副文案：固定"正在启动"文案下方，每 4 秒随机轮换一句
quoteEl.textContent = randomQuote();
quoteTimer = window.setInterval(() => {
  quoteEl.textContent = randomQuote();
}, QUOTE_MS);

// 接收 Rust 侧运行时下载/解压进度
void listen<ProgressPayload>("runtime-progress", (event) => {
  const p = event.payload;
  if (p.stage === "download") {
    showProgress();
    progressTextEl.textContent = "";
    progressHintEl.textContent = "首次运行需安装运行环境，可能需要几分钟，请耐心等待";
    progressFillEl.style.width = "0%";
  } else if (p.stage === "extract" && p.total) {
    showProgress();
    const percent = Math.min(100, Math.round(((p.current ?? 0) / p.total) * 100));
    progressFillEl.style.width = `${percent}%`;
    progressTextEl.textContent = `${EXTRACT_TEXTS[extractIndex(percent)]} ${percent}%`;
    progressHintEl.textContent = "首次运行需安装运行环境，可能需要几分钟，请耐心等待";
  }
});

void poll();

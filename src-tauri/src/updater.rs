use std::fs;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use semver::Version;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::dsh::DshManager;
use crate::runtime;

const REGISTRY_URLS: &[&str] = &[
  "https://registry.npmjs.org/@deepseek-ai/dsh",
  "https://registry.npmmirror.com/@deepseek-ai/dsh",
];
const DSH_HOST: &str = "127.0.0.1";
const DSH_PORT: u16 = 3080;
const LABEL_RESTART: &str = "重启更新";
const LABEL_LATER: &str = "稍后";

/// 启动后延迟数秒，在后台检查 dsh 是否有新版本。
pub fn spawn_check(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));

        let latest = match fetch_latest_version() {
            Ok(v) => v,
            Err(_) => return, // 网络不可用等：静默跳过本次检查
        };
        let current = match read_local_version(&app) {
            Ok(v) => v,
            Err(_) => return,
        };

        if compare_version(&latest, &current) != std::cmp::Ordering::Greater {
            return;
        }

        // 写更新日志，便于排查“点更新没反应”
        let _ = fs::write(
            std::env::temp_dir().join("dsh-updater.log"),
            format!("check latest={} current={}\n", latest, current),
        );
        let app_for_dialog = app.clone();
        app.dialog()
            .message(format!(
                "发现新版本 v{latest}（当前 v{current}）。\n点“重启更新”将下载约 70MB 并自动重启，期间请勿关闭窗口。"
            ))
            .title("DeepSeek Harness 更新")
            .buttons(MessageDialogButtons::OkCancelCustom(
                LABEL_RESTART.to_string(),
                LABEL_LATER.to_string(),
            ))
            .show(move |result| {
                let _ = fs::write(
                    std::env::temp_dir().join("dsh-updater.log"),
                    format!("click result={result}\n"),
                );
                if result {
                    let app = app_for_dialog.clone();
                    // 先弹“正在更新”阻塞提示，避免用户以为只是刷新
                    let _ = app
                        .dialog()
                        .message("正在下载并更新运行环境（约 70MB），完成后将自动刷新页面，请稍候…")
                        .title("正在更新")
                        .show(|_| {});
                    std::thread::spawn(move || {
                        let _ = fs::write(
                            std::env::temp_dir().join("dsh-updater.log"),
                            "start update_runtime\n",
                        );
                        match update_runtime(&app) {
                            Ok(()) => {
                                let _ = fs::write(
                                    std::env::temp_dir().join("dsh-updater.log"),
                                    "update ok, reload\n",
                                );
                                // 更新后重读版本，确认已变
                                if let Ok(v) = read_local_version(&app) {
                                    let _ = app.dialog().message(format!("更新完成，当前版本 v{v}，即将刷新页面")).title("更新完成").show(|_| {});
                                }
                                reload_after_ready(&app);
                            }
                            Err(e) => {
                                let _ = fs::write(
                                    std::env::temp_dir().join("dsh-updater-update-error.log"),
                                    &e,
                                );
                                let _ = app
                                    .dialog()
                                    .message(format!("更新失败：{e}"))
                                    .title("更新失败")
                                    .show(|_| {});
                            }
                        }
                    });
                }
            });
    });
}

/// 从 npm registry 读取 dist-tags.latest（主源失败自动切镜像）。
fn fetch_latest_version() -> Result<String, Box<dyn std::error::Error>> {
    let mut last_err: Option<Box<dyn std::error::Error>> = None;
    for url in REGISTRY_URLS {
        match (|| -> Result<String, Box<dyn std::error::Error>> {
            let body = ureq::get(*url)
                .timeout(Duration::from_secs(15))
                .call()?
                .into_string()?;
            let json: serde_json::Value = serde_json::from_str(&body)?;
            json["dist-tags"]["latest"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "registry 响应缺少 dist-tags.latest".into())
        })() {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| "registry 均不可用".into()))
}

/// 读取本地 runtime 中 dsh 的版本。
fn read_local_version(app: &AppHandle) -> Result<String, Box<dyn std::error::Error>> {
    let pkg = runtime::runtime_dir(app)
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("package.json");
    let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(pkg)?)?;
    json["version"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "runtime dsh package.json 缺少 version".into())
}

fn compare_version(a: &str, b: &str) -> std::cmp::Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        _ => a.cmp(b),
    }
}

/// 更新 runtime：先停 dsh（释放文件锁），下载最新 runtime.zip 替换，再重启。
fn update_runtime(app: &AppHandle) -> Result<(), String> {
    if let Some(manager) = app.try_state::<DshManager>() {
        manager.stop();
        // 多等一会确保文件句柄释放（Windows 文件锁）
        std::thread::sleep(Duration::from_secs(2));
    }
    // 重试一次下载（网络抖动）
    let mut last_err = String::new();
    for attempt in 1..=2 {
        match runtime::fetch_and_replace_runtime(app) {
            Ok(()) => {
                if let Some(manager) = app.try_state::<DshManager>() {
                    manager.start();
                }
                return Ok(());
            }
            Err(e) => {
                last_err = e;
                if attempt == 1 {
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    }
    // 失败也尝试拉起旧版，避免服务挂掉
    if let Some(manager) = app.try_state::<DshManager>() {
        manager.start();
    }
    Err(format!("{last_err}（已重试）\n日志：%LOCALAPPDATA%/com.dsh.desktop/dsh.log, %TEMP%/dsh-runtime-download-curl.log）"))
}

/// 等待 dsh 端口就绪后重载主窗口。
fn reload_after_ready(app: &AppHandle) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if TcpStream::connect((DSH_HOST, DSH_PORT)).is_ok() {
            break;
        }
        if Instant::now() > deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval("window.location.reload()");
    }
}

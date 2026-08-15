use std::fs;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use semver::Version;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::dsh::DshManager;
use crate::runtime;

const REGISTRY_URL: &str = "https://registry.npmmirror.com/@deepseek-ai/dsh";
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

        let app_for_dialog = app.clone();
        app.dialog()
            .message(format!(
                "发现新版本 v{latest}（当前 v{current}）。重启应用以更新？"
            ))
            .title("DeepSeek Harness 更新")
            .buttons(MessageDialogButtons::OkCancelCustom(
                LABEL_RESTART.to_string(),
                LABEL_LATER.to_string(),
            ))
            .show(move |result| {
                if result {
                    let app = app_for_dialog.clone();
                    std::thread::spawn(move || match update_runtime(&app) {
                        Ok(()) => reload_after_ready(&app),
                        Err(e) => {
                            let _ = app
                                .dialog()
                                .message(format!("更新失败：{e}"))
                                .title("更新失败")
                                .show(|_| {});
                        }
                    });
                }
            });
    });
}

/// 从 npm registry 读取 dist-tags.latest。
fn fetch_latest_version() -> Result<String, Box<dyn std::error::Error>> {
    let body = ureq::get(REGISTRY_URL)
        .timeout(Duration::from_secs(20))
        .call()?
        .into_string()?;
    let json: serde_json::Value = serde_json::from_str(&body)?;
    json["dist-tags"]["latest"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "registry 响应缺少 dist-tags.latest".into())
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
    }
    let result = runtime::fetch_and_replace_runtime(app);
    if let Some(manager) = app.try_state::<DshManager>() {
        manager.start();
    }
    result
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

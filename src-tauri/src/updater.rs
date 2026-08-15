use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use semver::Version;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::dsh::DshManager;

const REGISTRY_URL: &str = "https://registry.npmmirror.com/@deepseek-ai/dsh";
const DSH_HOST: &str = "127.0.0.1";
const DSH_PORT: u16 = 3080;
const LABEL_RESTART: &str = "重启更新";
const LABEL_LATER: &str = "稍后";

/// runtime 根目录：开发期用项目内 runtime，发布期用打包资源目录。
fn runtime_dir(app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 应有父目录")
            .join("runtime")
    } else {
        crate::strip_verbatim(app.path().resource_dir().expect("无法定位资源目录")).join("runtime")
    }
}

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
                            Ok(()) => {
                                if let Some(manager) = app.try_state::<DshManager>() {
                                    manager.restart();
                                }
                                reload_after_ready(&app);
                            }
                            Err(e) => {
                                let _ = app.dialog().message(format!("更新失败：{e}"))
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
    let body = ureq::get(REGISTRY_URL).timeout(Duration::from_secs(20)).call()?.into_string()?;
    let json: serde_json::Value = serde_json::from_str(&body)?;
    json["dist-tags"]["latest"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "registry 响应缺少 dist-tags.latest".into())
}

/// 读取本地 runtime 中 dsh 的版本。
fn read_local_version(app: &AppHandle) -> Result<String, Box<dyn std::error::Error>> {
    let pkg = runtime_dir(app)
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

/// 用 pnpm 更新 runtime 内的 dsh 包到最新版。
fn update_runtime(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let pnpm = resolve_pnpm();
    let status = Command::new(&pnpm)
        .current_dir(runtime_dir(app))
        .args(["add", "@deepseek-ai/dsh@latest"])
        .status()?;
    if !status.success() {
        return Err(format!("pnpm 更新失败（exit {:?}）", status.code()).into());
    }
    Ok(())
}

/// 定位 pnpm 命令：优先 PATH，回退到 D:\pnpm\bin。
fn resolve_pnpm() -> PathBuf {
    if which("pnpm").is_some() {
        PathBuf::from("pnpm")
    } else {
        PathBuf::from("D:\\pnpm\\bin\\pnpm.cmd")
    }
}

fn which(name: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(if cfg!(windows) { format!("{name}.cmd") } else { name.to_string() });
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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

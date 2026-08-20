use std::fs;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use semver::Version;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::dsh::DshManager;
use crate::runtime;

const REGISTRY_URLS: &[&str] = &[
  "https://registry.npmjs.org/@deepseek-ai/dsh",
  "https://registry.npmmirror.com/@deepseek-ai/dsh",
];
const DSH_HOST: &str = "127.0.0.1";
const DSH_PORT: u16 = 3080;

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

        // 写更新日志（追加）
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_default();
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(std::env::temp_dir().join("dsh-updater.log"))
                .and_then(|mut f| writeln!(f, "[{}] check latest={} current={}", ts, latest, current));
        }
        // 独立更新窗口（白底蓝鲸鱼，420x340），替代系统弹窗
        if let Err(e) = open_updater_window(&app, latest.clone(), current.clone()) {
            use std::io::Write;
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(std::env::temp_dir().join("dsh-updater.log"))
                .and_then(|mut f| writeln!(f, "open updater window failed: {}", e));
        }
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

fn open_updater_window(app: &AppHandle, latest: String, current: String) -> Result<(), String> {
    // 已有则聚焦
    if let Some(w) = app.get_webview_window("updater") {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = format!("updater.html?latest={}&current={}", latest, current);
    let win = WebviewWindowBuilder::new(app, "updater", WebviewUrl::App(url.into()))
        .title("DeepSeek Harness 更新")
        .inner_size(420.0, 340.0)
        .center()
        .resizable(false)
        .decorations(true)
        .build()
        .map_err(|e| e.to_string())?;
    // 3秒后若前端未拉取参数，主动推送一次
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        let _ = win.emit(
            "updater-info",
            serde_json::json!({ "latest": latest, "current": current }),
        );
        // 也确保主窗口能收到（调试）
        let _ = app2.emit("updater-info", serde_json::json!({ "latest": latest, "current": current }));
    });
    Ok(())
}

#[tauri::command]
pub fn do_update(app: AppHandle) -> Result<(), String> {
    {
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("dsh-updater.log"))
            .and_then(|mut f| writeln!(f, "do_update invoked"));
    }
    // 立即隐藏主窗口，只留更新小窗，避免“主窗口还开着”的错觉
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        // 完全退出式：先停服务，释放文件锁
        if let Some(manager) = app2.try_state::<DshManager>() {
            manager.stop();
            std::thread::sleep(Duration::from_secs(3));
        }
        let result = (|| -> Result<(), String> {
            let mut last_err = String::new();
            for attempt in 1..=2 {
                match runtime::fetch_and_replace_runtime(&app2) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        last_err = e;
                        if attempt == 1 {
                            std::thread::sleep(Duration::from_secs(2));
                        }
                    }
                }
            }
            Err(last_err)
        })();

        match result {
            Ok(()) => {
                if let Some(manager) = app2.try_state::<DshManager>() {
                    manager.start();
                }
                let _ = app2.emit("updater-done", "更新完成，正在重启...");
                // 等服务就绪后，重新显示主窗口并关闭更新窗口
                reload_after_ready(&app2);
                if let Some(w) = app2.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                    let _ = w.eval("window.location.reload()");
                }
                std::thread::sleep(Duration::from_secs(1));
                if let Some(w) = app2.get_webview_window("updater") {
                    let _ = w.close();
                }
            }
            Err(e) => {
                let _ = app2.emit("updater-error", e.clone());
                let _ = std::fs::write(
                    std::env::temp_dir().join("dsh-updater-update-error.log"),
                    &e,
                );
                // 失败也尝试拉起旧版，并把主窗口显示回来
                if let Some(manager) = app2.try_state::<DshManager>() {
                    manager.start();
                }
                if let Some(w) = app2.get_webview_window("main") {
                    let _ = w.show();
                }
            }
        }
    });
    Ok(())
}

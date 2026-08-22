use std::fs;
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use semver::Version;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::dsh::DshManager;
use crate::runtime;

/// Release 资产的版本元数据（CI 同步时生成，几字节）。
/// 客户端只认这个文件：它存在且更新意味着 runtime.zip 必定已就绪，
/// 避免"盯 npm 弹窗、下载却是旧包"的时序错位。
const RUNTIME_VERSION_URL: &str =
    "https://github.com/fanqie132/dsh-desktop/releases/download/runtime/runtime-version.txt";

/// 追加一行到 %TEMP%/dsh-updater.log（时间戳为 Unix 秒）。成功与失败都记录，
/// 避免"静默假成功"无法排查。
fn log_updater(msg: &str) {
    use std::io::Write;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("dsh-updater.log"))
        .and_then(|mut f| writeln!(f, "[{ts}] {msg}"));
}

/// 启动后延迟数秒，在后台检查 dsh 是否有新版本。
pub fn spawn_check(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));

        let latest = match fetch_latest_version() {
            Ok(v) => v,
            Err(e) => {
                log_updater(&format!("check skipped: cannot fetch version.txt ({e})"));
                return; // 网络不可用等：跳过本次检查（有日志可查）
            }
        };
        let current = match read_local_version(&app) {
            Ok(v) => v,
            Err(e) => {
                log_updater(&format!("check skipped: cannot read local version ({e})"));
                return;
            }
        };

        if compare_version(&latest, &current) != std::cmp::Ordering::Greater {
            return;
        }

        log_updater(&format!("check latest={latest} current={current}"));
        // 独立更新窗口（白底蓝鲸鱼，420x340），替代系统弹窗
        if let Err(e) = open_updater_window(&app, latest.clone(), current.clone()) {
            log_updater(&format!("open updater window failed: {e}"));
        }
    });
}

/// 从 Release 资产读取可更新目标版本（CI 同步完成后才会变化）。
/// 优先系统 curl（自动继承 HTTP(S)_PROXY 代理环境变量，与 runtime.zip 下载同策略——
/// github.com 在部分网络环境必须走代理，ureq 直连不通），失败回退 ureq 直连。
fn fetch_latest_version() -> Result<String, Box<dyn std::error::Error>> {
    let mut cmd = Command::new("curl");
    cmd.args(["-s", "-L", "--fail", "--max-time", "15", RUNTIME_VERSION_URL]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return Ok(v);
            }
        }
    }
    // 回退：ureq 直连（无代理但可直达 GitHub 的环境）
    let body = ureq::get(RUNTIME_VERSION_URL)
        .timeout(Duration::from_secs(15))
        .call()?
        .into_string()?;
    let v = body.trim().to_string();
    if v.is_empty() {
        return Err("runtime-version.txt 内容为空".into());
    }
    Ok(v)
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

fn open_updater_window(app: &AppHandle, latest: String, current: String) -> Result<(), String> {
    // 已有则聚焦
    if let Some(w) = app.get_webview_window("updater") {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = format!("updater.html?latest={}&current={}", latest, current);
    let win = WebviewWindowBuilder::new(app, "updater", WebviewUrl::App(url.into()))
        .title("DeepSeek Harness 更新")
        .inner_size(420.0, 380.0)
        .center()
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
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
pub fn do_update(app: AppHandle, target_version: Option<String>) -> Result<(), String> {
    let before = read_local_version(&app)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "?".into());
    log_updater(&format!(
        "update start: v{before} -> {}",
        target_version.as_deref().unwrap_or("?")
    ));
    // 立即隐藏主窗口，只留更新小窗，避免“主窗口还开着”的错觉
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        // 目标版本兜底：前端未传时自己拉一次 version.txt（供自检比对）
        let target = target_version.or_else(|| fetch_latest_version().ok());
        if target.is_none() {
            log_updater("self-check degraded: no target version available");
        }
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

        // 自检闭环：替换"成功"不等于更新成功——回读实际安装的版本，
        // 低于目标版本说明下载到的资产过期，按失败处理而非静默
        let outcome: Result<String, String> = match result {
            Ok(()) => {
                let actual = read_local_version(&app2)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|_| "?".into());
                let stale = matches!(&target,
                    Some(t) if compare_version(&actual, t) == std::cmp::Ordering::Less);
                if stale {
                    Err(format!(
                        "更新自检失败：期望 v{}，实际 v{}（下载到的资产已过期，请稍后重试）",
                        target.as_deref().unwrap_or("?"),
                        actual
                    ))
                } else {
                    Ok(actual)
                }
            }
            Err(e) => Err(e),
        };

        match outcome {
            Ok(actual) => {
                log_updater(&format!("update ok: v{before} -> v{actual} (self-check passed)"));
                if let Some(manager) = app2.try_state::<DshManager>() {
                    manager.start();
                }
                let _ = app2.emit(
                    "updater-done",
                    format!("已从 v{before} 更新到 v{actual}，正在重启..."),
                );
                // 等服务就绪后，重新显示主窗口并关闭更新窗口
                crate::tray::reload_when_ready(&app2);
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
                log_updater(&format!("update failed: {e}"));
                let _ = app2.emit("updater-error", e.clone());
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

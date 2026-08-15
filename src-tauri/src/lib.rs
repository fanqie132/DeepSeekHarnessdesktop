mod dsh;
mod runtime;
mod tray;
mod updater;

use dsh::DshManager;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{Manager, WindowEvent};
use tauri_plugin_dialog::DialogExt;

/// 去掉 Windows verbatim 路径前缀（`\\?\`），node 等程序无法解析该格式。
pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().into_owned();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    PathBuf::from(s)
}

/// 运行时资源根目录：开发期用项目内 runtime，发布期用打包资源目录。
fn runtime_base(app: &tauri::App) -> PathBuf {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 应有父目录")
            .to_path_buf()
    } else {
        strip_verbatim(
            app.path()
                .resource_dir()
                .expect("无法定位资源目录")
                .to_path_buf(),
        )
    }
}

/// node 可执行文件：开发期用系统 PATH 中的 node，发布期用捆绑的 node.exe。
fn resolve_node(app: &tauri::App) -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from("node")
    } else {
        runtime_base(app).join("node").join("node.exe")
    }
}

/// dsh 子进程日志文件位置。
fn resolve_dsh_log(handle: &tauri::AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 应有父目录")
            .join("dsh.log")
    } else {
        strip_verbatim(handle.path().resource_dir().expect("无法定位资源目录")).join("dsh.log")
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let node = resolve_node(app);
            let entry = runtime::runtime_entry(&app.handle());
            let log = resolve_dsh_log(app.handle());
            let manager = DshManager::new(node, entry, log);
            app.manage(manager);

            // 关闭窗口时隐藏到托盘（托盘“退出”才真正退出）
            if let Some(window) = app.get_webview_window("main") {
                let win = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win.hide();
                    }
                });
            }

            tray::setup(app)?;
            updater::spawn_check(app.handle().clone());

            // 后台确保 runtime 就绪（首次启动需下载），就绪后拉起 dsh
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                if !runtime::is_ready(&handle) {
                    if let Err(e) = runtime::fetch_and_replace_runtime(&handle) {
                        let log = resolve_dsh_log(&handle);
                        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log) {
                            let _ = writeln!(f, "[init] 运行时下载失败：{e}");
                        }
                        let _ = handle
                            .dialog()
                            .message(format!("运行时下载失败：{e}"))
                            .title("DeepSeek Harness 初始化失败")
                            .show(|_| {});
                        handle.exit(1);
                        return;
                    }
                }
                if let Some(manager) = handle.try_state::<DshManager>() {
                    manager.start();
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

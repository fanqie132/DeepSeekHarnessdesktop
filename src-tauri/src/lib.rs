mod dsh;
mod runtime;
mod tray;
mod updater;

use dsh::DshManager;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, WindowEvent};
use tauri_plugin_dialog::DialogExt;

/// “图片另存为”：下载图片到内存，弹系统保存框，写入所选路径。
#[tauri::command]
async fn save_image(app: tauri::AppHandle, url: String) -> Result<Option<String>, String> {
    let _ = log_dbg(&format!("save_image called: {url}"));
    let bytes = tokio::task::spawn_blocking(move || download_bytes(&url))
        .await
        .map_err(|e| e.to_string())??;
    let _ = log_dbg(&format!("downloaded {} bytes", bytes.len()));
    let path = app
        .dialog()
        .file()
        .set_title("保存图片")
        .add_filter("图片", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
        .blocking_save_file();
    let _ = log_dbg(&format!("save dialog returned: {:?}", path.is_some()));
    if let Some(p) = path {
        let pbuf: PathBuf = p.into_path().map_err(|e| format!("路径无效：{e}"))?;
        fs::write(&pbuf, &bytes).map_err(|e| format!("写入文件失败：{e}"))?;
        Ok(Some(pbuf.to_string_lossy().into_owned()))
    } else {
        Ok(None) // 用户取消
    }
}

fn log_dbg(msg: &str) -> Result<(), String> {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("dsh-saveimage.log"))
    {
        let _ = writeln!(f, "[{}] {msg}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0));
    }
    Ok(())
}

/// “图片另存为”（data URL 通道）：前端把图片转 base64 传入，弹系统保存框写入。
#[tauri::command]
async fn save_image_data(
    app: tauri::AppHandle,
    data: String,
    _filename: String,
) -> Result<Option<String>, String> {
    use base64::Engine;
    let base64_str = data
        .split(',')
        .nth(1)
        .ok_or_else(|| "无效的图片数据".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_str.trim())
        .map_err(|e| format!("解码图片失败：{e}"))?;
    let path = app
        .dialog()
        .file()
        .set_title("保存图片")
        .add_filter("图片", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
        .blocking_save_file();
    if let Some(p) = path {
        let pbuf: PathBuf = p.into_path().map_err(|e| format!("路径无效：{e}"))?;
        fs::write(&pbuf, &bytes).map_err(|e| format!("写入文件失败：{e}"))?;
        Ok(Some(pbuf.to_string_lossy().into_owned()))
    } else {
        Ok(None)
    }
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut reader = ureq::get(url)
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .map_err(|e| format!("下载失败：{e}"))?
        .into_reader();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).map_err(|e| format!("读取失败：{e}"))?;
    Ok(buf)
}

/// 注入到 dsh 页面的自定义右键菜单脚本：
/// 文本区域：复制 / 粘贴 / 全选；图片：复制 / 复制链接 / 另存为（去掉浏览器的“更多工具”）。
const CONTEXT_MENU_JS: &str = include_str!("../context-menu.js");

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
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(
                    std::env::temp_dir().join("dsh-pageload.log"),
                ) {
                    let _ = writeln!(f, "page finished: {}", payload.url());
                }
                let _ = webview.eval(CONTEXT_MENU_JS);
                // 检查 __TAURI__ 是否在远程页面可用
                let _ = webview.eval("setTimeout(function(){ try { fetch('http://127.0.0.1:9999/tauri-check?has=' + (!!window.__TAURI__)) } catch(e){} }, 2000)");
            }
        })
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
        .invoke_handler(tauri::generate_handler![save_image, save_image_data])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(manager) = app_handle.try_state::<DshManager>() {
                    manager.stop();
                }
            }
        });
}

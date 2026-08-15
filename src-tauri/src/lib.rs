mod dsh;
mod runtime;
mod tray;
mod updater;

use dsh::DshManager;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, WindowEvent};
use tauri_plugin_dialog::DialogExt;

/// 注入到 dsh 页面的自定义右键菜单脚本：
/// 文本区域：复制 / 粘贴 / 全选；图片：复制 / 复制链接 / 另存为（去掉浏览器的“更多工具”）。
const CONTEXT_MENU_JS: &str = r#"(function () {
  if (window.__dshCtxMenu) return;
  window.__dshCtxMenu = true;
  var menu = document.createElement('div');
  menu.id = '__dsh-ctxmenu';
  menu.style.cssText = 'position:fixed;z-index:2147483647;display:none;min-width:150px;background:#fff;border:1px solid rgba(0,0,0,.1);border-radius:8px;box-shadow:0 4px 16px rgba(0,0,0,.15);padding:4px;font-family:system-ui,sans-serif;font-size:13px;color:#1f2937;user-select:none;';
  document.documentElement.appendChild(menu);
  var dark = window.matchMedia('(prefers-color-scheme: dark)');
  function applyTheme() {
    if (dark.matches) {
      menu.style.background = '#1f2937'; menu.style.color = '#f3f4f6'; menu.style.borderColor = 'rgba(255,255,255,.12)';
    } else {
      menu.style.background = '#ffffff'; menu.style.color = '#1f2937'; menu.style.borderColor = 'rgba(0,0,0,.1)';
    }
  }
  dark.addEventListener('change', applyTheme); applyTheme();
  function hide() { menu.style.display = 'none'; }
  function show(x, y, items) {
    menu.innerHTML = '';
    items.forEach(function (it) {
      var item = document.createElement('div');
      item.textContent = it.label;
      item.style.cssText = 'padding:7px 12px;border-radius:6px;cursor:pointer;';
      item.addEventListener('mouseenter', function () { item.style.background = dark.matches ? 'rgba(255,255,255,.1)' : '#f3f4f6'; });
      item.addEventListener('mouseleave', function () { item.style.background = 'transparent'; });
      item.addEventListener('click', function () { hide(); if (it.action) it.action(); });
      menu.appendChild(item);
    });
    var r = menu.getBoundingClientRect();
    menu.style.left = Math.min(x, window.innerWidth - r.width - 4) + 'px';
    menu.style.top = Math.min(y, window.innerHeight - r.height - 4) + 'px';
    menu.style.display = 'block';
  }
  document.addEventListener('click', hide);
  document.addEventListener('scroll', hide, true);
  document.addEventListener('contextmenu', function (e) {
    var img = e.target.closest ? e.target.closest('img') : null;
    if (img) {
      e.preventDefault();
      var url = img.currentSrc || img.src;
      show(e.clientX, e.clientY, [
        { label: '复制图片', action: function () {
            fetch(url).then(function (r) { return r.blob(); }).then(function (b) {
              var t = b.type && b.type.indexOf('image/') === 0 ? b.type : 'image/png';
              try { navigator.clipboard.write([new ClipboardItem({ [t]: b })]); } catch (err) {}
            }).catch(function () {});
          } },
        { label: '复制图片链接', action: function () { navigator.clipboard.writeText(url); } },
        { label: '图片另存为', action: function () {
            var a = document.createElement('a');
            a.href = url;
            a.download = (img.alt || 'image') + '.' + ((url.split('.').pop() || 'png').split('?')[0]);
            a.click();
          } },
      ]);
      return;
    }
    e.preventDefault();
    show(e.clientX, e.clientY, [
      { label: '复制', action: function () { document.execCommand('copy'); } },
      { label: '粘贴', action: function () { document.execCommand('paste'); } },
      { label: '全选', action: function () { document.execCommand('selectAll'); } },
    ]);
  });
})();"#;

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
                let _ = webview.eval(CONTEXT_MENU_JS);
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
        .invoke_handler(tauri::generate_handler![])
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

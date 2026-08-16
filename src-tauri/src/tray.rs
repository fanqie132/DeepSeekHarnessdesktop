use std::net::TcpStream;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

use crate::dsh::DshManager;

/// 显示/恢复主窗口。
fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 创建系统托盘：显示窗口 / 重启 / 退出。
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "重启", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &restart, &quit])?;

    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().expect("缺少默认窗口图标").clone())
        .menu(&menu)
        .tooltip("DeepSeek Harness")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            // 左键单击/双击：恢复窗口
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "restart" => {
                // 完全结束旧进程（含子进程）后启动新进程，插件/环境变量随新进程生效
                if let Some(manager) = app.try_state::<DshManager>() {
                    manager.restart();
                }
                // 等待新 dsh 就绪后：恢复窗口并刷新页面（让用户感知重启完成）
                let app = app.clone();
                std::thread::spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(60);
                    loop {
                        if TcpStream::connect(("127.0.0.1", 3080)).is_ok() {
                            break;
                        }
                        if Instant::now() > deadline {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.eval("window.location.reload()");
                    }
                });
            }
            "quit" => {
                // 让独立 taskkill 后台清理 dsh，界面立即关闭
                if let Some(manager) = app.try_state::<DshManager>() {
                    manager.stop_detached();
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

use std::net::TcpStream;
use std::time::{Duration, Instant};

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::dsh::DshManager;

/// 显示/恢复主窗口。
fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 等待 dsh 端口就绪后刷新主窗口（重启/更新完成后的统一收尾）。
fn reload_when_ready(app: &AppHandle) {
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

/// 创建系统托盘：显示 / 重启 / 局域网访问开关 / 退出。
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "重启服务", true, None::<&str>)?;
    // B3：局域网转发器开关，默认关闭，状态持久化
    let lan_on = crate::forwarder_enabled(&app.handle());
    let lan = CheckMenuItem::with_id(app, "lan", "局域网访问（手机）", true, lan_on, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &restart, &lan, &quit])?;

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
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "restart" => {
                // 完全结束旧进程（含子进程）后启动新进程，插件/环境变量随新进程生效
                if let Some(manager) = app.try_state::<DshManager>() {
                    manager.restart();
                }
                reload_when_ready(app);
            }
            "lan" => {
                // MenuEvent 不携带勾选态：以持久化配置为准取反，再回写菜单显示
                let now_on = !crate::forwarder_enabled(app);
                let _ = lan.set_checked(now_on);
                apply_lan_switch(app, now_on);
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

/// 应用局域网访问开关：更新持久化 + 启停转发器 + 弹窗反馈连接信息。
fn apply_lan_switch(app: &tauri::AppHandle, on: bool) {
    use tauri_plugin_dialog::DialogExt;
    crate::set_forwarder_enabled(app, on);
    let (title, msg) = if !on {
        ("局域网访问", "已关闭局域网访问。".to_string())
    } else {
        let r = match app.try_state::<DshManager>() {
            Some(manager) => match manager.set_forwarder(true) {
                Some((url_base, token)) => (
                    "局域网访问已开启",
                    format!(
                        "手机浏览器打开：\n{url_base}/?token={token}\n\n（首次需信任自签名证书；电脑 IP 变化后会自动重签）"
                    ),
                ),
                None => ("开启失败", "无法获取局域网 IP 或生成证书，转发器未启动。".to_string()),
            },
            None => ("错误", "内部错误：服务未就绪。".to_string()),
        };
        r
    };
    let _ = app.dialog().message(msg).title(title).show(|_| {});
}

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

/// 打开/聚焦"手机扫码连接"小窗，推送连接地址（前端用 qrcode 库渲染二维码）。
fn open_connect_window(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("connect") {
        let _ = w.set_focus();
    } else {
        tauri::WebviewWindowBuilder::new(
            app,
            "connect",
            tauri::WebviewUrl::App("connect.html".into()),
        )
        .title("手机连接 - DeepSeek Harness")
        .inner_size(380.0, 520.0)
        .center()
        .resizable(false)
        .decorations(true)
        .build()
        .map_err(|e| e.to_string())?;
        // 窗口刚创建时前端可能还没挂好监听，稍后补推一次
        let app2 = app.clone();
        let url2 = url.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(600));
            if let Some(w) = app2.get_webview_window("connect") {
                use tauri::Emitter;
                let _ = w.emit("connect-info", serde_json::json!({ "url": url2 }));
            }
        });
    }
    use tauri::Emitter;
    let _ = app.emit("connect-info", serde_json::json!({ "url": url }));
    Ok(())
}

/// 关闭"手机扫码连接"小窗（关闭转发器时调用）。
fn close_connect_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("connect") {
        let _ = w.close();
    }
}

/// 应用局域网访问开关（纯开关职责）：开 → 拉起转发器并自动弹码；关 → 静默收窗。
fn apply_lan_switch(app: &tauri::AppHandle, on: bool) -> bool {
    crate::set_forwarder_enabled(app, on);
    if !on {
        close_connect_window(app);
        if let Some(manager) = app.try_state::<DshManager>() {
            manager.set_forwarder(false);
        }
        return true;
    }
    match open_and_get_connect_url(app) {
        Ok(url) => open_connect_window(app, &url).is_ok(),
        Err(_) => false,
    }
}

/// 幂等地确保转发器运行并返回完整连接地址（仅"lan"开关路径使用）。
fn open_and_get_connect_url(app: &tauri::AppHandle) -> Result<String, String> {
    match app.try_state::<DshManager>() {
        Some(manager) => match manager.set_forwarder(true) {
            Some((url_base, token)) => Ok(format!("{url_base}/?token={token}")),
            None => Err("无法获取局域网 IP 或生成证书/Token".to_string()),
        },
        None => Err("内部错误：服务未就绪".to_string()),
    }
}

/// 创建系统托盘：显示 / 重启 / 局域网开关 / 显示连接二维码 / 退出。
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    use tauri_plugin_dialog::DialogExt;

    let show = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "重启服务", true, None::<&str>)?;
    // 局域网转发器开关（默认关闭，状态持久化）
    let lan_on = crate::forwarder_enabled(&app.handle());
    let lan = CheckMenuItem::with_id(app, "lan", "局域网访问（手机）", true, lan_on, None::<&str>)?;
    let qr = MenuItem::with_id(app, "qr", "显示连接二维码", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &restart, &lan, &qr, &quit])?;

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
                let ok = apply_lan_switch(app, now_on);
                let _ = lan.set_checked(if ok { now_on } else { !now_on });
                if now_on && !ok {
                    let _ = app
                        .dialog()
                        .message("开启失败：无法获取局域网 IP 或生成证书/Token。")
                        .title("局域网访问")
                        .show(|_| {});
                }
            }
            "qr" => {
                // 纯展示：仅在转发器已运行时可用（零副作用），未开启则提示
                let r = app.try_state::<DshManager>().and_then(|m| m.connect_url());
                match r {
                    Some(url) => {
                        if let Err(e) = open_connect_window(app, &url) {
                            let _ = app.dialog().message(format!("打开失败：{e}")).title("手机连接").show(|_| {});
                        }
                    }
                    None => {
                        let _ = app
                            .dialog()
                            .message("请先勾选「局域网访问（手机）」以开启转发器。")
                            .title("手机连接")
                            .show(|_| {});
                    }
                }
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

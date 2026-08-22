use std::net::TcpStream;
use std::time::{Duration, Instant};

use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Position, WindowEvent};

use crate::dsh::DshManager;

/// 显示/恢复主窗口。
pub fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 等待 dsh 端口就绪后刷新主窗口（重启/更新完成后的统一收尾）。
pub fn reload_when_ready(app: &AppHandle) {
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
pub fn open_connect_window(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("connect") {
        let _ = w.set_focus();
    } else {
        tauri::WebviewWindowBuilder::new(
            app,
            "connect",
            tauri::WebviewUrl::App("connect.html".into()),
        )
        .title("手机连接 - DeepSeek Harness")
        .inner_size(380.0, 560.0)
        .center()
        .resizable(false)
        .decorations(false)
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

/// 幂等地确保转发器运行并返回完整连接地址（仅开关路径使用）。
fn open_and_get_connect_url(app: &tauri::AppHandle) -> Result<String, String> {
    match app.try_state::<DshManager>() {
        Some(manager) => match manager.set_forwarder(true) {
            Some((url_base, token)) => Ok(format!("{url_base}/?token={token}")),
            None => Err("无法获取局域网 IP 或生成证书/Token".to_string()),
        },
        None => Err("内部错误：服务未就绪".to_string()),
    }
}

/// 应用局域网访问开关（纯开关职责）：开 → 拉起转发器并自动弹码；关 → 静默收窗。
pub fn apply_lan_switch(app: &tauri::AppHandle, on: bool) -> bool {
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

/// 创建或复用托盘菜单面板（无边框透明小窗），并定位到托盘光标左上方。
fn toggle_menu_panel(app: &tauri::AppHandle, cursor: tauri::PhysicalPosition<f64>) {
    let win = match ensure_menu_window(app) {
        Ok(w) => w,
        Err(_) => return,
    };
    // 按主显示器 DPI 把逻辑尺寸换算成物理像素，再贴着光标左上摆放
    let sf = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let w_px = (224.0 * sf) as i32;
    let h_px = (250.0 * sf) as i32;
    let x = (cursor.x as i32 - w_px).max(0);
    let y = (cursor.y as i32 - h_px).max(0);
    let _ = win.set_position(Position::Physical(tauri::PhysicalPosition::new(x, y)));
    use tauri::Emitter;
    let _ = app.emit(
        "tray-state",
        serde_json::json!({ "lanOn": crate::forwarder_enabled(app) }),
    );
    let _ = win.show();
    let _ = win.set_focus();
}

/// 托盘菜单面板窗口：首次右键时创建，之后复用；失焦自动隐藏。
fn ensure_menu_window(app: &tauri::AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window("traymenu") {
        return Ok(w);
    }
    let w = tauri::WebviewWindowBuilder::new(
        app,
        "traymenu",
        tauri::WebviewUrl::App("traymenu.html".into()),
    )
    .visible(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .resizable(false)
    .inner_size(216.0, 244.0)
    .build()?;
    let w2 = w.clone();
    w.on_window_event(move |event| {
        // 点到面板外任意处即失焦，顺势隐藏，模拟原生菜单的关闭体验
        if matches!(event, WindowEvent::Focused(false)) {
            let _ = w2.hide();
        }
    });
    Ok(w)
}

/// 自绘托盘菜单的动作分发（由 traymenu.ts invoke 调用）。
#[tauri::command]
pub fn tray_menu_action(app: tauri::AppHandle, id: String) -> Result<(), String> {
    use tauri::Emitter;
    let hide_panel = |app: &tauri::AppHandle| {
        if let Some(w) = app.get_webview_window("traymenu") {
            let _ = w.hide();
        }
    };
    match id.as_str() {
        "show" => {
            hide_panel(&app);
            show_main(&app);
        }
        "restart" => {
            hide_panel(&app);
            if let Some(manager) = app.try_state::<DshManager>() {
                manager.restart();
            }
            reload_when_ready(&app);
        }
        "lan" => {
            let now_on = !crate::forwarder_enabled(&app);
            let ok = apply_lan_switch(&app, now_on);
            // 同步面板勾选态（面板保持展开，用户可继续操作或点外部关闭）
            let _ = app.emit("tray-state", serde_json::json!({ "lanOn": ok && now_on }));
            if now_on && !ok {
                use tauri_plugin_dialog::DialogExt;
                let _ = app
                    .dialog()
                    .message("开启失败：无法获取局域网 IP 或生成证书/Token。")
                    .title("局域网访问")
                    .show(|_| {});
            }
        }
        "qr" => {
            hide_panel(&app);
            let r = app.try_state::<DshManager>().and_then(|m| m.connect_url());
            match r {
                Some(url) => {
                    if let Err(e) = open_connect_window(&app, &url) {
                        use tauri_plugin_dialog::DialogExt;
                        let _ =
                            app.dialog().message(format!("打开失败：{e}")).title("手机连接").show(|_| {});
                    }
                }
                None => {
                    use tauri_plugin_dialog::DialogExt;
                    let _ = app
                        .dialog()
                        .message("请先开启「局域网访问（手机）」。")
                        .title("手机连接")
                        .show(|_| {});
                }
            }
        }
        "quit" => {
            if let Some(manager) = app.try_state::<DshManager>() {
                manager.stop_detached();
            }
            app.exit(0);
        }
        _ => {}
    }
    Ok(())
}

/// 创建系统托盘：左键恢复窗口，右键弹出自绘菜单面板。
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().expect("缺少默认窗口图标").clone())
        .tooltip("DeepSeek Harness")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            match event {
                // 左键单击/双击：恢复窗口
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => show_main(tray.app_handle()),
                // 右键抬起：在光标位置弹出自绘菜单面板
                TrayIconEvent::Click {
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    position,
                    ..
                } => toggle_menu_panel(tray.app_handle(), position),
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

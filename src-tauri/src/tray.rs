use std::net::TcpStream;
use std::time::{Duration, Instant};

use qrcodegen::{QrCode, QrCodeEcc};
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

/// 把 QrCode 渲染为自包含 SVG 字符串（白底黑码，前端 CSS 控制显示尺寸）。
fn qr_to_svg(qr: &QrCode, border: i32) -> String {
    let dim = qr.size() + border * 2;
    let mut path = String::new();
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            // 仅在黑色模块的左边缘起笔，压缩 path 长度
            if qr.get_module(x, y) && (x == 0 || !qr.get_module(x - 1, y)) {
                path.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {dim} {dim}\" shape-rendering=\"crispEdges\"><rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/><path fill=\"#000000\" d=\"{path}\"/></svg>"
    )
}

/// 打开/聚焦"手机扫码连接"小窗，推送二维码与地址。
fn open_connect_window(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    let qr = QrCode::encode_text(url, QrCodeEcc::Medium)
        .map_err(|e| format!("生成二维码失败：{e}"))?;
    let svg = qr_to_svg(&qr, 2);

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
        let svg2 = svg.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(600));
            if let Some(w) = app2.get_webview_window("connect") {
                use tauri::Emitter;
                let _ = w.emit(
                    "connect-info",
                    serde_json::json!({ "qrSvg": svg2, "url": url2 }),
                );
            }
        });
    }
    use tauri::Emitter;
    let _ = app.emit("connect-info", serde_json::json!({ "qrSvg": svg, "url": url }));
    Ok(())
}

/// 关闭"手机扫码连接"小窗（关闭转发器时调用）。
fn close_connect_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("connect") {
        let _ = w.close();
    }
}

/// 应用局域网访问开关：更新持久化 + 启停转发器 + 反馈。
/// 开启成功 → 打开二维码连接窗；关闭 → 静默收窗；失败 → 原生弹窗报错。
fn apply_lan_switch(app: &tauri::AppHandle, on: bool) {
    use tauri_plugin_dialog::DialogExt;
    crate::set_forwarder_enabled(app, on);
    if !on {
        close_connect_window(app);
        return;
    }
    let result = match app.try_state::<DshManager>() {
        Some(manager) => manager.set_forwarder(true).ok_or_else(|| "无法获取局域网 IP 或生成证书/Token，转发器未启动。".to_string()),
        None => Err("内部错误：服务未就绪。".to_string()),
    };
    match result {
        Ok((url_base, token)) => {
            let full = format!("{url_base}/?token={token}");
            if let Err(e) = open_connect_window(app, &full) {
                let _ = app
                    .dialog()
                    .message(format!("连接窗打开失败：{e}\n\n手动访问：{full}"))
                    .title("局域网访问已开启")
                    .show(|_| {});
            }
        }
        Err(e) => {
            let _ = app.dialog().message(e).title("开启失败").show(|_| {});
        }
    }
}

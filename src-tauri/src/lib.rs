#![allow(unexpected_cfgs)]

mod commands;
mod db;
mod monitor;

use commands::AppState;
use db::Database;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};
use tauri::tray::TrayIconBuilder;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt,
};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(ClipboardPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_dir = dirs_next().unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&app_dir).ok();
    let db_path = format!("{}/clipboard.db", app_dir);
    let db = Arc::new(Database::new(&db_path));

    let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
    let visible = Arc::new(AtomicBool::new(false));

    let visible_for_shortcut = visible.clone();
    let visible_for_tray = visible.clone();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build());

    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(tauri_nspanel::init());
    }

    builder
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut_event, event| {
                    if event.state() != ShortcutState::Pressed { return; }
                    // Only respond to Cmd+Shift+V
                    if shortcut_event.key != Code::KeyV { return; }
                    if visible_for_shortcut.load(Ordering::SeqCst) {
                        hide_panel(app);
                        visible_for_shortcut.store(false, Ordering::SeqCst);
                    } else {
                        show_panel(app);
                        visible_for_shortcut.store(true, Ordering::SeqCst);
                    }
                })
                .build(),
        )
        .manage(AppState { db: db.clone(), visible: visible.clone() })
        .setup(move |app| {
            // Hide dock icon, show only in menu bar
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            monitor::start_monitor(db.clone(), app.handle().clone());

            // Convert main window into NSPanel
            #[cfg(target_os = "macos")]
            {
                let window = app.get_webview_window("main").unwrap();
                let panel = window.to_panel::<ClipboardPanel>().unwrap();
                panel.set_level(PanelLevel::Floating.value());
                panel.set_style_mask(StyleMask::empty().nonactivating_panel().resizable().into());
                panel.set_collection_behavior(
                    CollectionBehavior::new()
                        .full_screen_auxiliary()
                        .can_join_all_spaces()
                        .into(),
                );
                unsafe {
                    use tauri_nspanel::objc2::msg_send;
                    let ns_panel = panel.as_panel();
                    let _: () = msg_send![ns_panel, setMovable: true];
                    let _: () = msg_send![ns_panel, setBecomesKeyOnlyIfNeeded: false];
                }
            }

            // Register global shortcut (Cmd+Shift+V); 被其他应用占用时降级为日志告警而不是启动失败
            if let Err(e) = app.global_shortcut().register(shortcut) {
                log::warn!("全局快捷键 ⌘⇧V 注册失败(可能被其他应用占用): {e}");
            }

            // Create system tray
            use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
            let show = MenuItemBuilder::with_id("show", "显示剪贴板  ⌘⇧V").build(app)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let quit = MenuItemBuilder::with_id("quit", "退出").accelerator("CmdOrCtrl+Q").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .item(&sep1)
                .item(&quit)
                .build()?;

            let visible_for_menu = visible.clone();
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AI 剪贴板")
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "quit" => app.exit(0),
                        "show" => {
                            show_panel(app);
                            visible_for_menu.store(true, Ordering::SeqCst);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(move |tray, event| {
                    use tauri::tray::{TrayIconEvent, MouseButton, MouseButtonState};
                    if let TrayIconEvent::Click { button, button_state, .. } = event {
                        if button == MouseButton::Left && button_state == MouseButtonState::Up {
                            let app = tray.app_handle();
                            if visible_for_tray.load(Ordering::SeqCst) {
                                hide_panel(app);
                                visible_for_tray.store(false, Ordering::SeqCst);
                            } else {
                                show_panel(app);
                                visible_for_tray.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::search_items,
            commands::search_items_filtered,
            commands::get_history,
            commands::get_history_filtered,
            commands::count_items,
            commands::get_favorites,
            commands::get_favorites_filtered,
            commands::get_images,
            commands::get_images_filtered,
            commands::get_items_by_type,
            commands::get_items_by_type_filtered,
            commands::toggle_favorite,
            commands::toggle_pin,
            commands::set_snippet_meta,
            commands::get_snippet_groups,
            commands::delete_item,
            commands::delete_all_items,
            commands::update_item,
            commands::copy_item,
            commands::paste_item,
            commands::hide_window,
            commands::start_drag,
            commands::poll_clipboard,
            commands::make_key_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn show_panel<R: tauri::Runtime + 'static>(app: &tauri::AppHandle<R>) {
    position_panel_centered(app);
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel("main") {
            unsafe {
                use tauri_nspanel::objc2::msg_send;
                use tauri_nspanel::objc2::runtime::AnyObject;
                // Activate the app so mouse events are routed to the panel
                let ns_app: *mut AnyObject = msg_send![tauri_nspanel::objc2::class!(NSApplication), sharedApplication];
                let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
                // Show panel and make it key window in one call
                let ns_panel = panel.as_panel();
                let _: () = msg_send![ns_panel, makeKeyAndOrderFront: std::ptr::null::<std::ffi::c_void>()];
            }
            let _ = app.emit("panel-shown", ());
            spawn_focus_watchdog(app.clone());
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(window) = app.get_webview_window("main") {
            window.show().ok();
            window.set_focus().ok();
        }
    }
}

/// 在鼠标所在显示器的中央弹出面板(多显示器时跟随鼠标所在屏幕)。
fn position_panel_centered<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else { return };
    let Ok(size) = window.outer_size() else { return };

    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        window.center().ok();
        return;
    };
    let pos = monitor.position();
    let dim = monitor.size();
    let x = pos.x as f64 + (dim.width as f64 - size.width as f64) / 2.0;
    let y = pos.y as f64 + (dim.height as f64 - size.height as f64) / 2.0;
    window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32)).ok();
}

fn hide_panel<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel("main") {
            panel.hide();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(window) = app.get_webview_window("main") {
            window.hide().ok();
        }
    }
}

/// 面板弹出后短暂守护 key window 状态（全屏 Space 等场景下刚显示时可能丢失 key），
/// 约 3 秒后自动退出，避免常驻线程每 250ms 唤醒一次。
#[cfg(target_os = "macos")]
fn spawn_focus_watchdog<R: tauri::Runtime + 'static>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        let stop = Arc::new(AtomicBool::new(false));
        for _ in 0..12 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let app_for_main = app.clone();
            let stop_for_main = stop.clone();
            let _ = app.run_on_main_thread(move || {
                if let Ok(panel) = app_for_main.get_webview_panel("main") {
                    unsafe {
                        use tauri_nspanel::objc2::msg_send;
                        use tauri_nspanel::objc2::runtime::AnyObject;
                        let ns_panel = panel.as_panel();
                        let is_visible: bool = msg_send![ns_panel, isVisible];
                        if !is_visible {
                            stop_for_main.store(true, Ordering::SeqCst);
                            return;
                        }
                        let is_key: bool = msg_send![ns_panel, isKeyWindow];
                        if is_key {
                            return;
                        }

                        // 面板仍显示但丢失 key 状态，导致 WebView 收不到鼠标事件，重新激活。
                        let ns_app: *mut AnyObject = msg_send![tauri_nspanel::objc2::class!(NSApplication), sharedApplication];
                        let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
                        let _: () = msg_send![ns_panel, makeKeyAndOrderFront: std::ptr::null::<std::ffi::c_void>()];
                    }
                }
            });
        }
    });
}

fn dirs_next() -> Option<String> {
    dirs::data_local_dir().map(|p| p.join("ai-clipboard").to_string_lossy().to_string())
}

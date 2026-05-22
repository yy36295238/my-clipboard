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
                start_panel_focus_watchdog(app.handle().clone(), visible.clone());
            }

            // Register global shortcut (Cmd+Shift+V)
            app.global_shortcut().register(shortcut)?;

            // Create system tray
            use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
            let show = MenuItemBuilder::with_id("show", "显示剪贴板  ⌘⇧V").build(app)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let about = MenuItemBuilder::with_id("about", "关于 AI Clipboard").build(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let quit = MenuItemBuilder::with_id("quit", "退出").accelerator("CmdOrCtrl+Q").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show)
                .item(&sep1)
                .item(&about)
                .item(&sep2)
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
            commands::get_favorites,
            commands::get_favorites_filtered,
            commands::get_images,
            commands::get_images_filtered,
            commands::toggle_favorite,
            commands::toggle_pin,
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

fn show_panel<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    eprintln!("[show] called");
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

fn hide_panel<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    eprintln!("[hide] called");
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

#[cfg(target_os = "macos")]
fn start_panel_focus_watchdog<R: tauri::Runtime + 'static>(
    app: tauri::AppHandle<R>,
    visible: Arc<AtomicBool>,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if !visible.load(Ordering::SeqCst) {
            continue;
        }

        let app_for_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Ok(panel) = app_for_main.get_webview_panel("main") {
                unsafe {
                    use tauri_nspanel::objc2::msg_send;
                    use tauri_nspanel::objc2::runtime::AnyObject;
                    let ns_panel = panel.as_panel();
                    let is_visible: bool = msg_send![ns_panel, isVisible];
                    let is_key: bool = msg_send![ns_panel, isKeyWindow];
                    if !is_visible || is_key {
                        return;
                    }

                    // 全屏 Space 切换后 NSPanel 可能仍显示但丢失 key 状态，导致 WebView 收不到鼠标事件。
                    let ns_app: *mut AnyObject = msg_send![tauri_nspanel::objc2::class!(NSApplication), sharedApplication];
                    let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
                    let _: () = msg_send![ns_panel, makeKeyAndOrderFront: std::ptr::null::<std::ffi::c_void>()];
                }
            }
        });
    });
}

fn dirs_next() -> Option<String> {
    dirs::data_local_dir().map(|p| p.join("ai-clipboard").to_string_lossy().to_string())
}

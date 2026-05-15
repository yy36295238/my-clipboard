mod commands;
mod db;
mod monitor;

use commands::AppState;
use db::Database;
use std::sync::Arc;
use tauri::Manager;
use tauri::tray::TrayIconBuilder;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_dir = dirs_next().unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&app_dir).ok();
    let db_path = format!("{}/clipboard.db", app_dir);
    let db = Arc::new(Database::new(&db_path));

    monitor::start_monitor(db.clone());

    let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                window.hide().ok();
                            } else {
                                show_in_current_space(&window);
                            }
                        }
                    }
                })
                .build(),
        )
        .manage(AppState { db })
        .setup(move |app| {
            // Register global shortcut
            app.global_shortcut().register(shortcut)?;

            // Set app as accessory (menu bar only, no dock icon)
            #[cfg(target_os = "macos")]
            {
                use objc::runtime::{Class, Object};
                use objc::msg_send;
                use objc::sel;
                use objc::sel_impl;
                unsafe {
                    let ns_app: *mut Object = msg_send![Class::get("NSApplication").unwrap(), sharedApplication];
                    let _: () = msg_send![ns_app, setActivationPolicy: 1i64]; // NSApplicationActivationPolicyAccessory = 1
                }
            }

            // Pre-configure window for fullscreen overlay
            if let Some(window) = app.get_webview_window("main") {
                set_panel_level(&window);
            }

            // Create system tray
            let _tray = TrayIconBuilder::new()
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png")).unwrap())
                .tooltip("AI 剪贴板")
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                window.hide().ok();
                            } else {
                                show_in_current_space(&window);
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::search_items,
            commands::get_history,
            commands::get_favorites,
            commands::toggle_favorite,
            commands::toggle_pin,
            commands::delete_item,
            commands::update_item,
            commands::copy_item,
            commands::paste_item,
            commands::hide_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Configure window to appear on all Spaces including fullscreen
fn set_panel_level(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use objc::msg_send;
        use objc::sel;
        use objc::sel_impl;
        use objc::runtime::Object;

        let ns_window: *mut Object = window.ns_window().unwrap() as *mut Object;
        unsafe {
            // CGWindowLevelForKey(kCGScreenSaverWindowLevelKey) = 1000
            let _: () = msg_send![ns_window, setLevel: 1000i64];
            // NSWindowCollectionBehaviorCanJoinAllSpaces = 1 << 0
            // NSWindowCollectionBehaviorStationary = 1 << 4
            // NSWindowCollectionBehaviorFullScreenAuxiliary = 1 << 8
            let behavior: u64 = (1 << 0) | (1 << 4) | (1 << 8);
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Show window in the current Space (including fullscreen) and focus it
fn show_in_current_space(window: &tauri::WebviewWindow) {
    window.show().ok();
    #[cfg(target_os = "macos")]
    {
        use objc::msg_send;
        use objc::sel;
        use objc::sel_impl;
        use objc::runtime::Object;

        let ns_window: *mut Object = window.ns_window().unwrap() as *mut Object;
        unsafe {
            let _: () = msg_send![ns_window, orderFrontRegardless];
        }
    }
    window.set_focus().ok();
}

fn dirs_next() -> Option<String> {
    dirs::data_local_dir().map(|p| p.join("ai-clipboard").to_string_lossy().to_string())
}

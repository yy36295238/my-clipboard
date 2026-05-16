#![allow(unexpected_cfgs)]

mod commands;
mod db;
mod monitor;

use commands::AppState;
use db::Database;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
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

    monitor::start_monitor(db.clone());

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
                    let is_esc = shortcut_event.mods.is_empty() && shortcut_event.key == Code::Escape;
                    let is_toggle = !shortcut_event.mods.is_empty();
                    eprintln!("[shortcut] esc={} toggle={} visible={}", is_esc, is_toggle, visible_for_shortcut.load(Ordering::SeqCst));
                    if is_esc {
                        if visible_for_shortcut.load(Ordering::SeqCst) {
                            hide_panel(app);
                            visible_for_shortcut.store(false, Ordering::SeqCst);
                        }
                    } else if is_toggle {
                        if visible_for_shortcut.load(Ordering::SeqCst) {
                            hide_panel(app);
                            visible_for_shortcut.store(false, Ordering::SeqCst);
                        } else {
                            show_panel(app);
                            visible_for_shortcut.store(true, Ordering::SeqCst);
                        }
                    }
                })
                .build(),
        )
        .manage(AppState { db, visible: visible.clone() })
        .setup(move |app| {
            // Hide dock icon, show only in menu bar
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

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
                panel.set_movable_by_window_background(true);
                unsafe {
                    use tauri_nspanel::objc2::msg_send;
                    let ns_panel = panel.as_panel();
                    let _: () = msg_send![ns_panel, setMovable: true];
                }
            }

            // Register global shortcut (Cmd+Shift+V)
            app.global_shortcut().register(shortcut)?;

            // Register ESC (handled in with_handler, only acts when visible)
            let esc_shortcut = Shortcut::new(None, Code::Escape);
            app.global_shortcut().register(esc_shortcut)?;

            // Create system tray
            use tauri::menu::{MenuBuilder, MenuItemBuilder};
            let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = MenuBuilder::new(app).item(&quit).build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AI 剪贴板")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id().as_ref() == "quit" {
                        app.exit(0);
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
            commands::get_history,
            commands::get_favorites,
            commands::toggle_favorite,
            commands::toggle_pin,
            commands::delete_item,
            commands::update_item,
            commands::copy_item,
            commands::paste_item,
            commands::hide_window,
            commands::start_drag,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn show_panel<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    eprintln!("[show] called");
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel("main") {
            panel.show();
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

fn dirs_next() -> Option<String> {
    dirs::data_local_dir().map(|p| p.join("ai-clipboard").to_string_lossy().to_string())
}

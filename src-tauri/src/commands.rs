use tauri::State;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use crate::db::{ClipboardItem, Database};
use crate::monitor::detect_type;

pub static SKIP_NEXT_CLIPBOARD: AtomicBool = AtomicBool::new(false);

pub struct AppState {
    pub db: Arc<Database>,
    pub visible: Arc<AtomicBool>,
}

#[tauri::command]
pub fn search_items(query: String, offset: usize, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.search(&query, 30, offset)
}

#[tauri::command]
pub fn get_history(offset: usize, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.search("", 30, offset)
}

#[tauri::command]
pub fn get_favorites(state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_favorites(50)
}

#[tauri::command]
pub fn get_images(offset: usize, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_images(30, offset)
}

#[tauri::command]
pub fn toggle_favorite(id: String, state: State<AppState>) -> bool {
    state.db.toggle_favorite(&id)
}

#[tauri::command]
pub fn toggle_pin(id: String, state: State<AppState>) -> bool {
    state.db.toggle_pin(&id)
}

#[tauri::command]
pub fn delete_item(id: String, state: State<AppState>) {
    state.db.delete(&id);
}

#[tauri::command]
pub fn update_item(id: String, content: String, state: State<AppState>) {
    state.db.update_content(&id, &content);
}

/// Silent copy — only sets clipboard, does NOT close window or simulate paste
#[tauri::command]
pub fn copy_item(content: String, content_type: String) {
    SKIP_NEXT_CLIPBOARD.store(true, Ordering::SeqCst);
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if content_type == "image" {
            set_image_clipboard(&mut clipboard, &content);
        } else {
            clipboard.set_text(content).ok();
        }
    }
}

/// Paste — sets clipboard, hides window, simulates Cmd+V into the previously focused app
#[tauri::command]
pub fn paste_item(content: String, content_type: String, window: tauri::WebviewWindow) {
    SKIP_NEXT_CLIPBOARD.store(true, Ordering::SeqCst);
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if content_type == "image" {
            set_image_clipboard(&mut clipboard, &content);
        } else {
            clipboard.set_text(content).ok();
        }
    }
    window.hide().ok();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        simulate_paste();
    });
}

fn set_image_clipboard(clipboard: &mut arboard::Clipboard, path: &str) {
    if let Ok(data) = std::fs::read(path) {
        let decoder = png::Decoder::new(std::io::Cursor::new(&data));
        if let Ok(mut reader) = decoder.read_info() {
            let mut buf = vec![0u8; reader.output_buffer_size()];
            if let Ok(info) = reader.next_frame(&mut buf) {
                buf.truncate(info.buffer_size());
                let img = arboard::ImageData {
                    width: info.width as usize,
                    height: info.height as usize,
                    bytes: std::borrow::Cow::Owned(buf),
                };
                clipboard.set_image(img).ok();
            }
        }
    }
}

#[tauri::command]
pub fn hide_window(app: tauri::AppHandle, state: State<AppState>) {
    state.visible.store(false, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::ManagerExt;
        if let Ok(panel) = app.get_webview_panel("main") {
            panel.hide();
        }
        return;
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window("main") {
            window.hide().ok();
        }
    }
}

#[cfg(target_os = "macos")]
fn simulate_paste() {
    use core_graphics::event::{CGEvent, CGEventFlags, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    if let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
        // 'v' key = keycode 9
        if let Ok(key_down) = CGEvent::new_keyboard_event(source.clone(), 9 as CGKeyCode, true) {
            key_down.set_flags(CGEventFlags::CGEventFlagCommand);
            key_down.post(core_graphics::event::CGEventTapLocation::HID);
        }
        if let Ok(key_up) = CGEvent::new_keyboard_event(source, 9 as CGKeyCode, false) {
            key_up.set_flags(CGEventFlags::CGEventFlagCommand);
            key_up.post(core_graphics::event::CGEventTapLocation::HID);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn simulate_paste() {}

#[tauri::command]
pub fn start_drag(app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::ManagerExt;
        if let Ok(panel) = app.get_webview_panel("main") {
            unsafe {
                use tauri_nspanel::objc2::msg_send;
                use tauri_nspanel::objc2::runtime::AnyObject;
                let ns_panel = panel.as_panel();
                let ns_app: *mut AnyObject = msg_send![tauri_nspanel::objc2::class!(NSApplication), sharedApplication];
                let event: *mut AnyObject = msg_send![ns_app, currentEvent];
                if !event.is_null() {
                    let _: () = msg_send![ns_panel, performWindowDragWithEvent: event];
                }
            }
        }
    }
}

/// Immediately check clipboard and save new content, called when panel is shown
#[tauri::command]
pub fn poll_clipboard(state: State<AppState>) {
    if SKIP_NEXT_CLIPBOARD.load(Ordering::SeqCst) {
        return;
    }
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                if state.db.has_content(&text) {
                    state.db.touch(&text);
                } else {
                    let item = ClipboardItem {
                        id: uuid::Uuid::new_v4().to_string(),
                        content: text.clone(),
                        content_type: detect_type(&text).to_string(),
                        created_at: chrono::Utc::now().timestamp(),
                        favorite: false,
                        pinned: false,
                    };
                    state.db.insert(&item);
                }
            }
        }
    }
}

#[tauri::command]
pub fn make_key_window(app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::ManagerExt;
        if let Ok(panel) = app.get_webview_panel("main") {
            unsafe {
                use tauri_nspanel::objc2::msg_send;
                let ns_panel = panel.as_panel();
                let _: () = msg_send![ns_panel, makeKeyWindow];
            }
        }
    }
}

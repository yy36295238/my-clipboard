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

/// 按关键词和日期范围查询剪贴板历史，日期范围由前端按本地日期换算成时间戳。
#[tauri::command]
pub fn search_items_filtered(query: String, offset: usize, start_at: Option<i64>, end_at: Option<i64>, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.search_filtered(&query, 30, offset, start_at, end_at)
}

#[tauri::command]
pub fn get_history(offset: usize, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.search("", 30, offset)
}

/// 按日期范围查询全部历史记录。
#[tauri::command]
pub fn get_history_filtered(offset: usize, start_at: Option<i64>, end_at: Option<i64>, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.search_filtered("", 30, offset, start_at, end_at)
}

/// 统计当前筛选条件下的剪贴板记录总数，用于前端展示已加载数量和总数。
#[tauri::command]
pub fn count_items(query: String, content_type: Option<String>, favorites_only: bool, start_at: Option<i64>, end_at: Option<i64>, state: State<AppState>) -> usize {
    state.db.count_items(&query, content_type.as_deref(), favorites_only, start_at, end_at)
}

#[tauri::command]
pub fn get_favorites(offset: usize, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_favorites_filtered(30, offset, None, None)
}

/// 按日期范围查询收藏记录。
#[tauri::command]
pub fn get_favorites_filtered(offset: usize, start_at: Option<i64>, end_at: Option<i64>, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_favorites_filtered(30, offset, start_at, end_at)
}

#[tauri::command]
pub fn get_images(offset: usize, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_images(30, offset)
}

/// 按日期范围查询图片记录。
#[tauri::command]
pub fn get_images_filtered(offset: usize, start_at: Option<i64>, end_at: Option<i64>, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_images_filtered(30, offset, start_at, end_at)
}

/// 按内容类型查询剪贴板记录，用于前端展示 text、image、json 等全部支持类型。
#[tauri::command]
pub fn get_items_by_type(content_type: String, offset: usize, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_by_type_filtered(&content_type, 30, offset, None, None)
}

/// 按内容类型和日期范围查询剪贴板记录。
#[tauri::command]
pub fn get_items_by_type_filtered(content_type: String, offset: usize, start_at: Option<i64>, end_at: Option<i64>, state: State<AppState>) -> Vec<ClipboardItem> {
    state.db.get_by_type_filtered(&content_type, 30, offset, start_at, end_at)
}

#[tauri::command]
pub fn toggle_favorite(id: String, state: State<AppState>) -> bool {
    state.db.toggle_favorite(&id)
}

#[tauri::command]
pub fn toggle_pin(id: String, state: State<AppState>) -> bool {
    state.db.toggle_pin(&id)
}

/// 设置片段的名称和分组(收藏后的记录即片段)。
#[tauri::command]
pub fn set_snippet_meta(id: String, name: String, group_name: String, state: State<AppState>) {
    state.db.set_snippet_meta(&id, name.trim(), group_name.trim());
}

/// 已有片段分组名,供前端分组输入框自动补全。
#[tauri::command]
pub fn get_snippet_groups(state: State<AppState>) -> Vec<String> {
    state.db.snippet_groups()
}

#[tauri::command]
pub fn delete_item(id: String, state: State<AppState>) {
    let item = state.db.get_content_and_type(&id);
    state.db.delete(&id);
    // 图片记录删除后同步清掉原图和缩略图,避免磁盘文件泄漏
    if let Some((content, content_type)) = item {
        if content_type == "image" {
            let path = std::path::PathBuf::from(&content);
            std::fs::remove_file(&path).ok();
            if let Some(thumb) = crate::monitor::thumb_path(&path) {
                std::fs::remove_file(thumb).ok();
            }
        }
    }
}

/// 删除未收藏的剪贴板记录，收藏夹内容由数据库层保留，前端负责二次确认后再调用。
#[tauri::command]
pub fn delete_all_items(state: State<AppState>) -> usize {
    let count = state.db.delete_all();
    crate::monitor::sweep_orphan_images(&state.db);
    count
}

/// 更新内容并重新识别类型,返回新类型给前端;图片记录的类型保持不变。
#[tauri::command]
pub fn update_item(id: String, content: String, state: State<AppState>) -> String {
    let content_type = match state.db.get_content_and_type(&id) {
        Some((_, t)) if t == "image" => t,
        _ => detect_type(&content).to_string(),
    };
    state.db.update_content(&id, &content, &content_type);
    content_type
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

/// 粘贴历史记录：先写入系统剪贴板，再隐藏面板并把 Cmd+V 发送回原先应用。
#[tauri::command]
pub fn paste_item(content: String, content_type: String, window: tauri::WebviewWindow, state: State<AppState>) {
    SKIP_NEXT_CLIPBOARD.store(true, Ordering::SeqCst);
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if content_type == "image" {
            set_image_clipboard(&mut clipboard, &content);
        } else {
            clipboard.set_text(content).ok();
        }
    }
    state.visible.store(false, Ordering::SeqCst);
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
    if crate::monitor::clipboard_is_concealed() {
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
                        name: String::new(),
                        group_name: String::new(),
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
                use tauri_nspanel::objc2::runtime::AnyObject;
                // 先激活 app 再设为 key window,与 show_panel 保持一致:
                // app 处于后台(Accessory)时仅 makeKeyWindow 不足以让 WebView 收到鼠标/键盘事件,需一并激活。
                let ns_app: *mut AnyObject = msg_send![tauri_nspanel::objc2::class!(NSApplication), sharedApplication];
                let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
                let ns_panel = panel.as_panel();
                let _: () = msg_send![ns_panel, makeKeyWindow];
            }
        }
    }
}

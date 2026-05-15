use arboard::Clipboard;
use chrono::Utc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::path::PathBuf;
use std::fs;
use uuid::Uuid;

use crate::commands::SKIP_NEXT_CLIPBOARD;
use crate::db::{ClipboardItem, Database};

fn detect_type(content: &str) -> &'static str {
    let trimmed = content.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            return "json";
        }
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return "url";
    }
    if trimmed.starts_with('#') || trimmed.contains("\n## ") || trimmed.contains("\n- ") {
        return "markdown";
    }
    if trimmed.contains("function ") || trimmed.contains("fn ") || trimmed.contains("def ")
        || trimmed.contains("class ") || trimmed.contains("import ")
        || trimmed.contains("SELECT ") || trimmed.contains("const ") || trimmed.contains("let ")
    {
        return "code";
    }
    "text"
}

pub fn start_monitor(db: Arc<Database>) {
    let images_dir = PathBuf::from(
        dirs::data_local_dir()
            .map(|p| p.join("ai-clipboard").join("images"))
            .unwrap_or_else(|| PathBuf::from("./images"))
    );
    fs::create_dir_all(&images_dir).ok();

    thread::spawn(move || {
        let mut clipboard = Clipboard::new().expect("Failed to access clipboard");
        let mut last_content = String::new();
        let mut last_image_hash: u64 = 0;

        loop {
            thread::sleep(Duration::from_millis(500));

            if SKIP_NEXT_CLIPBOARD.swap(false, Ordering::SeqCst) {
                if let Ok(current) = clipboard.get_text() {
                    last_content = current;
                }
                continue;
            }

            // Check for image first
            if let Ok(img) = clipboard.get_image() {
                let hash = simple_hash(&img.bytes);
                if hash != last_image_hash && img.bytes.len() > 0 {
                    last_image_hash = hash;
                    let id = Uuid::new_v4().to_string();
                    let filename = format!("{}.png", &id);
                    let filepath = images_dir.join(&filename);

                    // Save as PNG
                    if save_rgba_as_png(&img.bytes, img.width, img.height, &filepath) {
                        let item = ClipboardItem {
                            id,
                            content: filepath.to_string_lossy().to_string(),
                            content_type: "image".to_string(),
                            created_at: Utc::now().timestamp(),
                            favorite: false,
                            pinned: false,
                        };
                        db.insert(&item);
                        db.cleanup(500);
                    }
                }
                continue;
            }

            // Check for text
            let current = clipboard.get_text().unwrap_or_default();
            if !current.is_empty() && current != last_content {
                last_content = current.clone();
                if db.has_content(&current) {
                    continue;
                }
                let item = ClipboardItem {
                    id: Uuid::new_v4().to_string(),
                    content: current.clone(),
                    content_type: detect_type(&current).to_string(),
                    created_at: Utc::now().timestamp(),
                    favorite: false,
                    pinned: false,
                };
                db.insert(&item);
                db.cleanup(500);
            }
        }
    });
}

fn simple_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data.iter().step_by(64) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn save_rgba_as_png(rgba: &[u8], width: usize, height: usize, path: &PathBuf) -> bool {
    use std::io::BufWriter;
    let file = match fs::File::create(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = match encoder.write_header() {
        Ok(w) => w,
        Err(_) => return false,
    };
    writer.write_image_data(rgba).is_ok()
}

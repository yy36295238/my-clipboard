use arboard::Clipboard;
use chrono::Utc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::path::PathBuf;
use std::fs;
use tauri::Emitter;
use uuid::Uuid;

use crate::commands::SKIP_NEXT_CLIPBOARD;
use crate::db::{ClipboardItem, Database};

pub fn detect_type(content: &str) -> &'static str {
    let trimmed = content.trim();

    // JSON: must parse as valid JSON
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            return "json";
        }
    }

    // URL: single-line URL (no newlines)
    if (trimmed.starts_with("http://") || trimmed.starts_with("https://")) && !trimmed.contains('\n') {
        return "url";
    }

    // Email: single line, contains @ and domain with dot
    if is_email(trimmed) {
        return "email";
    }

    // Phone: Chinese mobile number pattern
    if is_phone(trimmed) {
        return "phone";
    }

    // Score-based code detection
    let code_score = compute_code_score(trimmed);
    if code_score >= 2 {
        return "code";
    }

    // Markdown
    if is_markdown(trimmed) {
        return "markdown";
    }

    "text"
}

fn is_email(content: &str) -> bool {
    if content.contains('\n') { return false; }
    let parts: Vec<&str> = content.split('@').collect();
    if parts.len() != 2 { return false; }
    let local = parts[0];
    let domain = parts[1];
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain.split('.').all(|s| !s.is_empty())
}

fn is_phone(content: &str) -> bool {
    if content.contains('\n') { return false; }
    let digits: String = content.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.len() == 11 && digits.starts_with('1')
}

fn compute_code_score(content: &str) -> u8 {
    let mut score: u8 = 0;
    let has_newline = content.contains('\n');

    // Strong indicators: almost always code (2 points each)
    let strong_keywords: &[&str] = &[
        "fn ", "pub fn ", "async fn ",
        "def ", "async def ",
        "func ",
        "impl ", "trait ", "pub struct ",
        "#include", "#define", "#ifdef", "#ifndef",
        "package main", "package ",
        "module ", "module.exports",
        "-> ", "=> ",  // return type / arrow syntax (Rust, Haskell, JS)
        "::",  // namespace separator (Rust, C++)
    ];
    for kw in strong_keywords {
        if content.contains(kw) {
            score += 2;
            if score >= 4 { return score; }
        }
    }

    // Medium indicators: likely code but need reinforcement (1 point each)
    let medium_keywords: &[&str] = &[
        "function ", "function*",
        "const ", "let ", "var ",
        "class ", "interface ", "enum ",
        "import ", "from 'import' ", "export ",
        "return ", "return\n",
        "SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE TABLE", "ALTER TABLE",
        "if (", "if(", "else if", "else {",
        "for (", "for(", "while (", "while(",
        "switch (", "case ",
        "try {", "catch (", "finally {",
        ".then(", ".catch(", ".map(", ".filter(", ".reduce(",
        "console.log", "print(", "println!",
        "sudo ", "chmod ", "mkdir ", "grep ", "awk ", "sed ",
        "docker ", "kubectl ",
        "git clone", "git push", "git pull",
        // Java / C# / Kotlin access modifiers & common types
        "private ", "protected ", "public ",
        "@Override", "@Autowired", "@Inject",
        "void ", "String ", "int ", "boolean ", "long ",
        "final ", "static ", "synchronized ",
        "extends ", "implements ", "throws ",
        "@Getter", "@Setter", "@Data",
    ];
    for kw in medium_keywords {
        if content.contains(kw) {
            score += 1;
            if score >= 4 { return score; }
        }
    }

    // Structural patterns: code-like syntax (1 point each, require newline context)
    if has_newline {
        // Lines ending with ; or { or } in multi-line content
        let lines: Vec<&str> = content.lines().take(20).collect();
        let mut semi_count = 0;
        let mut brace_count = 0;
        for line in lines.iter() {
            let l = line.trim_end();
            if l.ends_with(';') { semi_count += 1; }
            if l.ends_with('{') || l.ends_with('}') { brace_count += 1; }
        }
        if semi_count >= 2 { score += 1; }
        if brace_count >= 2 { score += 1; }

        // Shebang line
        if content.starts_with("#!") {
            score += 2;
        }

        // HTML/XML tags
        if content.contains("</") && content.contains(">") {
            score += 1;
        }

        // CSS-like property patterns
        if content.contains(": ") && content.contains(";") && content.contains("{") {
            // Check for CSS property patterns like "color: red;"
            let css_properties = ["color", "background", "margin", "padding", "font-size", "display", "width", "height", "border"];
            let mut css_hits = 0;
            for prop in css_properties.iter() {
                if content.contains(prop) { css_hits += 1; }
            }
            if css_hits >= 2 { score += 1; }
        }

        // Indentation patterns: consistent leading spaces/tabs suggest code
        let indented = lines.iter().filter(|l| l.starts_with("    ") || l.starts_with('\t')).count();
        if indented >= 3 {
            score += 1;
        }
    }

    score
}

fn is_markdown(content: &str) -> bool {
    // Check for markdown heading patterns
    if content.starts_with("# ") || content.starts_with("## ") || content.starts_with("### ") {
        return true;
    }
    if content.contains("\n# ") || content.contains("\n## ") || content.contains("\n### ") {
        return true;
    }
    // Bullet lists, numbered lists, or blockquotes
    if content.contains("\n- ") || content.contains("\n* ") || content.contains("\n> ") || content.contains("\n1. ") {
        return true;
    }
    // Links or bold/italic syntax
    if content.contains("](") && content.contains('[') {
        return true;
    }
    // Code blocks
    if content.contains("```") {
        return true;
    }
    false
}

pub fn start_monitor(db: Arc<Database>, app: tauri::AppHandle) {
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
        let mut insert_count: u32 = 0;

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
                        insert_count += 1;
                        if insert_count % 50 == 0 { db.cleanup(500); }
                        let _ = app.emit("clipboard-updated", ());
                    }
                }
                continue;
            }

            // Check for text
            let current = clipboard.get_text().unwrap_or_default();
            if !current.is_empty() && current != last_content {
                last_content = current.clone();
                if db.has_content(&current) {
                    db.touch(&current);
                    let _ = app.emit("clipboard-updated", ());
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
                insert_count += 1;
                if insert_count % 50 == 0 { db.cleanup(500); }
                let _ = app.emit("clipboard-updated", ());
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

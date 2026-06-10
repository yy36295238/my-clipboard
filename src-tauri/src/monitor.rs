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

    // JSON: 支持严格 JSON，也兼容带 // 字段说明的配置片段，避免被代码评分误判成 code。
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
            || serde_json::from_str::<serde_json::Value>(&strip_json_line_comments(trimmed)).is_ok()
        {
            return "json";
        }
    }

    // URL: 整段就是一个链接,不允许任何空白(否则"链接 + 说明文字"会被误判)
    if (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && !trimmed.chars().any(|c| c.is_whitespace())
    {
        return "url";
    }

    // Email: 整段就是一个邮箱地址
    if is_email(trimmed) {
        return "email";
    }

    // Phone: 整段就是一个中国大陆手机号
    if is_phone(trimmed) {
        return "phone";
    }

    // Markdown 信号足够强时优先于 code:
    // 带 ``` 围栏的文档里代码块关键词会把 code 分凑满,必须先判。
    let md_score = markdown_score(trimmed);
    if trimmed.contains("```") || md_score >= 2 {
        return "markdown";
    }

    // Score-based code detection
    let code_score = compute_code_score(trimmed);
    if code_score >= 2 {
        return "code";
    }

    if md_score >= 1 {
        return "markdown";
    }

    "text"
}

fn is_email(content: &str) -> bool {
    if content.chars().any(|c| c.is_whitespace()) { return false; }
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

/// 整段内容(可带 +86 前缀和 -/空格 分隔符)恰好是一个 1[3-9] 开头的 11 位手机号。
fn is_phone(content: &str) -> bool {
    let c = content.strip_prefix("+86").map(str::trim_start).unwrap_or(content);
    let mut digits = String::new();
    for ch in c.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if ch != '-' && ch != ' ' {
            return false;
        }
    }
    digits.len() == 11
        && digits.starts_with('1')
        && matches!(digits.as_bytes()[1], b'3'..=b'9')
}

fn strip_json_line_comments(content: &str) -> String {
    content
        .lines()
        .map(strip_json_line_comment)
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_json_line_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    let mut prev_slash: Option<usize> = None;

    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            prev_slash = None;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            prev_slash = None;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            prev_slash = None;
            continue;
        }
        if ch == '/' && !in_string {
            if let Some(start) = prev_slash {
                return &line[..start];
            }
            prev_slash = Some(idx);
        } else {
            prev_slash = None;
        }
    }
    line
}

fn compute_code_score(content: &str) -> u8 {
    let mut score: u8 = 0;

    // Strong indicators: almost always code (2 points each)
    let strong_keywords: &[&str] = &[
        "fn ", "pub fn ", "async fn ",
        "def ", "async def ",
        "func ",
        "impl ", "trait ", "pub struct ",
        "#include", "#define", "#ifdef", "#ifndef",
        "package main", "module.exports",
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
        "import ", "export ",
        "SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE TABLE", "ALTER TABLE",
        "if (", "if(", "else if", "else {",
        "for (", "for(", "while (", "while(",
        "switch (",
        "try {", "catch (", "finally {",
        ".then(", ".catch(", ".map(", ".filter(", ".reduce(",
        "console.log", "print(", "println!",
        "sudo ", "chmod ", "mkdir ", "grep ", "awk ", "sed ",
        "docker ", "kubectl ",
        "git clone", "git push", "git pull",
        "@Override", "@Autowired", "@Inject",
        "@Getter", "@Setter", "@Data",
        "-> ", "=> ",  // 箭头在普通文本里也常见,只给 1 分
        "::",
    ];
    for kw in medium_keywords {
        if content.contains(kw) {
            score += 1;
            if score >= 4 { return score; }
        }
    }

    // Structural patterns
    let lines: Vec<&str> = content.lines().take(20).collect();
    let mut semi_count = 0;
    let mut brace_count = 0;
    for line in lines.iter() {
        let l = line.trim_end();
        if l.ends_with(';') { semi_count += 1; }
        if l.ends_with('{') || l.ends_with('}') { brace_count += 1; }
    }
    let mut structure_score: u8 = 0;
    if semi_count >= 2 { structure_score += 1; }
    if brace_count >= 2 { structure_score += 1; }

    // Shebang line
    if content.starts_with("#!") {
        structure_score += 2;
    }

    // HTML/XML tags
    if content.contains("</") && content.contains('>') {
        structure_score += 1;
    }

    // CSS-like property patterns
    if content.contains(": ") && content.contains(';') && content.contains('{') {
        let css_properties = ["color", "background", "margin", "padding", "font-size", "display", "width", "height", "border"];
        let css_hits = css_properties.iter().filter(|p| content.contains(**p)).count();
        if css_hits >= 2 { structure_score += 1; }
    }

    // Indentation patterns: consistent leading spaces/tabs suggest code
    let indented = lines.iter().filter(|l| l.starts_with("    ") || l.starts_with('\t')).count();
    if indented >= 3 {
        structure_score += 1;
    }
    score += structure_score;
    if score >= 4 { return score; }

    // 英文普通文本里也常见的词(return/case/public/long...)很容易凑分,
    // 只有出现行尾分号/花括号等代码结构证据时才计入,关键词分不算结构证据。
    let has_structure = structure_score > 0 || semi_count >= 1 || brace_count >= 1;
    if has_structure {
        let prose_prone_keywords: &[&str] = &[
            "return ", "case ",
            "private ", "protected ", "public ",
            "void ", "String ", "int ", "boolean ", "long ",
            "final ", "static ", "synchronized ",
            "extends ", "implements ", "throws ",
            "package ", "module ",
        ];
        for kw in prose_prone_keywords {
            if content.contains(kw) {
                score += 1;
                if score >= 4 { return score; }
            }
        }
    }

    score
}

/// Markdown 信号计数:标题、列表/引用(含首行)、链接,各 1 分。
fn markdown_score(content: &str) -> u8 {
    let mut score: u8 = 0;
    if content.starts_with("# ") || content.starts_with("## ") || content.starts_with("### ")
        || content.contains("\n# ") || content.contains("\n## ") || content.contains("\n### ")
    {
        score += 1;
    }
    if content.starts_with("- ") || content.starts_with("* ") || content.starts_with("> ") || content.starts_with("1. ")
        || content.contains("\n- ") || content.contains("\n* ") || content.contains("\n> ") || content.contains("\n1. ")
    {
        score += 1;
    }
    if content.contains("](") && content.contains('[') {
        score += 1;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::detect_type;

    #[test]
    fn detects_json_with_line_comments() {
        let content = r#"{
    "groupPriceList" : {
               "currencyType": 1, // 是否是人民币 0：人民币 1：外币，(选填)
                "currencyCode": "CNY", // 币种，(必填)
                "currencyDesc": "人民币元" // 币种描述，(选填)
     },
    "otherCurrencyType": 1, // 是否是人民币 0：人民币 1：外币，(选填)
    "otherCurrencyCode": "CNY", // 币种，(必填)
    "otherCurrencyDesc": "人民币元" // 币种描述，(选填)
}"#;

        assert_eq!(detect_type(content), "json");
    }

    #[test]
    fn keeps_double_slash_inside_json_string() {
        let content = r#"{
  "url": "https://example.com/a//b", // 链接说明
  "name": "demo"
}"#;

        assert_eq!(detect_type(content), "json");
    }

    #[test]
    fn detects_plain_url() {
        assert_eq!(detect_type("https://example.com/a?b=1&c=2"), "url");
    }

    #[test]
    fn url_with_trailing_text_is_not_url() {
        assert_eq!(detect_type("https://example.com 这个链接打不开了"), "text");
    }

    #[test]
    fn detects_plain_email() {
        assert_eq!(detect_type("admin@example.com"), "email");
    }

    #[test]
    fn sentence_with_email_is_not_email() {
        assert_eq!(detect_type("请联系 admin@example.com"), "text");
    }

    #[test]
    fn detects_phone_with_separators() {
        assert_eq!(detect_type("13812345678"), "phone");
        assert_eq!(detect_type("138-1234-5678"), "phone");
        assert_eq!(detect_type("+86 138 1234 5678"), "phone");
    }

    #[test]
    fn sentence_with_phone_is_not_phone() {
        assert_eq!(detect_type("订单号 13812345678 已发货"), "text");
    }

    #[test]
    fn eleven_digits_with_invalid_segment_is_not_phone() {
        assert_eq!(detect_type("12345678901"), "text");
    }

    #[test]
    fn english_prose_is_not_code() {
        assert_eq!(detect_type("I will return the case to the public tomorrow"), "text");
    }

    #[test]
    fn arrow_in_plain_text_is_not_code() {
        assert_eq!(detect_type("进度 100% -> 完成"), "text");
    }

    #[test]
    fn java_signature_is_code() {
        assert_eq!(detect_type("public static void main(String[] args) {"), "code");
    }

    #[test]
    fn js_snippet_is_code() {
        assert_eq!(detect_type("const x = 1;\nconst y = 2;"), "code");
    }

    #[test]
    fn rust_single_line_is_code() {
        assert_eq!(detect_type("let v = Vec::new();"), "code");
    }

    #[test]
    fn markdown_with_code_block_is_markdown() {
        assert_eq!(detect_type("# 使用说明\n\n```js\nconst a = 1;\n```"), "markdown");
    }

    #[test]
    fn markdown_heading_and_list_beats_code() {
        assert_eq!(detect_type("# 待办\n\n- 修复 const 报错\n- 更新 import 路径"), "markdown");
    }

    #[test]
    fn first_line_bullet_list_is_markdown() {
        assert_eq!(detect_type("- 买牛奶\n- 倒垃圾"), "markdown");
    }

    #[test]
    fn python_script_with_comment_header_is_code() {
        assert_eq!(detect_type("# coding: utf-8\nimport os\nprint(os.path)"), "code");
    }
}

pub fn images_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|p| p.join("ai-clipboard").join("images"))
        .unwrap_or_else(|| PathBuf::from("./images"))
}

/// 缩略图路径:与原图同目录,文件名加 thumb_ 前缀。
pub fn thumb_path(original: &std::path::Path) -> Option<PathBuf> {
    let dir = original.parent()?;
    let name = original.file_name()?.to_str()?;
    Some(dir.join(format!("thumb_{}", name)))
}

/// 删除磁盘上没有任何记录引用的图片及其缩略图,防止删除记录后文件越积越多。
pub fn sweep_orphan_images(db: &Database) {
    let dir = images_dir();
    let referenced: std::collections::HashSet<String> = db.image_paths().into_iter().collect();
    let Ok(entries) = fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let original = name.strip_prefix("thumb_").unwrap_or(name);
        let original_path = dir.join(original).to_string_lossy().to_string();
        if !referenced.contains(&original_path) {
            fs::remove_file(&path).ok();
        }
    }
}

/// 密码管理器等会用 ConcealedType/TransientType 标记敏感内容,跳过不记录。
#[cfg(target_os = "macos")]
pub fn clipboard_is_concealed() -> bool {
    use tauri_nspanel::objc2::msg_send;
    use tauri_nspanel::objc2::runtime::AnyObject;
    unsafe {
        let pb: *mut AnyObject = msg_send![tauri_nspanel::objc2::class!(NSPasteboard), generalPasteboard];
        if pb.is_null() { return false; }
        let types: *mut AnyObject = msg_send![pb, types];
        if types.is_null() { return false; }
        let count: usize = msg_send![types, count];
        for i in 0..count {
            let t: *mut AnyObject = msg_send![types, objectAtIndex: i];
            if t.is_null() { continue; }
            let utf8: *const std::os::raw::c_char = msg_send![t, UTF8String];
            if utf8.is_null() { continue; }
            let s = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
            if s == "org.nspasteboard.ConcealedType" || s == "org.nspasteboard.TransientType" {
                return true;
            }
        }
        false
    }
}

#[cfg(not(target_os = "macos"))]
pub fn clipboard_is_concealed() -> bool { false }

pub fn start_monitor(db: Arc<Database>, app: tauri::AppHandle) {
    let images_dir = images_dir();
    fs::create_dir_all(&images_dir).ok();

    thread::spawn(move || {
        // 一次性迁移:给老的图片记录补内容哈希,然后清掉重复图片
        for (id, path) in db.images_missing_hash() {
            if let Some(hash) = hash_png_file(&path) {
                db.set_content_hash(&id, &hash);
            }
        }
        if db.dedupe_images() > 0 {
            let _ = app.emit("clipboard-updated", ());
        }
        sweep_orphan_images(&db);

        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                log::error!("无法访问剪贴板,监控线程退出: {e}");
                return;
            }
        };
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

            if clipboard_is_concealed() {
                continue;
            }

            // Check for image first
            if let Ok(img) = clipboard.get_image() {
                let hash = simple_hash(&img.bytes);
                if hash != last_image_hash && img.bytes.len() > 0 {
                    last_image_hash = hash;

                    // 数据库级去重:同一张图重复复制只刷新时间戳,不再落新文件
                    let full_hash = image_hash(&img.bytes, img.width, img.height);
                    if let Some(existing_id) = db.find_image_id_by_hash(&full_hash) {
                        db.touch_id(&existing_id);
                        let _ = app.emit("clipboard-updated", ());
                        continue;
                    }

                    let id = Uuid::new_v4().to_string();
                    let filename = format!("{}.png", &id);
                    let filepath = images_dir.join(&filename);

                    // Save as PNG
                    if save_rgba_as_png(&img.bytes, img.width, img.height, &filepath) {
                        if let Some((thumb, tw, th)) = make_thumbnail(&img.bytes, img.width, img.height, 400) {
                            save_rgba_as_png(&thumb, tw, th, &images_dir.join(format!("thumb_{}", &filename)));
                        }
                        let item = ClipboardItem {
                            id,
                            content: filepath.to_string_lossy().to_string(),
                            content_type: "image".to_string(),
                            created_at: Utc::now().timestamp(),
                            favorite: false,
                            pinned: false,
                            name: String::new(),
                            group_name: String::new(),
                        };
                        db.insert_with_hash(&item, &full_hash);
                        insert_count += 1;
                        if insert_count % 50 == 0 { db.cleanup(500); sweep_orphan_images(&db); }
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
                    name: String::new(),
                    group_name: String::new(),
                };
                db.insert(&item);
                insert_count += 1;
                if insert_count % 50 == 0 { db.cleanup(500); sweep_orphan_images(&db); }
                let _ = app.emit("clipboard-updated", ());
            }
        }
    });
}

/// 最近邻降采样生成列表缩略图;宽度不超过 max_w 时返回 None,直接复用原图。
fn make_thumbnail(rgba: &[u8], width: usize, height: usize, max_w: usize) -> Option<(Vec<u8>, usize, usize)> {
    if width <= max_w || width == 0 || height == 0 || rgba.len() < width * height * 4 {
        return None;
    }
    let tw = max_w;
    let th = ((height as f64 * max_w as f64 / width as f64).round() as usize).max(1);
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        let sy = y * height / th;
        for x in 0..tw {
            let sx = x * width / tw;
            let si = (sy * width + sx) * 4;
            let di = (y * tw + x) * 4;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    Some((out, tw, th))
}

fn simple_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data.iter().step_by(64) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 完整内容哈希(全量 FNV + 尺寸),用于数据库级去重;
/// 只在检测到新图片事件时计算一次,不在每次轮询里跑。
fn image_hash(rgba: &[u8], width: usize, height: usize) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in rgba {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{}x{}:{:016x}", width, height, h)
}

/// 解码已存的 PNG 并计算内容哈希,用于老数据补哈希。
fn hash_png_file(path: &str) -> Option<String> {
    let data = fs::read(path).ok()?;
    let decoder = png::Decoder::new(std::io::Cursor::new(&data));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some(image_hash(&buf, info.width as usize, info.height as usize))
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

use rusqlite::{Connection, ToSql, params};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    pub favorite: bool,
    pub pinned: bool,
    /// 片段名称,收藏后可自定义,空串表示未命名
    #[serde(default)]
    pub name: String,
    /// 片段分组,空串表示未分组
    #[serde(rename = "groupName", default)]
    pub group_name: String,
}

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    pub fn new(path: &str) -> Self {
        let conn = Connection::open(path).expect("Failed to open database");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clipboard_items (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                content_type TEXT NOT NULL DEFAULT 'text',
                created_at INTEGER NOT NULL,
                favorite INTEGER NOT NULL DEFAULT 0,
                pinned INTEGER NOT NULL DEFAULT 0
            );
            DROP TRIGGER IF EXISTS clipboard_ai;
            DROP TRIGGER IF EXISTS clipboard_ad;
            DROP TRIGGER IF EXISTS clipboard_au;
            DROP TABLE IF EXISTS clipboard_fts;"
        ).expect("Failed to create tables");
        // 增量迁移:老库没有这些列时补上,已存在时报错忽略
        conn.execute("ALTER TABLE clipboard_items ADD COLUMN name TEXT NOT NULL DEFAULT ''", []).ok();
        conn.execute("ALTER TABLE clipboard_items ADD COLUMN group_name TEXT NOT NULL DEFAULT ''", []).ok();
        conn.execute("ALTER TABLE clipboard_items ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''", []).ok();
        Database { conn: Mutex::new(conn) }
    }

    pub fn insert(&self, item: &ClipboardItem) {
        self.insert_with_hash(item, "");
    }

    /// 插入记录并携带内容哈希(目前仅图片使用,用于跨时间去重)。
    pub fn insert_with_hash(&self, item: &ClipboardItem, content_hash: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO clipboard_items (id, content, content_type, created_at, favorite, pinned, name, group_name, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![item.id, item.content, item.content_type, item.created_at, item.favorite as i32, item.pinned as i32, item.name, item.group_name, content_hash],
        ).ok();
    }

    /// 按内容哈希查找已存在的图片记录。
    pub fn find_image_id_by_hash(&self, hash: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM clipboard_items WHERE content_type = 'image' AND content_hash = ?1 LIMIT 1",
            params![hash],
            |row| row.get(0),
        ).ok()
    }

    /// 刷新指定记录的时间戳,使其排到列表最前。
    pub fn touch_id(&self, id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET created_at = ?2 WHERE id = ?1",
            params![id, chrono::Utc::now().timestamp()],
        ).ok();
    }

    /// 还没有内容哈希的图片记录(老数据),用于启动时补算。
    pub fn images_missing_hash(&self) -> Vec<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, content FROM clipboard_items WHERE content_type = 'image' AND content_hash = ''"
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    pub fn set_content_hash(&self, id: &str, hash: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET content_hash = ?2 WHERE id = ?1",
            params![id, hash],
        ).ok();
    }

    /// 清理重复图片:同一哈希只保留最新一条,收藏/置顶的不删。返回删除条数。
    pub fn dedupe_images(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM clipboard_items
             WHERE content_type = 'image' AND favorite = 0 AND pinned = 0 AND content_hash != ''
               AND id NOT IN (
                 SELECT id FROM (
                   SELECT id, MAX(created_at) FROM clipboard_items
                   WHERE content_type = 'image' AND content_hash != ''
                   GROUP BY content_hash
                 )
               )",
            [],
        ).unwrap_or(0)
    }

    pub fn has_content(&self, content: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM clipboard_items WHERE content = ?1",
            params![content],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0) > 0
    }

    pub fn touch(&self, content: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET created_at = ?2 WHERE content = ?1",
            params![content, chrono::Utc::now().timestamp()],
        ).ok();
    }

    pub fn search(&self, query: &str, limit: usize, offset: usize) -> Vec<ClipboardItem> {
        self.search_filtered(query, limit, offset, None, None)
    }

    /// 按关键词和本地日期范围查询历史记录，时间范围使用 Unix 秒级时间戳的左闭右开区间。
    pub fn search_filtered(&self, query: &str, limit: usize, offset: usize, start_at: Option<i64>, end_at: Option<i64>) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        if query.is_empty() && start_at.is_none() && end_at.is_none() {
            return self.recent(&conn, limit, offset);
        }
        self.query_items(&conn, query, None, false, query.is_empty(), limit, offset, start_at, end_at)
    }

    /// 统计当前筛选条件下的记录总数，用于前端展示“已加载/总数”。
    pub fn count_items(&self, query: &str, content_type: Option<&str>, favorites_only: bool, start_at: Option<i64>, end_at: Option<i64>) -> usize {
        let conn = self.conn.lock().unwrap();
        let mut filters = Vec::new();
        let mut values: Vec<Box<dyn ToSql>> = Vec::new();

        Self::append_filters(&mut filters, &mut values, query, content_type, favorites_only, start_at, end_at);

        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        };
        let sql = format!("SELECT COUNT(*) FROM clipboard_items{}", where_clause);
        let params = values.iter().map(|value| value.as_ref() as &dyn ToSql);
        conn.query_row(&sql, rusqlite::params_from_iter(params), |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    fn query_items(
        &self,
        conn: &Connection,
        query: &str,
        content_type: Option<&str>,
        favorites_only: bool,
        pinned_first: bool,
        limit: usize,
        offset: usize,
        start_at: Option<i64>,
        end_at: Option<i64>,
    ) -> Vec<ClipboardItem> {
        let mut filters = Vec::new();
        let mut values: Vec<Box<dyn ToSql>> = Vec::new();

        Self::append_filters(&mut filters, &mut values, query, content_type, favorites_only, start_at, end_at);

        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        };
        let order_by = if favorites_only {
            // 片段按分组聚合展示,未分组(空串)排最前,组内仍置顶优先
            "group_name ASC, pinned DESC, created_at DESC"
        } else if pinned_first {
            "pinned DESC, created_at DESC"
        } else {
            "created_at DESC"
        };
        let sql = format!(
            "SELECT id, content, content_type, created_at, favorite, pinned, name, group_name
             FROM clipboard_items{} ORDER BY {} LIMIT ? OFFSET ?",
            where_clause, order_by
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;
        values.push(Box::new(limit_i64));
        values.push(Box::new(offset_i64));
        let params = values.iter().map(|value| value.as_ref() as &dyn ToSql);
        stmt.query_map(rusqlite::params_from_iter(params), |row| Self::row_to_item(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    fn append_filters(
        filters: &mut Vec<String>,
        values: &mut Vec<Box<dyn ToSql>>,
        query: &str,
        content_type: Option<&str>,
        favorites_only: bool,
        start_at: Option<i64>,
        end_at: Option<i64>,
    ) {
        if favorites_only {
            filters.push("favorite = 1".to_string());
        }
        if let Some(content_type) = content_type {
            filters.push("content_type = ?".to_string());
            values.push(Box::new(content_type.to_string()));
        }
        for term in query.split_whitespace() {
            let escaped = term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            filters.push("content LIKE ? ESCAPE '\\'".to_string());
            values.push(Box::new(format!("%{}%", escaped)));
        }
        if let Some(start_at) = start_at {
            filters.push("created_at >= ?".to_string());
            values.push(Box::new(start_at));
        }
        if let Some(end_at) = end_at {
            filters.push("created_at < ?".to_string());
            values.push(Box::new(end_at));
        }
    }

    fn recent(&self, conn: &Connection, limit: usize, offset: usize) -> Vec<ClipboardItem> {
        let mut stmt = conn.prepare(
            "SELECT id, content, content_type, created_at, favorite, pinned, name, group_name
             FROM clipboard_items ORDER BY pinned DESC, created_at DESC LIMIT ?1 OFFSET ?2"
        ).unwrap();
        stmt.query_map(params![limit as i64, offset as i64], |row| Self::row_to_item(row)).unwrap().filter_map(|r| r.ok()).collect()
    }

    /// 查询收藏记录，可叠加日期范围过滤。
    pub fn get_favorites_filtered(&self, limit: usize, offset: usize, start_at: Option<i64>, end_at: Option<i64>) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        self.query_items(&conn, "", None, true, false, limit, offset, start_at, end_at)
    }

    pub fn get_images(&self, limit: usize, offset: usize) -> Vec<ClipboardItem> {
        self.get_images_filtered(limit, offset, None, None)
    }

    /// 按内容类型查询记录，可叠加日期范围过滤。
    pub fn get_by_type_filtered(&self, content_type: &str, limit: usize, offset: usize, start_at: Option<i64>, end_at: Option<i64>) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        self.query_items(&conn, "", Some(content_type), false, false, limit, offset, start_at, end_at)
    }

    /// 查询图片记录，可叠加日期范围过滤。
    pub fn get_images_filtered(&self, limit: usize, offset: usize, start_at: Option<i64>, end_at: Option<i64>) -> Vec<ClipboardItem> {
        self.get_by_type_filtered("image", limit, offset, start_at, end_at)
    }

    pub fn toggle_favorite(&self, id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET favorite = CASE WHEN favorite = 0 THEN 1 ELSE 0 END WHERE id = ?1",
            params![id],
        ).unwrap();
        let val: i32 = conn.query_row("SELECT favorite FROM clipboard_items WHERE id = ?1", params![id], |r| r.get(0)).unwrap_or(0);
        val != 0
    }

    pub fn toggle_pin(&self, id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET pinned = CASE WHEN pinned = 0 THEN 1 ELSE 0 END WHERE id = ?1",
            params![id],
        ).unwrap();
        let val: i32 = conn.query_row("SELECT pinned FROM clipboard_items WHERE id = ?1", params![id], |r| r.get(0)).unwrap_or(0);
        val != 0
    }

    pub fn delete(&self, id: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_items WHERE id = ?1", params![id]).ok();
    }

    /// 清空非收藏的剪贴板记录，收藏夹内容需要保留，避免误删长期保存的数据。
    pub fn delete_all(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_items WHERE favorite = 0 AND pinned = 0", []).unwrap_or(0)
    }

    /// Remove old items beyond the limit, keeping favorites and pinned
    pub fn cleanup(&self, max_items: usize) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM clipboard_items WHERE id IN (
                SELECT id FROM clipboard_items
                WHERE favorite = 0 AND pinned = 0
                ORDER BY created_at DESC
                LIMIT -1 OFFSET ?1
            )",
            params![max_items as i64],
        ).ok();
    }

    pub fn update_content(&self, id: &str, content: &str, content_type: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET content = ?2, content_type = ?3 WHERE id = ?1",
            params![id, content, content_type],
        ).ok();
    }

    pub fn get_content_and_type(&self, id: &str) -> Option<(String, String)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT content, content_type FROM clipboard_items WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok()
    }

    /// 所有图片记录的文件路径,用于孤儿文件清扫。
    pub fn image_paths(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare("SELECT content FROM clipboard_items WHERE content_type = 'image'") {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// 更新片段的名称和分组。
    pub fn set_snippet_meta(&self, id: &str, name: &str, group_name: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET name = ?2, group_name = ?3 WHERE id = ?1",
            params![id, name, group_name],
        ).ok();
    }

    /// 已有的片段分组名,用于分组输入框的自动补全。
    pub fn snippet_groups(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT group_name FROM clipboard_items WHERE favorite = 1 AND group_name != '' ORDER BY group_name"
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| row.get::<_, String>(0))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipboardItem> {
        Ok(ClipboardItem {
            id: row.get(0)?,
            content: row.get(1)?,
            content_type: row.get(2)?,
            created_at: row.get(3)?,
            favorite: row.get::<_, i32>(4)? != 0,
            pinned: row.get::<_, i32>(5)? != 0,
            name: row.get(6)?,
            group_name: row.get(7)?,
        })
    }
}

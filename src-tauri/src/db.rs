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
            CREATE VIRTUAL TABLE IF NOT EXISTS clipboard_fts USING fts5(
                content, content='clipboard_items', content_rowid='rowid'
            );
            CREATE TRIGGER IF NOT EXISTS clipboard_ai AFTER INSERT ON clipboard_items BEGIN
                INSERT INTO clipboard_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS clipboard_ad AFTER DELETE ON clipboard_items BEGIN
                INSERT INTO clipboard_fts(clipboard_fts, rowid, content) VALUES('delete', old.rowid, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS clipboard_au AFTER UPDATE ON clipboard_items BEGIN
                INSERT INTO clipboard_fts(clipboard_fts, rowid, content) VALUES('delete', old.rowid, old.content);
                INSERT INTO clipboard_fts(rowid, content) VALUES (new.rowid, new.content);
            END;"
        ).expect("Failed to create tables");
        Database { conn: Mutex::new(conn) }
    }

    pub fn insert(&self, item: &ClipboardItem) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO clipboard_items (id, content, content_type, created_at, favorite, pinned) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![item.id, item.content, item.content_type, item.created_at, item.favorite as i32, item.pinned as i32],
        ).ok();
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

        if favorites_only {
            filters.push("favorite = 1".to_string());
        }
        if let Some(content_type) = content_type {
            filters.push("content_type = ?".to_string());
            values.push(Box::new(content_type.to_string()));
        }
        for term in query.split_whitespace() {
            filters.push("content LIKE ?".to_string());
            values.push(Box::new(format!("%{}%", term)));
        }
        if let Some(start_at) = start_at {
            filters.push("created_at >= ?".to_string());
            values.push(Box::new(start_at));
        }
        if let Some(end_at) = end_at {
            filters.push("created_at < ?".to_string());
            values.push(Box::new(end_at));
        }

        let where_clause = if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        };
        let order_by = if pinned_first {
            "pinned DESC, created_at DESC"
        } else {
            "created_at DESC"
        };
        let sql = format!(
            "SELECT id, content, content_type, created_at, favorite, pinned
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

    fn recent(&self, conn: &Connection, limit: usize, offset: usize) -> Vec<ClipboardItem> {
        let mut stmt = conn.prepare(
            "SELECT id, content, content_type, created_at, favorite, pinned
             FROM clipboard_items ORDER BY pinned DESC, created_at DESC LIMIT ?1 OFFSET ?2"
        ).unwrap();
        stmt.query_map(params![limit as i64, offset as i64], |row| Self::row_to_item(row)).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn get_favorites(&self, limit: usize) -> Vec<ClipboardItem> {
        self.get_favorites_filtered(limit, None, None)
    }

    /// 查询收藏记录，可叠加日期范围过滤。
    pub fn get_favorites_filtered(&self, limit: usize, start_at: Option<i64>, end_at: Option<i64>) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        self.query_items(&conn, "", None, true, false, limit, 0, start_at, end_at)
    }

    pub fn get_images(&self, limit: usize, offset: usize) -> Vec<ClipboardItem> {
        self.get_images_filtered(limit, offset, None, None)
    }

    /// 查询图片记录，可叠加日期范围过滤。
    pub fn get_images_filtered(&self, limit: usize, offset: usize, start_at: Option<i64>, end_at: Option<i64>) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        self.query_items(&conn, "", Some("image"), false, false, limit, offset, start_at, end_at)
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

    /// 清空全部剪贴板记录，包含收藏和置顶记录。
    pub fn delete_all(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_items", []).unwrap_or(0)
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

    pub fn update_content(&self, id: &str, content: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET content = ?2 WHERE id = ?1",
            params![id, content],
        ).ok();
    }

    fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipboardItem> {
        Ok(ClipboardItem {
            id: row.get(0)?,
            content: row.get(1)?,
            content_type: row.get(2)?,
            created_at: row.get(3)?,
            favorite: row.get::<_, i32>(4)? != 0,
            pinned: row.get::<_, i32>(5)? != 0,
        })
    }
}

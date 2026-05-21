use rusqlite::{Connection, params};
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
        let conn = self.conn.lock().unwrap();
        if query.is_empty() {
            return self.recent(&conn, limit, offset);
        }
        let terms: Vec<String> = query.split_whitespace()
            .map(|t| format!("%{}%", t))
            .collect();
        let where_clause = terms.iter().enumerate()
            .map(|(i, _)| format!("content LIKE ?{}", i + 1))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "SELECT id, content, content_type, created_at, favorite, pinned
             FROM clipboard_items WHERE {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
            where_clause, terms.len() + 1, terms.len() + 2
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            terms.iter().map(|s| s as &dyn rusqlite::ToSql)
                .chain(std::iter::once(&limit_i64 as &dyn rusqlite::ToSql))
                .chain(std::iter::once(&offset_i64 as &dyn rusqlite::ToSql))
        )).unwrap();
        let mut result = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            result.push(ClipboardItem {
                id: row.get(0).unwrap(),
                content: row.get(1).unwrap(),
                content_type: row.get(2).unwrap(),
                created_at: row.get(3).unwrap(),
                favorite: row.get::<_, i32>(4).unwrap() != 0,
                pinned: row.get::<_, i32>(5).unwrap() != 0,
            });
        }
        result
    }

    fn recent(&self, conn: &Connection, limit: usize, offset: usize) -> Vec<ClipboardItem> {
        let mut stmt = conn.prepare(
            "SELECT id, content, content_type, created_at, favorite, pinned
             FROM clipboard_items ORDER BY pinned DESC, created_at DESC LIMIT ?1 OFFSET ?2"
        ).unwrap();
        stmt.query_map(params![limit as i64, offset as i64], |row| {
            Ok(ClipboardItem {
                id: row.get(0)?,
                content: row.get(1)?,
                content_type: row.get(2)?,
                created_at: row.get(3)?,
                favorite: row.get::<_, i32>(4)? != 0,
                pinned: row.get::<_, i32>(5)? != 0,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn get_favorites(&self, limit: usize) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, content, content_type, created_at, favorite, pinned
             FROM clipboard_items WHERE favorite = 1 ORDER BY created_at DESC LIMIT ?1"
        ).unwrap();
        stmt.query_map(params![limit as i64], |row| {
            Ok(ClipboardItem {
                id: row.get(0)?,
                content: row.get(1)?,
                content_type: row.get(2)?,
                created_at: row.get(3)?,
                favorite: row.get::<_, i32>(4)? != 0,
                pinned: row.get::<_, i32>(5)? != 0,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn get_images(&self, limit: usize, offset: usize) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, content, content_type, created_at, favorite, pinned
             FROM clipboard_items WHERE content_type = 'image' ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"
        ).unwrap();
        stmt.query_map(params![limit as i64, offset as i64], |row| {
            Ok(ClipboardItem {
                id: row.get(0)?,
                content: row.get(1)?,
                content_type: row.get(2)?,
                created_at: row.get(3)?,
                favorite: row.get::<_, i32>(4)? != 0,
                pinned: row.get::<_, i32>(5)? != 0,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
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
}

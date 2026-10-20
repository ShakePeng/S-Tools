use std::{path::Path, sync::Mutex};

use rusqlite::{params, Connection};

use crate::models::{AllHistory, HistoryItem};

pub struct Storage {
    connection: Mutex<Connection>,
}

impl Storage {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|error| format!("创建数据目录失败：{error}"))?;
        let connection = Connection::open(data_dir.join("s-tools.db"))
            .map_err(|error| format!("打开历史数据库失败：{error}"))?;
        connection
            .execute_batch(
                r#"
                PRAGMA journal_mode = WAL;
                CREATE TABLE IF NOT EXISTS translation_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    created_at INTEGER NOT NULL,
                    source_text TEXT NOT NULL,
                    translated_text TEXT NOT NULL,
                    source_lang TEXT NOT NULL,
                    target_lang TEXT NOT NULL,
                    provider_id TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS screenshot_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    created_at INTEGER NOT NULL,
                    path TEXT NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_translation_created
                    ON translation_history(created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_screenshot_created
                    ON screenshot_history(created_at DESC);
                "#,
            )
            .map_err(|error| format!("初始化历史数据库失败：{error}"))?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn add_translation(
        &self,
        source_text: &str,
        translated_text: &str,
        source_lang: &str,
        target_lang: &str,
        provider_id: &str,
        limit: usize,
    ) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "历史数据库锁已损坏".to_string())?;
        let now = chrono::Utc::now().timestamp();
        connection
            .execute(
                "INSERT INTO translation_history
                 (created_at, source_text, translated_text, source_lang, target_lang, provider_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    now,
                    source_text,
                    translated_text,
                    source_lang,
                    target_lang,
                    provider_id
                ],
            )
            .map_err(|error| format!("写入翻译历史失败：{error}"))?;
        trim_history(&connection, "translation_history", limit)?;
        Ok(())
    }

    pub fn add_screenshot(
        &self,
        path: &Path,
        width: u32,
        height: u32,
        limit: usize,
    ) -> Result<i64, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "历史数据库锁已损坏".to_string())?;
        let now = chrono::Utc::now().timestamp();
        connection
            .execute(
                "INSERT INTO screenshot_history (created_at, path, width, height)
                 VALUES (?1, ?2, ?3, ?4)",
                params![now, path.to_string_lossy(), width, height],
            )
            .map_err(|error| format!("写入截图历史失败：{error}"))?;
        let id = connection.last_insert_rowid();
        trim_history(&connection, "screenshot_history", limit)?;
        Ok(id)
    }

    pub fn all_history(&self) -> Result<AllHistory, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "历史数据库锁已损坏".to_string())?;
        let translations = {
            let mut statement = connection
                .prepare(
                    "SELECT id, created_at, source_text, translated_text, provider_id
                     FROM translation_history ORDER BY created_at DESC LIMIT 500",
                )
                .map_err(|error| format!("读取翻译历史失败：{error}"))?;
            let rows = statement
                .query_map([], |row| {
                    Ok(HistoryItem {
                        id: row.get(0)?,
                        kind: "translation".into(),
                        created_at: row.get(1)?,
                        title: row.get(2)?,
                        subtitle: row.get(3)?,
                        path: None,
                        provider_id: Some(row.get(4)?),
                    })
                })
                .map_err(|error| format!("读取翻译历史失败：{error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("读取翻译历史失败：{error}"))?;
            rows
        };
        let screenshots = {
            let mut statement = connection
                .prepare(
                    "SELECT id, created_at, path, width, height
                     FROM screenshot_history ORDER BY created_at DESC LIMIT 500",
                )
                .map_err(|error| format!("读取截图历史失败：{error}"))?;
            let rows = statement
                .query_map([], |row| {
                    let path: String = row.get(2)?;
                    let width: i64 = row.get(3)?;
                    let height: i64 = row.get(4)?;
                    Ok(HistoryItem {
                        id: row.get(0)?,
                        kind: "screenshot".into(),
                        created_at: row.get(1)?,
                        title: Path::new(&path)
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| "截图".into()),
                        subtitle: format!("{width} × {height}"),
                        path: Some(path),
                        provider_id: None,
                    })
                })
                .map_err(|error| format!("读取截图历史失败：{error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("读取截图历史失败：{error}"))?;
            rows
        };
        Ok(AllHistory {
            translations,
            screenshots,
        })
    }

    pub fn delete(&self, kind: &str, id: i64) -> Result<Option<String>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "历史数据库锁已损坏".to_string())?;
        match kind {
            "translation" => {
                connection
                    .execute("DELETE FROM translation_history WHERE id = ?1", params![id])
                    .map_err(|error| format!("删除翻译历史失败：{error}"))?;
                Ok(None)
            }
            "screenshot" => {
                let path: Option<String> = connection
                    .query_row(
                        "SELECT path FROM screenshot_history WHERE id = ?1",
                        params![id],
                        |row| row.get(0),
                    )
                    .ok();
                connection
                    .execute("DELETE FROM screenshot_history WHERE id = ?1", params![id])
                    .map_err(|error| format!("删除截图历史失败：{error}"))?;
                Ok(path)
            }
            _ => Err("未知历史类型".into()),
        }
    }
}

fn trim_history(connection: &Connection, table: &str, limit: usize) -> Result<(), String> {
    let keep = limit.clamp(20, 1000) as i64;
    connection
        .execute(
            &format!(
                "DELETE FROM {table} WHERE id NOT IN
                 (SELECT id FROM {table} ORDER BY created_at DESC LIMIT ?1)"
            ),
            params![keep],
        )
        .map_err(|error| format!("清理历史记录失败：{error}"))?;
    Ok(())
}

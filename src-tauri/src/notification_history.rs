use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

const RETENTION_DAYS: i64 = 30;
const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationHistoryRecord {
    pub id: String,
    pub title: String,
    pub body: String,
    pub category: String,
    pub created_at: i64,
    pub read: bool,
    pub url: Option<String>,
}

#[derive(Clone)]
pub struct NotificationHistoryStore {
    db_path: PathBuf,
}

impl NotificationHistoryStore {
    pub fn new() -> Result<Self> {
        Self::open_at(crate::providers::config_dir()?.join("notification-history.db"))
    }

    pub fn open_at(db_path: PathBuf) -> Result<Self> {
        let store = Self { db_path };
        store.initialize()?;
        store.prune()?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection> {
        Connection::open(&self.db_path).context("打开通知历史数据库失败")
    }

    fn initialize(&self) -> Result<()> {
        self.connect()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS notification_history (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                category TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                is_read INTEGER NOT NULL DEFAULT 0,
                url TEXT
            );
            CREATE INDEX IF NOT EXISTS notification_history_created_at
            ON notification_history(created_at DESC);",
        )?;
        Ok(())
    }

    fn cutoff(now_ms: i64) -> i64 {
        now_ms - RETENTION_DAYS * MILLIS_PER_DAY
    }

    pub fn prune(&self) -> Result<usize> {
        let cutoff = Self::cutoff(Utc::now().timestamp_millis());
        Ok(self.connect()?.execute(
            "DELETE FROM notification_history WHERE created_at < ?1 OR category = 'terminal'",
            params![cutoff],
        )?)
    }

    pub fn list(&self) -> Result<Vec<NotificationHistoryRecord>> {
        self.prune()?;
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id,title,body,category,created_at,is_read,url
             FROM notification_history ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(NotificationHistoryRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                category: row.get(3)?,
                created_at: row.get(4)?,
                read: row.get::<_, i64>(5)? != 0,
                url: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn append(&self, record: &NotificationHistoryRecord) -> Result<()> {
        self.prune()?;
        if record.created_at < Self::cutoff(Utc::now().timestamp_millis()) {
            return Ok(());
        }
        self.connect()?.execute(
            "INSERT INTO notification_history(id,title,body,category,created_at,is_read,url)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(id) DO NOTHING",
            params![
                record.id,
                record.title,
                record.body,
                record.category,
                record.created_at,
                i64::from(record.read),
                record.url,
            ],
        )?;
        Ok(())
    }

    pub fn import(&self, records: &[NotificationHistoryRecord]) -> Result<()> {
        for record in records {
            self.append(record)?;
        }
        Ok(())
    }

    pub fn mark_all_read(&self) -> Result<()> {
        self.prune()?;
        self.connect()?
            .execute("UPDATE notification_history SET is_read=1", [])?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.connect()?
            .execute("DELETE FROM notification_history", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn record(id: &str, created_at: i64) -> NotificationHistoryRecord {
        NotificationHistoryRecord {
            id: id.into(),
            title: "通知".into(),
            body: "完整内容".into(),
            category: "general".into(),
            created_at,
            read: false,
            url: None,
        }
    }

    #[test]
    fn history_survives_reopen_and_expires_after_thirty_days() {
        let path = std::env::temp_dir().join(format!("crewvela-notification-{}.db", Uuid::new_v4()));
        let now = Utc::now().timestamp_millis();
        let store = NotificationHistoryStore::open_at(path.clone()).unwrap();
        store
            .append(&record("recent", now - 29 * MILLIS_PER_DAY))
            .unwrap();
        store
            .append(&record("expired", now - 31 * MILLIS_PER_DAY))
            .unwrap();
        drop(store);

        let reopened = NotificationHistoryStore::open_at(path.clone()).unwrap();
        let records = reopened.list().unwrap();
        assert_eq!(
            records
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["recent"]
        );
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn read_state_and_clear_are_persisted() {
        let path = std::env::temp_dir().join(format!("crewvela-notification-{}.db", Uuid::new_v4()));
        let store = NotificationHistoryStore::open_at(path.clone()).unwrap();
        store
            .append(&record("one", Utc::now().timestamp_millis()))
            .unwrap();
        store.mark_all_read().unwrap();
        assert!(store.list().unwrap()[0].read);
        store.clear().unwrap();
        assert!(store.list().unwrap().is_empty());
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn terminal_bells_are_not_kept_in_notification_history() {
        let path = std::env::temp_dir().join(format!("crewvela-notification-{}.db", Uuid::new_v4()));
        let store = NotificationHistoryStore::open_at(path.clone()).unwrap();
        let mut bell = record("bell", Utc::now().timestamp_millis());
        bell.category = "terminal".into();
        store.append(&bell).unwrap();

        assert!(store.list().unwrap().is_empty());

        drop(store);
        let _ = std::fs::remove_file(path);
    }
}

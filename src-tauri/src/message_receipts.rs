use crate::control::OutboxMessage;
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Receipt {
    id: String,
    sender: String,
    target: String,
    body: String,
    queued_at: i64,
    submitted_at: Option<i64>,
    received_at: Option<i64>,
    reply_at: Option<i64>,
    reply: Option<String>,
    pub(crate) stage: String,
}

fn file(outbox: &Path) -> PathBuf { outbox.join("receipts.sqlite3") }

fn open(outbox: &Path, create: bool) -> Result<Option<Connection>> {
    if !create && !file(outbox).is_file() { return Ok(None); }
    std::fs::create_dir_all(outbox)?;
    let db = Connection::open(file(outbox))?;
    db.busy_timeout(Duration::from_secs(3))?;
    if create {
        db.execute_batch("CREATE TABLE IF NOT EXISTS receipts (
            id TEXT PRIMARY KEY, sender TEXT NOT NULL, request TEXT NOT NULL, target TEXT NOT NULL,
            body TEXT NOT NULL, reply_to TEXT, queued_at INTEGER NOT NULL,
            submitted_at INTEGER, received_at INTEGER, stage TEXT NOT NULL, UNIQUE(sender,request)
        )")?;
    }
    Ok(Some(db))
}

fn now() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64 }

pub(crate) fn publish(outbox: &Path, message: &OutboxMessage, request: Option<&str>, reply_to: Option<&str>) -> Result<String> {
    let sender = &message.from.as_ref().context("持久回执需要绑定发送会话")?.session_id;
    let request = request.map(|value| uuid::Uuid::parse_str(value).map(|id| id.to_string()))
        .transpose().context("requestId 必须为 UUID")?.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut db = open(outbox, true)?.context("无法打开回执存储")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some((id, target, body, parent)) = tx.query_row(
        "SELECT id,target,body,reply_to FROM receipts WHERE sender=?1 AND request=?2",
        params![sender, request],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)),
    ).optional()? {
        ensure!(target == message.to && body == message.body && parent.as_deref() == reply_to, "不能复用请求 ID 修改消息");
        return Ok(id);
    }
    if let Some(parent) = reply_to {
        let related: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM receipts WHERE id=?1 AND target=?2 AND sender=?3)",
            params![parent, sender, message.to], |row| row.get(0),
        )?;
        ensure!(related, "回复只能关联当前会话收到的原消息");
    }
    let name = crate::messaging::new_outbox_name();
    let id = crate::control::outbox_message_id(Path::new(&name))?;
    tx.execute("INSERT INTO receipts(id,sender,request,target,body,reply_to,queued_at,stage) VALUES (?1,?2,?3,?4,?5,?6,?7,'publishing')",
        params![id,sender,request,message.to,message.body,reply_to,now()])?;
    // Reserve across processes before publishing. An interrupted reservation is
    // visible as uncertain; repeating its request never publishes again.
    tx.commit()?;
    crate::messaging::write_named_outbox(outbox, &name, message)?;
    advance(outbox, &id, "queued")?;
    Ok(id)
}

pub(crate) fn advance(outbox: &Path, id: &str, stage: &str) -> Result<bool> {
    if !id.starts_with("outbox-") { return Ok(false); }
    let Some(db) = open(outbox, false)? else { return Ok(false); };
    let predecessor = match stage {
        "queued" => "('publishing')",
        "attempting" => "('queued','publishing')",
        "submitted" => "('queued','publishing','attempting')",
        "received" => "('queued','publishing','attempting','submitted')",
        "rejected" => "('queued','publishing')",
        _ => return Ok(false),
    };
    Ok(db.execute(&format!("UPDATE receipts SET stage=?2,
        submitted_at=CASE WHEN ?2='submitted' THEN ?3 ELSE submitted_at END,
        received_at=CASE WHEN ?2='received' THEN ?3 ELSE received_at END
        WHERE id=?1 AND stage IN {predecessor}"), params![id,stage,now()])? > 0)
}

pub(crate) fn unsure_after_restart(outbox: &Path, id: &str) -> Result<bool> {
    if !id.starts_with("outbox-") { return Ok(false); }
    let Some(db) = open(outbox, false)? else { return Ok(false); };
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM receipts WHERE id=?1 AND stage IN ('attempting','submitted','received'))",
        [id], |row| row.get(0))?)
}

fn select(outbox: &Path, session: &str, by_id: Option<&str>) -> Result<Vec<Receipt>> {
    let Some(db) = open(outbox, false)? else { return Ok(Vec::new()); };
    let mut statement = db.prepare(
        "SELECT r.id,r.sender,r.target,r.body,r.queued_at,r.submitted_at,r.received_at,r.stage,
         q.body, COALESCE(q.received_at,q.submitted_at,q.queued_at), q.stage
         FROM receipts r LEFT JOIN receipts q ON q.id = (
           SELECT id FROM receipts WHERE reply_to=r.id AND stage IN ('queued','attempting','submitted','received')
           ORDER BY queued_at DESC,id DESC LIMIT 1
         )
         WHERE (r.sender=?1 OR r.target=?1) AND (?2 IS NULL OR r.id=?2)
         ORDER BY r.queued_at DESC,r.id DESC LIMIT 30",
    )?;
    let rows = statement.query_map(params![session,by_id], |row| {
        let stage: String = row.get(7)?;
        let reply_stage: Option<String> = row.get(10)?;
        Ok(Receipt {
            id: row.get(0)?, sender: row.get(1)?, target: row.get(2)?, body: row.get(3)?,
            queued_at: row.get(4)?, submitted_at: row.get(5)?, received_at: row.get(6)?,
            reply_at: row.get(9)?, reply: row.get(8)?,
            stage: if reply_stage.as_deref() == Some("received") { "replied".into() }
                else if reply_stage.is_some() { "reply_recorded".into() }
                else { stage },
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn lookup(outbox: &Path, session: &str, id: &str) -> Result<Receipt> {
    select(outbox, session, Some(id))?.into_iter().next().context("此会话没有对应的消息回执")
}

#[tauri::command]
pub(crate) fn list_session_message_receipts(session_id: String) -> Result<Vec<Receipt>, String> {
    uuid::Uuid::parse_str(&session_id).map_err(|error| error.to_string())?;
    select(&crate::providers::config_dir().map_err(|error| error.to_string())?.join("outbox"), &session_id, None)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::MessageOrigin;

    #[test]
    fn receipt_workflow_survives_restart_and_correlates_only_verified_reply() {
        let root = std::env::temp_dir().join(format!("crewvela-receipt-{}", uuid::Uuid::new_v4()));
        let a = uuid::Uuid::new_v4().to_string();
        let b = uuid::Uuid::new_v4().to_string();
        let request = uuid::Uuid::new_v4().to_string();
        let message = OutboxMessage {
            to: b.clone(), body: "check zero".into(),
            from: Some(MessageOrigin { session_id: a.clone(), label: "A".into() }),
        };
        let first = publish(&root, &message, Some(&request), None).unwrap();
        assert_eq!(lookup(&root, &a, &first).unwrap().stage, "queued");
        assert_eq!(publish(&root, &message, Some(&request), None).unwrap(), first);
        assert_eq!(std::fs::read_dir(&root).unwrap().flatten().filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json")).count(), 1);
        assert!(lookup(&root, "unrelated", &first).is_err());
        let mut changed = message.clone();
        changed.body = "different".into();
        assert!(publish(&root, &changed, Some(&request), None).is_err());
        assert!(advance(&root, &first, "attempting").unwrap());
        assert!(unsure_after_restart(&root, &first).unwrap());
        assert!(advance(&root, &first, "submitted").unwrap());
        assert_eq!(lookup(&root, &a, &first).unwrap().received_at, None);
        let reply = OutboxMessage {
            to: a.clone(), body: "zero works".into(),
            from: Some(MessageOrigin { session_id: b.clone(), label: "B".into() }),
        };
        assert!(publish(&root, &message, None, Some(&first)).is_err());
        let second = publish(&root, &reply, None, Some(&first)).unwrap();
        assert_eq!(lookup(&root, &a, &first).unwrap().stage, "reply_recorded");
        assert!(advance(&root, &second, "received").unwrap());
        let restored = lookup(&root, &a, &first).unwrap();
        assert_eq!(restored.stage, "replied");
        assert_eq!(restored.reply.as_deref(), Some("zero works"));
        let _ = std::fs::remove_dir_all(root);
    }
}

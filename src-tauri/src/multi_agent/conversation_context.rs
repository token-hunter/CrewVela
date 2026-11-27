use super::{decode, public_reply_text, ConversationChatDispatch, ConversationMessage, ConversationRound};
use anyhow::Result;
use rusqlite::{params, Connection};
use std::borrow::Cow;

const HISTORY_CHARS: usize = 16_000;
const MESSAGE_CHARS: usize = 2_500;
const QUOTE_CHARS: usize = 6_000;
const HISTORY_ROWS: usize = 48;

pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS ma_conversation_publications (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            message_id TEXT NOT NULL UNIQUE REFERENCES ma_conversation_messages(id) ON DELETE CASCADE,
            project_id TEXT NOT NULL,
            topic_id TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS ma_conversation_publications_topic
            ON ma_conversation_publications(project_id, topic_id, seq);
         CREATE TABLE IF NOT EXISTS ma_conversation_context_cursors (
            task_id TEXT PRIMARY KEY REFERENCES ma_tasks(id) ON DELETE CASCADE,
            publication_seq INTEGER NOT NULL
         );
         INSERT OR IGNORE INTO ma_conversation_publications(message_id, project_id, topic_id)
            SELECT id, project_id, json_extract(data_json,'$.conversationId')
            FROM ma_conversation_messages
            WHERE json_extract(data_json,'$.senderType') IN ('user','agent')
              AND json_extract(data_json,'$.status')='completed'
              AND json_extract(data_json,'$.kind')<>'silent'
              AND json_extract(data_json,'$.conversationId') IS NOT NULL
              AND id NOT IN (SELECT message_id FROM ma_conversation_publications)
            ORDER BY created_at,rowid;
         CREATE TRIGGER IF NOT EXISTS ma_conversation_publish_message
         AFTER INSERT ON ma_conversation_messages
         WHEN json_extract(NEW.data_json,'$.senderType') IN ('user','agent')
          AND json_extract(NEW.data_json,'$.status')='completed'
          AND json_extract(NEW.data_json,'$.kind')<>'silent'
         BEGIN
           INSERT INTO ma_conversation_publications(message_id, project_id, topic_id)
            VALUES(NEW.id, NEW.project_id, json_extract(NEW.data_json,'$.conversationId'));
         END;
         CREATE TRIGGER IF NOT EXISTS ma_conversation_republish_message
         AFTER UPDATE OF data_json ON ma_conversation_messages
         WHEN json_extract(NEW.data_json,'$.senderType') IN ('user','agent')
          AND json_extract(NEW.data_json,'$.status')='completed'
          AND json_extract(NEW.data_json,'$.kind')<>'silent'
          AND (
            json_extract(OLD.data_json,'$.status') IS NOT json_extract(NEW.data_json,'$.status')
            OR json_extract(OLD.data_json,'$.body') IS NOT json_extract(NEW.data_json,'$.body')
            OR json_extract(OLD.data_json,'$.kind') IS NOT json_extract(NEW.data_json,'$.kind')
          )
         BEGIN
           DELETE FROM ma_conversation_publications WHERE message_id=NEW.id;
           INSERT INTO ma_conversation_publications(message_id, project_id, topic_id)
            VALUES(NEW.id, NEW.project_id, json_extract(NEW.data_json,'$.conversationId'));
         END;",
    )?;
    Ok(())
}

pub(super) fn cursor(db: &Connection, project: &str, topic: &str) -> Result<i64> {
    Ok(db.query_row(
        "SELECT COALESCE(MAX(seq),0) FROM ma_conversation_publications WHERE project_id=?1 AND topic_id=?2",
        params![project, topic],
        |row| row.get(0),
    )?)
}

pub(super) fn record_cursor(db: &Connection, task: &str, cursor: i64) -> Result<()> {
    db.execute(
        "INSERT INTO ma_conversation_context_cursors(task_id,publication_seq) VALUES(?1,?2)
         ON CONFLICT(task_id) DO UPDATE SET publication_seq=excluded.publication_seq",
        params![task, cursor],
    )?;
    Ok(())
}

pub(super) fn quote(body: &str) -> String {
    excerpt(body.trim(), QUOTE_CHARS)
}

fn excerpt(text: &str, limit: usize) -> String {
    let mut chars = text.char_indices();
    let Some((prefix, _)) = chars.nth(limit / 2) else { return text.to_owned() };
    if chars.count() < limit - limit / 2 {
        return text.to_owned();
    }
    let suffix = text.char_indices().rev().nth(limit / 2 - 1).map(|(index, _)| index).unwrap_or(prefix);
    format!("{}\n[中间省略，见原消息]\n{}", &text[..prefix], &text[suffix..])
}

pub(super) fn history(
    db: &Connection,
    round: &ConversationRound,
    reply_to: Option<&str>,
    seen: Option<(i64, &str)>,
) -> Result<Vec<String>> {
    let (since, own_reply) = seen.unwrap_or((0, ""));
    let independent = matches!(round.chat_dispatch, Some(ConversationChatDispatch::Parallel));
    let mut query = db.prepare(
        "SELECT m.data_json FROM ma_conversation_publications p
         JOIN ma_conversation_messages m ON m.id=p.message_id
         WHERE p.project_id=?1 AND p.topic_id=?2 AND p.seq>?3
           AND json_extract(m.data_json,'$.status')='completed'
           AND json_extract(m.data_json,'$.kind')<>'silent'
           AND m.id NOT IN (?4,?5,?6)
           AND (?7=0 OR json_extract(m.data_json,'$.senderType')='user'
                OR json_extract(m.data_json,'$.roundId') IS NOT ?8)
         ORDER BY p.seq DESC LIMIT ?9",
    )?;
    let rows = query.query_map(
        params![round.project_id, round.conversation_id, since.max(0), round.user_message_id,
            reply_to.unwrap_or(""), own_reply, independent, round.id, (HISTORY_ROWS + 1) as i64],
        |row| row.get::<_, String>(0),
    )?;
    let mut selected = Vec::new();
    let mut references = Vec::new();
    let mut spent = 0;
    let mut more = false;
    for (index, row) in rows.enumerate() {
        if index == HISTORY_ROWS {
            more = true;
            break;
        }
        let message: ConversationMessage = decode(row?)?;
        let source = message.agent_id.as_deref().unwrap_or("human");
        let body = if message.sender_type == "agent" { Cow::Owned(public_reply_text(&message.body)) }
            else { Cow::Borrowed(message.body.as_str()) };
        let body = body.trim();
        if body.is_empty() { continue; }
        let line = format!("[{} | {} | {}]\n{}", message.id, message.sender_type, source, excerpt(body, MESSAGE_CHARS));
        let cost = line.chars().count() + 2;
        if spent + cost <= HISTORY_CHARS {
            spent += cost;
            selected.push(line);
        } else if references.len() < 8 {
            let title: String = body.lines().next().unwrap_or_default().chars().take(100).collect();
            references.push(format!("[{}] {title}", message.id));
        } else {
            more = true;
        }
    }
    selected.reverse();
    if !references.is_empty() || more {
        references.reverse();
        let intro = if more { "较早消息可在原话题查阅。以下是部分引用：" } else { "其余消息可在原话题查阅：" };
        selected.insert(0, format!("{intro}\n{}", references.join("\n")));
    }
    Ok(selected)
}

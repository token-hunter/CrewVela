use super::{run_status, CollaborationConversation};
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemberReceipt {
    message_id: String,
    sender_id: Option<String>,
    agent_id: Option<String>,
    run_id: Option<String>,
    purpose: String,
    status: &'static str,
    arguments: Value,
    error: Option<String>,
}

pub(super) fn for_topic(
    db: &Connection,
    topic: &CollaborationConversation,
) -> Result<HashMap<String, Vec<MemberReceipt>>> {
    let mut statement = db.prepare(
        "SELECT json_extract(round.data_json,'$.userMessageId'), x.message_id, m.sender_id,
                x.recipient_id, x.purpose, x.arguments, r.id, r.status, r.session_locator_json,
                r.error, r.output_summary, task.status, recipient.enabled,
                EXISTS(SELECT 1 FROM ma_runtime_events e WHERE e.run_id=r.id AND e.event_type='agent_activity'),
                EXISTS(SELECT 1 FROM ma_runtime_events e WHERE e.run_id=r.id AND e.event_type='conversation_prompt_received')
         FROM ma_conversation_rounds round
         JOIN ma_member_messages x ON x.round_id=round.id
         JOIN ma_messages m ON m.id=x.message_id AND m.project_id=round.project_id
         LEFT JOIN ma_agents recipient ON recipient.id=x.recipient_id AND recipient.project_id=m.project_id
         LEFT JOIN ma_tasks task ON task.id='cli-message-'||m.id AND task.project_id=m.project_id
         LEFT JOIN ma_runs r ON r.id=(SELECT rr.id FROM ma_runs rr WHERE rr.task_id=task.id ORDER BY rr.attempt DESC LIMIT 1)
         WHERE round.project_id=?1 AND json_extract(round.data_json,'$.conversationId')=?2
           AND x.purpose IN ('status','question','handoff') AND json_extract(x.arguments,'$.purpose') IS NOT NULL
         ORDER BY m.created_at,m.rowid",
    )?;
    let mut result: HashMap<String, Vec<MemberReceipt>> = HashMap::new();
    let mut rows = statement.query(params![topic.project_id, topic.id])?;
    while let Some(row) = rows.next()? {
        let request: String = row.get(0)?;
        let purpose: String = row.get(4)?;
        let raw_arguments: String = row.get(5)?;
        let phase: Option<String> = row.get(7)?;
        let locator: Option<String> = row.get(8)?;
        let output: Option<String> = row.get(10)?;
        let task: Option<String> = row.get(11)?;
        let enabled: Option<bool> = row.get(12)?;
        let activity: bool = row.get(13)?;
        let received: bool = row.get(14)?;
        let status = if purpose == "status" {
            "inbox"
        } else if let Some(phase) = phase {
            match run_status(&phase, locator.as_deref()) {
                "running" if activity => "running",
                "running" if received => "received",
                "running" => "awaiting_receipt",
                "completed" if purpose == "handoff" || output.as_deref().is_none_or(|body| body.trim().is_empty()) => "handled",
                "paused" => "unconfirmed",
                status => status,
            }
        } else {
            match task.as_deref() {
                Some("cancelled") => "cancelled",
                Some("failed") => "failed",
                _ if enabled != Some(true) => "not_dispatched",
                _ => "queued",
            }
        };
        result.entry(request).or_default().push(MemberReceipt {
            message_id: row.get(1)?, sender_id: row.get(2)?, agent_id: row.get(3)?, run_id: row.get(6)?,
            purpose, status, arguments: serde_json::from_str(&raw_arguments)?, error: row.get(9)?,
        });
    }
    Ok(result)
}

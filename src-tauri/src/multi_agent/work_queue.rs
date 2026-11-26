use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

/// Saved conversation requests and stable origins for engineering tasks.
/// The open conversation already supplies its public messages, plans and rounds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkRecord {
    pub project_id: String,
    pub conversation_id: String,
    pub topic_title: Option<String>,
    pub kind: String,
    pub id: String,
    pub status: String,
    pub message_id: String,
    pub request: String,
    pub summary: Option<String>,
    pub updated_at: i64,
}

pub(super) fn records(db: &Connection, project_id: &str) -> Result<Vec<WorkRecord>> {
    let mut query = db.prepare_cached(
        "WITH topics(id, current) AS (
            SELECT id, 1 FROM ma_conversations WHERE project_id=?1
            UNION ALL SELECT h.id, 0 FROM ma_conversation_history h
            WHERE h.project_id=?1 AND h.disposition NOT IN ('cleared','deleted')
              AND NOT EXISTS (SELECT 1 FROM ma_conversations c WHERE c.project_id=?1 AND c.id=h.id)
         ), rounds AS (
            SELECT r.id, r.status, r.created_at, r.rowid AS position, r.data_json,
                   topics.id AS topic, topics.current,
                   json_extract(r.data_json,'$.userMessageId') AS request_id
            FROM ma_conversation_rounds r JOIN topics ON topics.id=json_extract(r.data_json,'$.conversationId')
            WHERE r.project_id=?1
         ), items(kind,id,status,topic,message_id,request_id,summary,updated_at) AS (
            SELECT 'plan',p.id,'pending',r.topic,json_extract(p.data_json,'$.messageId'),r.request_id,
                   json_extract(p.data_json,'$.summary'),json_extract(p.data_json,'$.createdAt')
            FROM ma_conversation_plans p JOIN rounds r ON r.id=p.round_id
            WHERE p.project_id=?1 AND r.current=0 AND json_extract(p.data_json,'$.status')='pending'
              AND json_type(p.data_json,'$.managed') IS NOT 'object'
            UNION ALL
            SELECT 'round',r.id,r.status,r.topic,r.request_id,r.request_id,
                   json_extract(r.data_json,'$.stopReason'),json_extract(r.data_json,'$.updatedAt')
            FROM rounds r WHERE r.current=0 AND r.status IN ('queued','running','waiting_approval','needs_decision','unconfirmed','stopping')
              AND NOT EXISTS (SELECT 1 FROM rounds newer WHERE newer.topic=r.topic
                              AND (newer.created_at,newer.position)>(r.created_at,r.position))
            UNION ALL
            SELECT 'queued',m.id,json_extract(m.data_json,'$.status'),topics.id,m.id,m.id,NULL,
                   json_extract(m.data_json,'$.updatedAt')
            FROM ma_conversation_messages m JOIN topics ON topics.id=json_extract(m.data_json,'$.conversationId')
            WHERE m.project_id=?1 AND topics.current=0 AND json_type(m.data_json,'$.queuedInput')='object'
              AND json_extract(m.data_json,'$.status') IN ('queued','paused','unconfirmed','needs_decision')
            UNION ALL
            SELECT 'origin',t.id,t.status,r.topic,turn.id,r.request_id,NULL,t.updated_at
            FROM ma_tasks t JOIN ma_conversation_turns turn ON turn.task_id=t.id JOIN rounds r ON r.id=turn.round_id
            WHERE t.project_id=?1 AND r.current=0 AND (t.status IN ('running','claimed','waiting_approval')
              OR EXISTS(SELECT 1 FROM ma_approvals a WHERE a.project_id=?1 AND a.task_id=t.id AND a.status='pending'))
            UNION ALL
            SELECT 'origin',t.id,t.status,r.topic,json_extract(p.data_json,'$.messageId'),r.request_id,NULL,t.updated_at
            FROM ma_tasks t JOIN ma_conversation_task_scopes s ON s.task_id=t.id
            JOIN ma_conversation_plans p ON p.id=s.plan_id AND p.project_id=t.project_id JOIN rounds r ON r.id=p.round_id
            WHERE t.project_id=?1 AND r.current=0 AND t.status NOT IN ('draft','done','cancelled','failed')
         )
         SELECT i.topic,COALESCE(title.title,(SELECT SUBSTR(TRIM(json_extract(m.data_json,'$.body')),1,80)
                FROM ma_conversation_messages m WHERE m.project_id=?1 AND json_extract(m.data_json,'$.conversationId')=i.topic
                  AND json_extract(m.data_json,'$.senderType')='user' AND json_extract(m.data_json,'$.kind')<>'queued_message'
                ORDER BY m.created_at,m.rowid LIMIT 1)),
                i.kind,i.id,i.status,i.message_id,json_extract(request.data_json,'$.body'),i.summary,i.updated_at
         FROM items i JOIN ma_conversation_messages request ON request.id=i.request_id AND request.project_id=?1
              AND json_extract(request.data_json,'$.conversationId')=i.topic AND json_extract(request.data_json,'$.senderType')='user'
         LEFT JOIN ma_conversation_titles title ON title.project_id=?1 AND title.conversation_id=i.topic",
    )?;
    let rows = query.query_map([project_id], |row| Ok(WorkRecord {
        project_id: project_id.into(), conversation_id: row.get(0)?, topic_title: row.get(1)?,
        kind: row.get(2)?, id: row.get(3)?, status: row.get(4)?, message_id: row.get(5)?,
        request: row.get(6)?, summary: row.get(7)?, updated_at: row.get(8)?,
    }))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

use super::{
    CollaborationMessage, ConversationMessage, ConversationRound, MultiAgentStore,
    SendCollaborationMessageInput,
};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Applies across all automatic routes for one originating request. Explicit first
/// answers are reserved for each selected participant, separately from these slots.
const MAX_EXTRA_TURNS: u32 = 8;

#[derive(Debug, thiserror::Error)]
#[error("已达到该来源任务的自动接话、私信和委派上限，请由用户补充方向后继续")]
pub(super) struct TurnLimit;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct RouteDenied(&'static str);

pub(crate) fn permanent_rejection(error: &anyhow::Error) -> bool {
    error.is::<TurnLimit>() || error.is::<RouteDenied>()
}

pub(super) fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS ma_member_messages (
            message_id TEXT PRIMARY KEY REFERENCES ma_messages(id) ON DELETE CASCADE,
            root_id TEXT NOT NULL, round_id TEXT REFERENCES ma_conversation_rounds(id) ON DELETE CASCADE,
            recipient_id TEXT, purpose TEXT NOT NULL, arguments TEXT NOT NULL,
            request_id TEXT, run_id TEXT REFERENCES ma_runs(id) ON DELETE CASCADE,
            UNIQUE(run_id, request_id)
         );
         CREATE INDEX IF NOT EXISTS ma_member_messages_inbox ON ma_member_messages(purpose, recipient_id);
         CREATE INDEX IF NOT EXISTS ma_member_messages_round ON ma_member_messages(round_id);
         CREATE TABLE IF NOT EXISTS ma_member_turn_budget (
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            root_id TEXT NOT NULL, action_id TEXT NOT NULL, cost INTEGER NOT NULL,
            PRIMARY KEY(project_id, root_id, action_id)
         );
         CREATE VIEW IF NOT EXISTS ma_member_message_readers AS
         SELECT x.message_id, a.id AS agent_id, json_extract(r.data_json,'$.conversationId') AS topic_id
         FROM ma_member_messages x JOIN ma_messages m ON m.id=x.message_id
         JOIN ma_agents a ON a.project_id=m.project_id LEFT JOIN ma_conversation_rounds r ON r.id=x.round_id
         WHERE a.id=m.sender_id OR a.id=x.recipient_id OR (x.recipient_id IS NULL AND
           (x.round_id IS NULL OR EXISTS(SELECT 1 FROM json_each(r.data_json,'$.participantAgentIds') WHERE value=a.id)));
         CREATE VIEW IF NOT EXISTS ma_inactive_member_tasks AS
         SELECT 'cli-message-'||x.message_id AS task_id FROM ma_member_messages x
         JOIN ma_conversation_rounds r ON r.id=x.round_id WHERE NOT EXISTS(
            SELECT 1 FROM ma_conversations c WHERE c.project_id=r.project_id AND c.id=json_extract(r.data_json,'$.conversationId'));"
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Purpose {
    Status,
    Question,
    Handoff,
}

impl Purpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Question => "question",
            Self::Handoff => "handoff",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Handoff {
    pub todos: Vec<String>,
    pub acceptance: Vec<String>,
    pub artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemberSend {
    pub body: String,
    pub target_agent_id: Option<String>,
    pub purpose: Option<Purpose>,
    pub handoff: Option<Handoff>,
    pub request_id: Option<String>,
}

impl MemberSend {
    fn validate(&self, legacy: bool) -> Result<Purpose> {
        let purpose = self.purpose.context("请选择成员消息目的")?;
        let limit = if legacy { 32_000 } else { 2000 };
        ensure!(
            !self.body.trim().is_empty() && self.body.chars().count() <= limit,
            "消息正文限 1 至 {limit} 字，长记录请引用产物"
        );
        ensure!(
            purpose == Purpose::Status || self.target_agent_id.is_some(),
            "提问和交接必须指定一位接收成员"
        );
        ensure!(
            (purpose == Purpose::Handoff) == self.handoff.is_some(),
            "交接须附待办、验收条件和证据产物"
        );
        if let Some(handoff) = &self.handoff {
            for (items, max, name) in [
                (&handoff.todos, 480, "待办"),
                (&handoff.acceptance, 480, "验收条件"),
                (&handoff.artifact_ids, 128, "证据产物 ID"),
            ] {
                ensure!(
                    (1..=8).contains(&items.len())
                        && items
                            .iter()
                            .all(|s| !s.trim().is_empty() && s.chars().count() <= max),
                    "{name}须提供 1 至 8 项且每项不超过 {max} 字"
                );
            }
        }
        if let Some(request) = &self.request_id {
            uuid::Uuid::parse_str(request).context("requestId 必须是 UUID")?;
        }
        Ok(purpose)
    }

    fn text(&self, purpose: Purpose, project: &str) -> String {
        let label = match purpose {
            Purpose::Status => "状态更新",
            Purpose::Question => "定向提问",
            Purpose::Handoff => "交接目标",
        };
        let mut body = format!("【{label}】{}", self.body.trim());
        if let Some(handoff) = &self.handoff {
            for (heading, items) in [("待办", &handoff.todos), ("验收条件", &handoff.acceptance)]
            {
                body.push_str(&format!("\n{heading}"));
                for item in items {
                    body.push_str(&format!("\n- {}", item.trim()));
                }
            }
            body.push_str("\n证据产物");
            for id in &handoff.artifact_ids {
                body.push_str(&format!("\n- crewvela://projects/{project}/artifacts/{id}"));
            }
        }
        body
    }
}

#[derive(Debug)]
struct Origin {
    root: String,
    round: Option<String>,
    topic: Option<String>,
    participants: Option<Vec<String>>,
    private_members: Option<(String, String)>,
    limit: u32,
}

fn round_origin(project: &str, round: &ConversationRound) -> Result<Origin> {
    ensure!(round.project_id == project, "来源话题不属于当前项目");
    let limit = if round.mode == "discussion" && round.intent == "exchange" {
        (round.participant_agent_ids.len() as u32)
            .saturating_mul(round.max_rounds)
            .min(24)
    } else {
        MAX_EXTRA_TURNS
    };
    Ok(Origin {
        root: round.user_message_id.clone(),
        round: Some(round.id.clone()),
        topic: Some(round.conversation_id.clone()),
        participants: Some(round.participant_agent_ids.clone()),
        private_members: None,
        limit,
    })
}

fn origin(db: &Connection, project: &str, task: &str) -> Result<Origin> {
    let child = task.strip_prefix("cli-message-").unwrap_or("");
    let saved = db.query_row(
        "SELECT x.root_id,x.round_id,m.sender_id,x.recipient_id FROM ma_member_messages x JOIN ma_messages m ON m.id=x.message_id
         WHERE x.message_id=?1 AND m.project_id=?2", params![child, project],
        |row| Ok((row.get::<_,String>(0)?, row.get::<_,Option<String>>(1)?, Some((row.get::<_,String>(2)?,row.get::<_,String>(3)?))))).optional()?;
    let (root, round, private_members) = match saved {
        Some(saved) => saved,
        None => {
            let round = db.query_row(
                "SELECT t.round_id FROM ma_conversation_turns t JOIN ma_conversation_rounds r ON r.id=t.round_id
                   WHERE t.task_id=?1 AND r.project_id=?2
                 UNION ALL SELECT p.round_id FROM ma_conversation_task_scopes s
                   JOIN ma_conversation_plans p ON p.id=s.plan_id WHERE s.task_id=?1 AND p.project_id=?2 LIMIT 1",
                params![task, project], |row| row.get(0)).optional()?;
            (task.to_string(), round, None)
        }
    };
    if let Some(round_id) = round {
        let data: String = db.query_row(
            "SELECT data_json FROM ma_conversation_rounds WHERE id=?1 AND project_id=?2",
            params![round_id, project],
            |row| row.get(0),
        )?;
        let mut context = round_origin(project, &serde_json::from_str(&data)?)?;
        context.private_members = private_members;
        return Ok(context);
    }
    Ok(Origin {
        root,
        round: None,
        topic: None,
        participants: None,
        private_members,
        limit: MAX_EXTRA_TURNS,
    })
}

pub(super) fn topic_for_task(db: &Connection, project: &str, task: &str) -> Result<Option<String>> {
    Ok(origin(db, project, task)?.topic)
}

fn reserve(db: &Connection, project: &str, source: &Origin, action: &str, cost: u32) -> Result<()> {
    let used: u32 = db.query_row("SELECT COALESCE(SUM(cost),0) FROM ma_member_turn_budget WHERE project_id=?1 AND root_id=?2",
        params![project, source.root], |r| r.get(0))?;
    if used.saturating_add(cost) > source.limit {
        return Err(TurnLimit.into());
    }
    db.execute(
        "INSERT INTO ma_member_turn_budget(project_id,root_id,action_id,cost) VALUES(?1,?2,?3,?4)",
        params![project, source.root, action, cost],
    )?;
    Ok(())
}

pub(super) fn register_conversation_turn(
    db: &Connection,
    round: &ConversationRound,
    task: &str,
    target: &ConversationMessage,
) -> Result<()> {
    if target.sender_type != "agent" {
        return Ok(());
    }
    let extra: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_conversation_turns current JOIN ma_conversation_turns previous
          ON previous.round_id=current.round_id AND previous.ordinal<current.ordinal
          JOIN ma_tasks a ON a.id=previous.task_id JOIN ma_tasks b ON b.id=current.task_id AND a.assignee_agent_id=b.assignee_agent_id
          WHERE current.task_id=?1)", [task], |r| r.get(0))?;
    if extra {
        reserve(
            db,
            &round.project_id,
            &round_origin(&round.project_id, round)?,
            task,
            1,
        )?;
    }
    Ok(())
}

pub(super) fn reserve_delegation(
    db: &Connection,
    project: &str,
    task: &str,
    delegation: &str,
) -> Result<()> {
    // A checkpointed design handoff adds a design turn and a resume of its caller.
    reserve(db, project, &origin(db, project, task)?, delegation, 2)
}

fn allowed_recipient(source: &Origin, sender: &str, target: Option<&str>) -> Result<()> {
    if let Some((from, to)) = &source.private_members {
        ensure!(
            target.is_some_and(|id| id == from || id == to),
            RouteDenied("私信任务只能回复原通信参与者")
        );
    }
    if let Some(target) = target {
        ensure!(target != sender, RouteDenied("接收者必须是另一位成员"));
        ensure!(
            source
                .participants
                .as_ref()
                .is_none_or(|roster| roster.iter().any(|id| id == target)),
            RouteDenied("接收者不在来源任务已选的成员中")
        );
    }
    Ok(())
}

fn ensure_current_topic(db: &Connection, project: &str, source: &Origin) -> Result<()> {
    if let Some(topic) = &source.topic {
        let current: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_conversations WHERE project_id=?1 AND id=?2)",
            params![project, topic],
            |r| r.get(0),
        )?;
        ensure!(current, RouteDenied("来源话题已切换，请回到原话题后继续"));
    }
    Ok(())
}

fn ensure_run(db: &Connection, project: &str, task: &str, run: &str, agent: &str) -> Result<()> {
    let active: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_runs r JOIN ma_agent_sessions s ON s.project_id=r.project_id AND s.agent_id=r.agent_id
         JOIN ma_agents a ON a.id=r.agent_id WHERE r.project_id=?1 AND r.task_id=?2 AND r.id=?3 AND r.agent_id=?4 AND a.enabled=1
         AND s.current_run_id=r.id AND s.current_task_id=r.task_id AND s.control_mode='auto'
         AND r.status IN ('starting','running') AND COALESCE(json_extract(r.session_locator_json,'$.stopUnverified'),0)=0)",
        params![project,task,run,agent], |r| r.get(0))?;
    ensure!(active, "来源任务已结束、被接管或成员已切换，无法发送");
    ensure!(
        super::managed_delivery::task_target(db, task)?.is_none(),
        "托管任务只能使用已授予的协作工具"
    );
    Ok(())
}

fn receipt(message: &CollaborationMessage, purpose: Purpose, source: &Origin) -> Value {
    json!({"messageId":message.id, "sourceTaskId":message.task_id, "sourceId":source.root, "conversationId":source.topic,
        "targetAgentId":message.target_agent_id, "purpose":purpose, "stage":if purpose==Purpose::Status {"inbox"} else {"queued"}})
}

impl MultiAgentStore {
    /// Unlabelled project MCP messages retain their passive inbox semantics.
    /// Their readership still follows a conversation or a private delivery task.
    pub(super) fn legacy_topic_inbox(
        &self,
        project: &str,
        task: &str,
        run: &str,
        sender: &str,
        recipient: Option<String>,
        body: &str,
    ) -> Result<Option<CollaborationMessage>> {
        let mut db = self.connection.lock().unwrap();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source = origin(&tx, project, task)?;
        if source.round.is_none() && source.private_members.is_none() {
            return Ok(None);
        }
        ensure_run(&tx, project, task, run, sender)?;
        ensure_current_topic(&tx, project, &source)?;
        allowed_recipient(&source, sender, recipient.as_deref())?;
        if let Some(id) = &recipient {
            let allowed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_agents WHERE id=?1 AND project_id=?2 AND enabled=1)",
                params![id,project],|r|r.get(0))?;
            ensure!(allowed, "接收成员无效或已禁用");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let message = Self::insert_collaboration_message(
            &tx,
            SendCollaborationMessageInput {
                project_id: project.into(),
                task_id: Some(task.into()),
                sender_type: "agent".into(),
                sender_id: Some(sender.into()),
                target_agent_id: recipient,
                body: body.into(),
            },
            &id,
        )?;
        tx.execute("INSERT INTO ma_member_messages(message_id,root_id,round_id,recipient_id,purpose,arguments,run_id)
            VALUES(?1,?2,?3,?4,'status','{}',?5)",params![id,source.root,source.round,message.target_agent_id,run])?;
        tx.commit()?;
        Ok(Some(message))
    }

    /// Old clients have only a bound app session. Use the file's host-observed
    /// publication time, even when ingestion follows a run's normal completion.
    pub(crate) fn record_bound_session_message(
        &self,
        sender: &crate::sessions::SessionRecord,
        target: &crate::sessions::SessionRecord,
        id: &str,
        body: &str,
        published_at: i64,
    ) -> Result<bool> {
        let (Some(project), Some(agent)) = (
            sender.multi_agent_project_id.as_deref(),
            sender.multi_agent_agent_id.as_deref(),
        ) else {
            return Ok(false);
        };
        let mut db = self.connection.lock().unwrap();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_member_messages x JOIN ma_messages m ON m.id=x.message_id
             WHERE x.message_id=?1 AND x.purpose='direct' AND m.project_id=?2 AND m.sender_id=?3 AND x.recipient_id=?4 AND m.body=?5)",
             params![id,project,agent,target.multi_agent_agent_id,body.trim()], |row| row.get(0))?;
        if prior {
            return Ok(true);
        }
        let mut query=tx.prepare("SELECT r.id,r.task_id,(r.status IN ('starting','running','succeeded') AND s.control_mode='auto'
            AND COALESCE(json_extract(r.session_locator_json,'$.stopUnverified'),0)=0)
            FROM ma_runs r JOIN ma_agent_sessions s ON r.project_id=s.project_id AND r.agent_id=s.agent_id
            WHERE s.project_id=?1 AND s.agent_id=?2 AND s.app_session_id=?3
            AND r.created_at<=?4 AND (r.finished_at IS NULL OR r.finished_at>=?4) LIMIT 2")?;
        let candidates = query
            .query_map(params![project, agent, sender.id, published_at], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(query);
        let mut candidates = candidates.into_iter();
        let Some((run, task, eligible)) = candidates.next() else {
            return Ok(false);
        };
        ensure!(
            candidates.next().is_none(),
            RouteDenied("旧会话消息的来源 Run 不唯一")
        );
        ensure!(
            eligible && super::managed_delivery::task_target(&tx, &task)?.is_none(),
            RouteDenied("来源 Run 已停止、接管或未获通信权限")
        );
        if super::conversation::is_conversation_task(&task) {
            let policy: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_conversation_turns t JOIN ma_conversation_rounds r ON r.id=t.round_id,
                json_each(r.data_json,'$.memberPolicies') p WHERE t.task_id=?1 AND p.key=?2 AND json_extract(p.value,'$.version')=2)",
                params![task,agent],|r|r.get(0))?;
            ensure!(policy, RouteDenied("本轮讨论未授权成员私信"));
        }
        let source = origin(&tx, project, &task)?;
        let receiver = target
            .multi_agent_agent_id
            .as_deref()
            .filter(|_| target.multi_agent_project_id.as_deref() == Some(project));
        if receiver.is_none() && source.round.is_none() && source.private_members.is_none() {
            return Ok(false);
        }
        let receiver = receiver.context(RouteDenied("来源任务仅允许联系原话题的已选项目成员"))?;
        ensure_current_topic(&tx, project, &source)?;
        allowed_recipient(&source, agent, Some(receiver))?;
        let valid: bool = tx.query_row(
            "SELECT COUNT(*)=2 FROM ma_agents WHERE project_id=?1 AND id IN (?2,?3) AND enabled=1",
            params![project, agent, receiver],
            |row| row.get(0),
        )?;
        ensure!(valid, RouteDenied("来源或接收成员已禁用"));
        reserve(&tx, project, &source, id, 1)?;
        Self::insert_collaboration_message(
            &tx,
            SendCollaborationMessageInput {
                project_id: project.into(),
                task_id: Some(task),
                sender_type: "agent".into(),
                sender_id: Some(agent.into()),
                target_agent_id: Some(receiver.into()),
                body: body.into(),
            },
            id,
        )?;
        tx.execute("INSERT INTO ma_member_messages(message_id,root_id,round_id,recipient_id,purpose,arguments,run_id)
           VALUES(?1,?2,?3,?4,'direct','{}',?5)",params![id,source.root,source.round,receiver,run])?;
        tx.commit()?;
        Ok(true)
    }

    pub(super) fn send_member_message(
        &self,
        project: &str,
        task: &str,
        run: &str,
        agent: &str,
        input: MemberSend,
        legacy: bool,
    ) -> Result<Value> {
        let purpose = input.validate(legacy)?;
        let target = input.target_agent_id.as_deref();
        let recipient = target.map(|id| self.agent_profile(id)).transpose()?;
        ensure!(
            recipient
                .as_ref()
                .is_none_or(|a| a.enabled && a.project_id == project),
            "接收者不属于当前项目或已禁用"
        );
        let _runtime = recipient
            .as_ref()
            .filter(|_| purpose != Purpose::Status)
            .map(|a| crate::runtime_preferences::acquire(a.runtime_kind))
            .transpose()?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_run(&tx, project, task, run, agent)?;
        if let Some(receiver) = target {
            let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_agents WHERE id=?1 AND project_id=?2 AND enabled=1)",params![receiver,project],|r|r.get(0))?;
            ensure!(valid, "接收者不属于当前项目或已禁用");
        }
        let source = origin(&tx, project, task)?;
        allowed_recipient(&source, agent, target)?;
        let arguments = serde_json::to_string(&input)?;
        let body = if legacy {
            input.body.clone()
        } else {
            input.text(purpose, project)
        };
        let previous = tx.query_row("SELECT message_id,arguments FROM ma_member_messages WHERE run_id=?1 AND request_id=?2",
            params![run,input.request_id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?))).optional()?;
        let (id, retry) = match previous {
            Some((id, saved)) => {
                ensure!(saved == arguments, "相同 requestId 已用于不同消息");
                (id, true)
            }
            None => (uuid::Uuid::new_v4().to_string(), false),
        };
        if !retry {
            ensure_current_topic(&tx, project, &source)?;
            if let Some(handoff) = &input.handoff {
                for artifact in &handoff.artifact_ids {
                    let allowed: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM ma_artifacts a WHERE a.id=?1 AND a.project_id=?2
                         AND (a.task_id=?3 OR EXISTS(SELECT 1 FROM ma_task_dependencies d WHERE d.task_id=?3 AND d.depends_on_task_id=a.task_id)))",
                        params![artifact,project,task], |r| r.get(0))?;
                    ensure!(allowed, "证据产物不存在或不在来源任务的依赖范围内");
                }
            }
            if purpose != Purpose::Status {
                reserve(&tx, project, &source, &id, 1)?;
            }
        }
        let message = Self::insert_collaboration_message(
            &tx,
            SendCollaborationMessageInput {
                project_id: project.into(),
                task_id: Some(task.into()),
                sender_type: "agent".into(),
                sender_id: Some(agent.into()),
                target_agent_id: input.target_agent_id,
                body,
            },
            &id,
        )?;
        if !retry {
            tx.execute("INSERT INTO ma_member_messages(message_id,root_id,round_id,recipient_id,purpose,arguments,request_id,run_id)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![id,source.root,source.round,message.target_agent_id,purpose.as_str(),arguments,input.request_id,run])?;
        }
        tx.commit()?;
        Ok(receipt(&message, purpose, &source))
    }

    pub(super) fn shared_evidence(
        &self,
        project: &str,
        task: &str,
        agent: &str,
        artifact: &str,
    ) -> Result<bool> {
        let db = self.connection.lock().unwrap();
        let topic = topic_for_task(&db, project, task)?;
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_member_messages x JOIN ma_member_message_readers readers ON readers.message_id=x.message_id
             JOIN ma_messages m ON m.id=x.message_id, json_each(x.arguments,'$.handoff.artifactIds') ref
             WHERE m.project_id=?1 AND x.recipient_id=?2 AND readers.agent_id=?2 AND readers.topic_id IS ?3 AND ref.value=?4)",
            params![project,agent,topic,artifact], |r| r.get(0))?)
    }

    /// The normal scheduler owns claims, runtime selection and recovery. A status
    /// never appears in this query. A previously created task is only reawakened.
    pub(crate) fn drain_member_inbox(
        &self,
        queue: &crate::control::SessionActionQueue,
    ) -> Result<std::collections::HashSet<String>> {
        let mut projects = std::collections::HashSet::new();
        queue.with_delivery_lock(|| -> Result<()> {
            let pending = {
                let db = self.connection.lock().unwrap();
                let mut q = db.prepare(
                    "SELECT m.project_id,m.id,m.sender_id,x.recipient_id,m.task_id,m.body,
                            COALESCE(s.app_session_id,m.sender_id),COALESCE(r.app_session_id,x.recipient_id),t.id IS NOT NULL
                     FROM ma_member_messages x JOIN ma_messages m ON m.id=x.message_id
                     JOIN ma_agents recipient ON recipient.id=x.recipient_id AND recipient.enabled=1
                     LEFT JOIN ma_agent_sessions s ON s.project_id=m.project_id AND s.agent_id=m.sender_id
                     LEFT JOIN ma_agent_sessions r ON r.project_id=m.project_id AND r.agent_id=x.recipient_id
                     LEFT JOIN ma_tasks t ON t.id='cli-message-'||m.id
                     WHERE x.purpose IN ('question','handoff') AND (t.id IS NULL OR t.status='ready')
                       AND NOT EXISTS(SELECT 1 FROM ma_inactive_member_tasks z WHERE z.task_id='cli-message-'||m.id)
                     ORDER BY m.created_at,m.rowid")?;
                let rows = q.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
                    r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,
                    r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,bool>(8)?)))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (project,id,sender,target,task,body,from_session,to_session,queued) in pending {
                if queued { projects.insert(project); continue; }
                let prompt = format!("{body}\n\n来源成员 {sender}，来源任务 {task}，消息 {id}。需要查看交接证据时，向 cc_artifact_inspect 提供清单中的产物 ID。回复结果可用 cc_message_send 的 status，只有新的具体问题才用 question。");
                if queue.admit_member_turn(&from_session, &to_session, || self.enqueue_cli_message_task(&project,&target,&id,&prompt))? {
                    projects.insert(project);
                }
            }
            Ok(())
        })?;
        Ok(projects)
    }
}

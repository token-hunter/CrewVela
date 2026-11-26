use super::*;

fn template_topic(connection: &Connection, project: &str, topic: &str) -> Result<CollaborationConversation> {
    connection.query_row(
        "SELECT created_at FROM ma_conversations WHERE project_id=?1 AND id=?2
         UNION ALL SELECT created_at FROM ma_conversation_history WHERE project_id=?1 AND id=?2
           AND disposition NOT IN ('cleared','deleted') LIMIT 1",
        params![project, topic], |row| Ok(CollaborationConversation {
            id: topic.to_owned(), project_id: project.to_owned(), created_at: row.get(0)?, title: None,
        }),
    ).optional()?.context("模板原话题已被清空、删除或不属于该项目")
}

fn template_member(connection: &Connection, project: &str, agent: &str) -> Result<AgentRuntimeKind> {
    let (runtime, status): (String, String) = connection.query_row(
        "SELECT a.runtime_kind,p.status FROM ma_agents a JOIN ma_projects p ON p.id=a.project_id
         WHERE a.project_id=?1 AND a.id=?2 AND a.enabled=1",
        params![project, agent], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?.context("模板成员或项目已不可用")?;
    ensure!(!matches!(status.as_str(), "paused" | "cancelled"), "项目已暂停或取消，不能执行模板");
    match runtime.as_str() {
        "claude_code" => Ok(AgentRuntimeKind::ClaudeCode),
        "pi" => Ok(AgentRuntimeKind::Pi),
        _ => bail!("只读模板需要支持受限研究的 Claude Code 或 Pi 成员"),
    }
}

pub(in crate::multi_agent) fn validate_template_claim(connection: &Connection, task: &str, project: &str, agent: &str) -> Result<()> {
    if !is_conversation_task(task) { return Ok(()); }
    let template = connection.query_row("SELECT EXISTS(SELECT 1 FROM ma_conversation_turns t
        JOIN ma_conversation_rounds r ON r.id=t.round_id WHERE t.task_id=?1 AND json_extract(r.data_json,'$.templateRun')=1)",
        [task], |row| row.get::<_, bool>(0))?;
    if template { template_member(connection, project, agent)?; }
    Ok(())
}

pub(super) fn checked_destination(connection: &Connection, selected: &CollaborationConversation, input: &ConversationSendInput) -> Result<CollaborationConversation> {
    let topic = input.expected_conversation_id.as_deref().context("模板缺少原话题")?;
    ensure!(input.mode == "discussion" && input.intent == "chat" && !input.managed && !input.native_capabilities
        && input.attachment_ids.is_empty() && input.participant_agent_ids.len() == 1
        && input.task_rules.as_ref().is_some_and(|rules| rules.tools == "research" && rules.archived_rule_id.is_none()),
        "模板必须通过单成员受限研究通道运行");
    template_member(connection, &input.project_id, &input.participant_agent_ids[0])?;
    let waiting: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_conversation_messages WHERE project_id=?1
         AND json_extract(data_json,'$.conversationId')=?2 AND json_extract(data_json,'$.status')='queued'
         AND json_type(data_json,'$.queuedInput')='object')",
        params![input.project_id, selected.id], |row| row.get(0),
    )?;
    ensure!(!waiting, "当前话题有待派发消息，请在其处理后重试");
    template_topic(connection, &input.project_id, topic)
}

fn round_for_request(connection: &Connection, project: &str, topic: &str, client: &str) -> Result<Option<ConversationRound>> {
    connection.query_row("SELECT data_json FROM ma_conversation_rounds WHERE project_id=?1
        AND client_message_id=?2 AND json_extract(data_json,'$.conversationId')=?3",
        params![project, client, topic], |row| row.get::<_, String>(0)).optional()?.map(decode).transpose()
}

impl MultiAgentStore {
    pub(crate) fn template_schedule_hold(&self, project: &str) -> Result<Option<&'static str>> {
        let conn = self.connection.lock().unwrap();
        let paused = conn.query_row("SELECT EXISTS(SELECT 1 FROM ma_projects WHERE id=?1 AND status='paused')", [project], |row| row.get::<_, bool>(0))?;
        if paused { return Ok(Some("协作项目已暂停，本次模板跳过")); }
        let occupied = conn.query_row("SELECT EXISTS(SELECT 1 FROM ma_conversation_rounds WHERE project_id=?1
            AND status IN ('queued','running','waiting_approval','stopping','unconfirmed'))
            OR EXISTS(SELECT 1 FROM ma_conversation_messages m JOIN ma_conversations c ON c.project_id=m.project_id
                WHERE m.project_id=?1 AND json_extract(m.data_json,'$.conversationId')=c.id
                AND json_extract(m.data_json,'$.status')='queued' AND json_type(m.data_json,'$.queuedInput')='object')",
            [project], |row| row.get::<_, bool>(0))?;
        Ok(occupied.then_some("项目正在处理已有对话，本次模板跳过"))
    }

    pub(crate) fn validate_template_destination(&self, project: &str, topic: &str, agent: &str) -> Result<()> {
        let runtime = {
            let connection = self.connection.lock().unwrap();
            template_topic(&connection, project, topic)?;
            template_member(&connection, project, agent)?
        };
        runtime.ensure_available()
    }

    pub(crate) fn template_round(&self, project: &str, topic: &str, client: &str) -> Result<Option<ConversationRound>> {
        let connection = self.connection.lock().unwrap();
        round_for_request(&connection, project, topic, client)
    }

    pub(super) fn topic_is_selected(&self, project: &str, topic: &str) -> Result<bool> {
        Ok(self.connection.lock().unwrap().query_row("SELECT EXISTS(SELECT 1 FROM ma_conversations WHERE project_id=?1 AND id=?2)",
            params![project, topic], |row| row.get(0))?)
    }
}

impl ConversationSupervisor {
    pub(crate) fn send_template(&self, store: MultiAgentStore, input: ConversationSendInput,
        host: AgentTerminalHost, bridge: CollaborationBridge, scheduled: bool) -> Result<()> {
        self.send_in_topic(store, input, host, bridge, None,
            if scheduled { TopicDispatch::ScheduledTemplate } else { TopicDispatch::TemplateTrial })
    }

    pub(crate) fn stop_template_round(&self, store: &MultiAgentStore, project: &str, topic: &str, client: &str) -> Result<()> {
        let owners = self.active.lock().unwrap();
        let connection = store.connection.lock().unwrap();
        let mut round = round_for_request(&connection, project, topic, client)?.context("未找到本次模板的执行回执")?;
        ensure!(matches!(round.status.as_str(), "queued" | "running" | "waiting_approval" | "stopping"), "本次模板已结束或停止状态未确认");
        let owner = owners.get(project).context("本次模板的运行进程尚未确认")?;
        round.status = "stopping".into();
        round.stop_reason = Some("cancelled:用户停止本次模板执行".into());
        round.updated_at = now();
        save_round(&connection, &round)?;
        owner.store(true, Ordering::Release);
        Ok(())
    }
}

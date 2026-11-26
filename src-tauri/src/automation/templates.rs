use super::{is_active_stage, AutomationManager, AutomationRun};
use crate::multi_agent::{AgentTerminalHost, CollaborationBridge, ConversationSendInput, ConversationSupervisor, MultiAgentStore};
use crate::runtime_admission::RuntimeAdmission;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum TemplateId { PrReview, IssueTriage, ReleaseCheck }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TemplateAction {
    template_id: TemplateId,
    pub(crate) project_id: String,
    pub(crate) conversation_id: String,
    pub(crate) agent_id: String,
    pub(crate) body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) creation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TemplateReceipt {
    project_id: String,
    conversation_id: String,
    client_message_id: String,
}

pub(crate) struct TemplateRuntime {
    pub(crate) supervisor: ConversationSupervisor,
    pub(crate) bridge: CollaborationBridge,
    pub(crate) admission: RuntimeAdmission,
}

impl TemplateAction {
    pub(crate) fn validate(&self, store: &MultiAgentStore) -> Result<()> {
        store.validate_template_destination(&self.project_id, &self.conversation_id, &self.agent_id)
    }

    fn input(&self, client: &str) -> ConversationSendInput {
        ConversationSendInput {
            project_id: self.project_id.clone(), expected_conversation_id: Some(self.conversation_id.clone()),
            client_message_id: client.into(), body: self.body.clone(), participant_agent_ids: vec![self.agent_id.clone()],
            attachment_ids: vec![], chat_dispatch: None, mode: "discussion".into(), managed: false,
            integration_owner: None, intent: "chat".into(), max_rounds: 1, max_duration_seconds: 600,
            task_rules: Some(crate::multi_agent::conversation::ConversationTaskRules::default()), native_capabilities: false,
        }
    }
}

impl TemplateReceipt {
    fn evidence(&self, store: &MultiAgentStore) -> Result<Option<(String, String)>> {
        let Some(round) = store.template_round(&self.project_id, &self.conversation_id, &self.client_message_id)? else { return Ok(None); };
        let (stage, detail) = match round.status.as_str() {
            "queued" => ("queued", "模板已进入原话题，等待成员接手"),
            "running" => ("running", "成员正在原话题执行只读检查"),
            "waiting_approval" => ("waiting", "成员正在等待人工处理"),
            "stopping" => ("stopping", "等待本次模板执行停止"),
            "completed" => ("completed", "成员已完成，结果保存在原话题"),
            "failed" | "needs_decision" => ("failed", "本次模板未完成，请查看原话题"),
            "cancelled" | "paused" => ("cancelled", "本次模板执行已停止"),
            _ => ("unconfirmed", "执行状态未确认，请核对原话题"),
        };
        Ok(Some((stage.into(), round.stop_reason.unwrap_or_else(|| detail.into()))))
    }

    pub(super) fn reconcile(&self, store: &MultiAgentStore, run: &mut AutomationRun, now: u64) -> Option<bool> {
        let (stage, detail) = match self.evidence(store) {
            Ok(Some(evidence)) => evidence,
            Ok(None) => ("unconfirmed".into(), "缺少原话题的执行回执，请核对；不会自动重发".into()),
            Err(reason) => ("unconfirmed".into(), format!("读取原话题回执失败：{reason:#}")),
        };
        if run.stage.as_deref() == Some(&stage) && run.detail == detail { return None; }
        run.updated_at = now;
        run.ok = !matches!(stage.as_str(), "failed" | "cancelled");
        let settled = matches!(stage.as_str(), "completed" | "failed").then_some(stage == "completed");
        run.stage = Some(stage); run.detail = detail;
        settled
    }

    pub(super) fn ensure_acknowledgeable(&self, store: &MultiAgentStore) -> Result<()> {
        if let Some((stage, _)) = self.evidence(store)? {
            ensure!(!is_active_stage(&stage), "原模板仍在运行或停止未确认，请先核对原话题");
        }
        Ok(())
    }
}

impl AutomationManager {
    pub(crate) fn dispatch_template(&self, action: &TemplateAction, run_id: &str, store: &MultiAgentStore, host: AgentTerminalHost) -> Result<()> {
        let runtime = self.template_runtime.get().context("话题执行服务未连接")?;
        let _admission = runtime.admission.admit()?;
        let input = self.reserve_template(action, run_id)?;
        runtime.supervisor.send_template(store.clone(), input, host, runtime.bridge.clone(), true)
    }

    pub(super) fn reserve_template(&self, action: &TemplateAction, run_id: &str) -> Result<ConversationSendInput> {
        let input = action.input(&format!("template-{run_id}"));
        let receipt = TemplateReceipt { project_id: action.project_id.clone(), conversation_id: action.conversation_id.clone(),
            client_message_id: input.client_message_id.clone() };
        {
            let mut items = self.items.lock().unwrap();
            let mut next = items.clone();
            let run = next.iter_mut().flat_map(|a| a.recent_runs.iter_mut()).find(|r| r.id == run_id).context("执行记录不存在")?;
            ensure!(run.stage.as_deref() == Some("dispatching") && run.topic.is_none(), "本次计划已不再允许派发");
            run.topic = Some(receipt);
            self.persist(&next)?;
            *items = next;
        }
        Ok(input)
    }

    pub(crate) fn stop_template(&self, id: &str, run_id: &str, store: &MultiAgentStore) -> Result<bool> {
        let items = self.list();
        let Some(run) = items.iter().find(|a| a.id == id).and_then(|a| a.recent_runs.iter().find(|r| r.id == run_id)) else { return Ok(false); };
        let Some(topic) = &run.topic else { return Ok(false); };
        let runtime = self.template_runtime.get().context("话题执行服务未连接")?;
        ensure!(is_active_stage(run.stage.as_deref().unwrap_or("unconfirmed")), "本次模板已结束");
        runtime.supervisor.stop_template_round(store, &topic.project_id, &topic.conversation_id, &topic.client_message_id)?;
        self.mark_stopping(id, run_id)?;
        Ok(true)
    }
}

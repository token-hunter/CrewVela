//! Conversation permissions are a persisted per-request ceiling, independent of a member's role.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationTaskRules {
    pub tools: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub archived_rule_id: Option<String>,
}
impl Default for ConversationTaskRules {
    fn default() -> Self { Self { tools: "research".into(), instructions: String::new(), archived_rule_id: None } }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationHardBoundary {
    pub tools: String,
    pub network: String,
    pub denied_read_paths: Vec<String>,
}
impl Default for ConversationHardBoundary {
    fn default() -> Self { Self { tools: "research".into(), network: "public_web".into(), denied_read_paths: Vec::new() } }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPolicyEntry {
    pub project_id: String,
    pub agent_id: Option<String>,
    pub boundary: ConversationHardBoundary,
    pub legacy_review_required: bool,
    pub legacy_text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRuntimeCapability {
    pub runtime_kind: AgentRuntimeKind,
    pub research: bool,
    pub reply_only: bool,
    pub network_control: bool,
    pub path_control: bool,
    pub status: String,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationArchivedRule {
    pub id: String,
    pub agent_id: Option<String>,
    pub label: String,
    pub instructions: String,
    pub tools: String,
    pub created_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationPolicySetInput {
    pub project_id: String,
    pub agent_id: Option<String>,
    pub boundary: ConversationHardBoundary,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationMigrationPreviewInput {
    pub project_id: String,
    pub agent_id: Option<String>,
    pub replacement_text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMigrationPreview {
    pub id: String,
    pub project_id: String,
    pub agent_id: Option<String>,
    pub previous_text: String,
    pub replacement_text: String,
    pub archived_rules: String,
    pub created_at: i64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationMigrationApplyInput {
    pub project_id: String,
    pub preview_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EffectiveConversationPolicy {
    pub version: u32,
    pub boundary: ConversationHardBoundary,
    pub task_instructions: String,
    pub restriction_reasons: Vec<String>,
}
impl EffectiveConversationPolicy {
    // Old queued rounds must never gain new tools after an application update.
    pub(crate) fn legacy() -> Self {
        Self { version: 1, boundary: ConversationHardBoundary { tools: "none".into(), network: "none".into(), denied_read_paths: Vec::new() }, task_instructions: String::new(), restriction_reasons: vec!["升级前请求保留原无工具策略".into()] }
    }
    pub(crate) fn allows_tools(&self) -> bool { self.boundary.tools == "research" }
    pub(crate) fn uses_native_capabilities(&self) -> bool { self.version == 2 }
}

pub(super) fn migrate_policy(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS ma_conversation_policies (
        project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
        scope_id TEXT NOT NULL, data_json TEXT NOT NULL, PRIMARY KEY(project_id,scope_id));
      CREATE TABLE IF NOT EXISTS ma_conversation_rule_migrations (
        id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
        data_json TEXT NOT NULL, applied INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS ma_conversation_archived_rules (
        id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
        data_json TEXT NOT NULL);")?;
    Ok(())
}

fn policy_text(connection: &Connection, project: &str, agent: Option<&str>) -> Result<String> {
    Ok(match agent {
        Some(agent) => connection.query_row("SELECT description FROM ma_agents WHERE project_id=?1 AND id=?2", params![project, agent], |r| r.get(0))?,
        None => connection.query_row("SELECT goal FROM ma_projects WHERE id=?1", [project], |r| r.get(0))?,
    })
}

/// These markers only add a conservative migration hold. They never grant permission or
/// interpret a role as a permission policy. The complete original text stays visible/persisted.
fn needs_legacy_review(text: &str) -> bool {
    explicit_no_tools(text) || legacy_reply_only(text) || legacy_no_network(text) || legacy_read_restriction(text)
}

fn legacy_reply_only(text: &str) -> bool {
    ["只回复验收标记", "仅回复验收标记"].iter().any(|needle| text.contains(needle))
}

fn legacy_no_network(text: &str) -> bool {
    let text = text.to_lowercase();
    ["禁止联网", "不联网", "禁止访问网络", "不访问网络", "不能联网", "不得联网", "no network", "no internet", "do not access the internet"].iter().any(|needle| text.contains(needle))
}
fn legacy_read_restriction(text: &str) -> bool {
    let text = text.to_lowercase();
    ["禁止读取", "不得读取", "不要读取", "不允许读取", "不能读取", "不读文件", "不读写文件", "不能访问目录", "禁止访问目录", "do not read", "must not read"].iter().any(|needle| text.contains(needle))
}
fn apply_legacy_ceiling(policy: &mut EffectiveConversationPolicy, text: &str, label: &str) {
    if explicit_no_tools(text) || legacy_reply_only(text) {
        policy.boundary.tools = "none".into();
        policy.restriction_reasons.push(format!("{label}明确禁止工具，原规则在迁移前继续有效"));
    }
    if legacy_no_network(text) {
        policy.boundary.network = "none".into();
        policy.restriction_reasons.push(format!("{label}明确禁止联网；项目读取仍按独立边界开放"));
    }
    if legacy_read_restriction(text) {
        policy.boundary.tools = "none".into();
        policy.restriction_reasons.push(format!("{label}含尚未结构化的读取限制，需迁移审阅后才能安全开放研究工具"));
    }
}

pub(super) fn explicit_no_tools(text: &str) -> bool {
    let text = text.to_lowercase();
    ["不调用工具", "不调用任何工具", "不要调用工具", "不使用工具", "不用工具", "禁止调用工具", "禁用工具", "do not use tools", "don't use tools", "no tools"].iter().any(|needle| text.contains(needle))
}

fn validate_boundary(boundary: &mut ConversationHardBoundary) -> Result<()> {
    if !matches!(boundary.tools.as_str(), "research" | "none") || !matches!(boundary.network.as_str(), "public_web" | "none") { bail!("无效的群聊长期工具限制"); }
    if boundary.denied_read_paths.len() > 64 { bail!("最多设置 64 个禁止读取路径"); }
    for path in &mut boundary.denied_read_paths {
        *path = path.trim().replace('\\', "/");
        if path.is_empty() || path.len() > 1024 || path.contains(['\0', '*', '?']) || path.split('/').any(|p| p == "..") {
            bail!("禁止读取路径必须是明确的相对或绝对路径，不能包含通配符或路径穿越");
        }
        let parsed = std::path::Path::new(path);
        if parsed.components().any(|component| matches!(component, std::path::Component::Normal(name) if name.to_string_lossy().contains(':')))
            || (parsed.has_root() && !parsed.is_absolute()) || (!parsed.is_absolute() && path.contains(':')) {
            bail!("禁止读取路径不能使用盘符相对路径或NTFS备用数据流");
        }
    }
    boundary.denied_read_paths.sort();
    boundary.denied_read_paths.dedup();
    Ok(())
}

pub(super) fn policy_entry(connection: &Connection, project: &str, agent: Option<&str>) -> Result<ConversationPolicyEntry> {
    let text = policy_text(connection, project, agent)?;
    let boundary = connection.query_row("SELECT data_json FROM ma_conversation_policies WHERE project_id=?1 AND scope_id=?2", params![project, agent.unwrap_or("")], |r| r.get::<_, String>(0)).optional()?.map(decode).transpose()?.unwrap_or_default();
    Ok(ConversationPolicyEntry { project_id: project.into(), agent_id: agent.map(str::to_string), boundary, legacy_review_required: needs_legacy_review(&text), legacy_text: text })
}

pub(super) fn member_policies(connection: &Connection, project: &str) -> Result<Vec<ConversationPolicyEntry>> {
    let mut statement = connection.prepare("SELECT id FROM ma_agents WHERE project_id=?1 ORDER BY created_at,id")?;
    let agents = statement.query_map([project], |r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    agents.iter().map(|agent| policy_entry(connection, project, Some(agent))).collect()
}
pub(super) fn archived_rules(connection: &Connection, project: &str) -> Result<Vec<ConversationArchivedRule>> {
    rows(connection, "SELECT data_json FROM ma_conversation_archived_rules WHERE project_id=?1 ORDER BY rowid", project)
}

pub(super) fn runtime_capabilities(legacy_desktop: bool) -> Vec<ConversationRuntimeCapability> {
    let mut runtimes = discussion_runtimes();
    if legacy_desktop { runtimes.push(AgentRuntimeKind::ZCode); }
    runtimes.into_iter().map(|runtime_kind| ConversationRuntimeCapability {
        runtime_kind, research: true, reply_only: true, network_control: false, path_control: false,
        status: "native".into(), detail: if runtime_kind == AgentRuntimeKind::ZCode { "已有 ZCode Desktop 成员保持原配置兼容；新增成员请选择 CLI，不自动替换账号或模型。" } else { "保留 CLI 原生工具、联网、Skill、MCP 与权限确认方式；角色设定仅用于专长和分工。实际可用性由当前 CLI 安装、登录及自身配置决定。" }.into(),
    }).collect()
}

fn intersect(boundary: &mut ConversationHardBoundary, other: &ConversationHardBoundary) {
    if other.tools == "none" { boundary.tools = "none".into(); }
    if other.network == "none" { boundary.network = "none".into(); }
    boundary.denied_read_paths.extend(other.denied_read_paths.iter().cloned());
    boundary.denied_read_paths.sort(); boundary.denied_read_paths.dedup();
}

pub(super) fn resolve_round_policies(connection: &Connection, input: &ConversationSendInput) -> Result<HashMap<String, EffectiveConversationPolicy>> {
    let mut rules = input.task_rules.clone().unwrap_or_default();
    if !matches!(rules.tools.as_str(), "research" | "none") || rules.instructions.chars().count() > MAX_MESSAGE { bail!("无效的本次任务规则"); }
    let archived = rules.archived_rule_id.as_ref().map(|rule_id| {
        let archived = archived_rules(connection, &input.project_id)?.into_iter().find(|rule| &rule.id == rule_id).ok_or_else(|| anyhow!("原规则归档不存在或不属于当前项目"))?;
        if archived.agent_id.as_ref().is_some_and(|agent| !input.participant_agent_ids.contains(agent)) { bail!("归档规则所属成员未参与本轮，未应用其他成员的规则"); }
        Ok::<_,anyhow::Error>(archived)
    }).transpose()?;
    if input.native_capabilities {
        return Ok(input.participant_agent_ids.iter().map(|agent| {
            let mut instructions = rules.instructions.clone();
            if rules.tools == "none" { instructions.push_str("\n用户要求本次不使用工具，只回复本次消息。"); }
            if let Some(rule) = archived.as_ref().filter(|rule| rule.agent_id.as_ref().is_none_or(|owner| owner == agent)) {
                instructions = format!("{}\n{}", rule.instructions, instructions);
            }
            (agent.clone(), EffectiveConversationPolicy { version: 2, boundary: Default::default(), task_instructions: instructions, restriction_reasons: Vec::new() })
        }).collect());
    }
    if explicit_no_tools(&input.body) || explicit_no_tools(&rules.instructions) { rules.tools = "none".into(); }
    let project = policy_entry(connection, &input.project_id, None)?;
    let mut result = HashMap::new();
    for agent_id in &input.participant_agent_ids {
        let member = policy_entry(connection, &input.project_id, Some(agent_id))?;
        let mut policy = EffectiveConversationPolicy { version: 1, boundary: project.boundary.clone(), task_instructions: rules.instructions.clone(), restriction_reasons: Vec::new() };
        intersect(&mut policy.boundary, &member.boundary);
        for text in [&input.body, &rules.instructions] {
            if legacy_no_network(text) { policy.boundary.network = "none".into(); policy.restriction_reasons.push("本次消息禁止联网，后续消息重新计算".into()); }
            if legacy_read_restriction(text) { policy.boundary.tools = "none".into(); policy.restriction_reasons.push("本次消息含尚未结构化的读取限制，已拒绝研究工具；可用独立路径限制明确范围".into()); }
        }
        if rules.tools == "none" { policy.boundary.tools = "none".into(); policy.restriction_reasons.push("本次消息禁止工具，后续消息重新计算".into()); }
        if let Some(archived) = archived.as_ref().filter(|rule| rule.agent_id.as_ref().is_none_or(|owner| owner == agent_id)) {
            if archived.tools == "none" { policy.boundary.tools = "none".into(); }
            apply_legacy_ceiling(&mut policy, &archived.instructions, "本次选用的归档规则");
            policy.task_instructions = format!("{}\n{}", archived.instructions, policy.task_instructions);
            policy.restriction_reasons.push("本次选用了迁移前规则归档，仅在本次生效".into());
        }
        for (label, entry) in [("项目目标", &project), ("成员职责", &member)] {
            apply_legacy_ceiling(&mut policy, &entry.legacy_text, label);
        }
        result.insert(agent_id.clone(), policy);
    }
    Ok(result)
}

impl MultiAgentStore {
    pub fn conversation_policy_set(&self, mut input: ConversationPolicySetInput) -> Result<ConversationSnapshot> {
        validate_boundary(&mut input.boundary)?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        policy_text(&tx, &input.project_id, input.agent_id.as_deref())?;
        tx.execute("INSERT INTO ma_conversation_policies(project_id,scope_id,data_json) VALUES(?1,?2,?3) ON CONFLICT(project_id,scope_id) DO UPDATE SET data_json=excluded.data_json", params![input.project_id, input.agent_id.as_deref().unwrap_or(""), serde_json::to_string(&input.boundary)?])?;
        let snapshot = snapshot_with(&tx, &input.project_id)?;
        tx.commit()?; Ok(snapshot)
    }

    pub fn conversation_migration_preview(&self, input: ConversationMigrationPreviewInput) -> Result<ConversationMigrationPreview> {
        if input.replacement_text.chars().count() > MAX_MESSAGE { bail!("新职责或项目目标过长"); }
        let connection = self.connection.lock().unwrap();
        let previous_text = policy_text(&connection, &input.project_id, input.agent_id.as_deref())?;
        if previous_text == input.replacement_text { bail!("新文本与原文本相同，无需迁移"); }
        let preview = ConversationMigrationPreview { id: id(), project_id: input.project_id, agent_id: input.agent_id, archived_rules: previous_text.clone(), previous_text, replacement_text: input.replacement_text, created_at: now() };
        connection.execute("INSERT INTO ma_conversation_rule_migrations(id,project_id,data_json) VALUES(?1,?2,?3)", params![preview.id, preview.project_id, serde_json::to_string(&preview)?])?;
        Ok(preview)
    }

    pub fn conversation_migration_apply(&self, input: ConversationMigrationApplyInput) -> Result<ConversationSnapshot> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (raw, applied): (String, bool) = tx.query_row("SELECT data_json,applied FROM ma_conversation_rule_migrations WHERE id=?1 AND project_id=?2", params![input.preview_id, input.project_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let preview: ConversationMigrationPreview = decode(raw)?;
        if applied { return snapshot_with(&tx, &input.project_id); }
        let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_conversation_rounds WHERE project_id=?1 AND status IN ('queued','running','waiting_approval','stopping','unconfirmed'))", [&input.project_id], |r| r.get(0))?;
        if active { bail!("当前讨论尚未结束或停止未确认，请结束后应用迁移"); }
        if policy_text(&tx, &input.project_id, preview.agent_id.as_deref())? != preview.previous_text { bail!("原规则已变化，请重新预览后确认"); }
        match &preview.agent_id {
            Some(agent) => { tx.execute("UPDATE ma_agents SET description=?3,updated_at=?4 WHERE project_id=?1 AND id=?2", params![input.project_id, agent, preview.replacement_text, now()])?; },
            None => { tx.execute("UPDATE ma_projects SET goal=?2,updated_at=?3 WHERE id=?1", params![input.project_id, preview.replacement_text, now()])?; },
        }
        let archived_tools = if explicit_no_tools(&preview.previous_text) || legacy_reply_only(&preview.previous_text) || legacy_read_restriction(&preview.previous_text) { "none" } else { "research" };
        let archived = ConversationArchivedRule { id: preview.id.clone(), agent_id: preview.agent_id, label: format!("迁移前{}规则", if preview.previous_text.contains("验收") { "验收" } else { "任务" }), instructions: preview.previous_text, tools: archived_tools.into(), created_at: now() };
        tx.execute("INSERT INTO ma_conversation_archived_rules(id,project_id,data_json) VALUES(?1,?2,?3)", params![archived.id, input.project_id, serde_json::to_string(&archived)?])?;
        tx.execute("UPDATE ma_conversation_rule_migrations SET applied=1 WHERE id=?1", [&preview.id])?;
        let snapshot = snapshot_with(&tx, &input.project_id)?;
        tx.commit()?; Ok(snapshot)
    }

    pub(crate) fn conversation_task_policy(&self, task: &str, agent: &str) -> Result<EffectiveConversationPolicy> {
        let connection = self.connection.lock().unwrap();
        let raw: String = connection.query_row("SELECT r.data_json FROM ma_conversation_rounds r JOIN ma_conversation_turns t ON t.round_id=r.id WHERE t.task_id=?1", [task], |r| r.get(0))?;
        let round: ConversationRound = decode(raw)?;
        if !round.participant_agent_ids.iter().any(|id| id == agent) { bail!("研究工具身份不属于当前请求"); }
        let mut policy = round.member_policies.get(agent).cloned().unwrap_or_else(EffectiveConversationPolicy::legacy);
        if policy.uses_native_capabilities() { return Ok(policy); }
        if policy.version != 1 { bail!("不支持的群聊策略版本，拒绝扩大权限"); }
        // A boundary tightened while a request is running takes effect immediately. Relaxing
        // settings never widens the immutable ceiling that was accepted with that request.
        for entry in [policy_entry(&connection, &round.project_id, None)?, policy_entry(&connection, &round.project_id, Some(agent))?] {
            intersect(&mut policy.boundary, &entry.boundary);
            apply_legacy_ceiling(&mut policy, &entry.legacy_text, "当前配置");
        }
        Ok(policy)
    }
}

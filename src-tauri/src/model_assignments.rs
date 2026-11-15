//! Typed runtime defaults plus existing member/session model fields. No credentials.
use crate::{
    multi_agent::{AgentProfile, AgentRuntimeKind, MultiAgentStore},
    providers::ProviderConfig,
    sessions::{SessionManager, SessionRecord, SessionStatus},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

static DEFAULT_WRITES: Mutex<()> = Mutex::new(());
pub const RUNTIMES: &[(&str, &str)] = &[
    ("claude_code", "Claude Code"),
    ("codex", "Codex"),
    ("kimi_code", "Kimi Code"),
    ("pi", "Pi"),
    ("opencode", "OpenCode"),
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub connection_id: Option<String>,
    pub model: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSelection {
    pub connection_id: String,
    pub model: Option<String>,
    pub source: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Defaults {
    #[serde(default)]
    runtimes: BTreeMap<String, Selection>,
}

fn config_path() -> Result<PathBuf> {
    #[cfg(test)]
    {
        static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        return Ok(PATH
            .get_or_init(|| {
                std::env::temp_dir()
                    .join(format!("crewvela-model-test-{}.json", uuid::Uuid::new_v4()))
            })
            .clone());
    }
    #[cfg(not(test))]
    {
        Ok(crate::providers::config_dir()?.join("runtime-model-defaults.json"))
    }
}
fn read_at(path: &Path) -> Result<Defaults> {
    if !path.exists() {
        return Ok(Defaults::default());
    }
    serde_json::from_slice(&std::fs::read(path).context("读取 Agent 默认模型失败")?)
        .context("Agent 默认模型配置损坏，未自动覆盖")
}
fn default_for(data: &Defaults, runtime: &str) -> Selection {
    data.runtimes.get(runtime).cloned().unwrap_or_default()
}
fn token(value: &impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("model metadata serialization"))
    )
}
pub fn kind(value: &str) -> Result<AgentRuntimeKind> {
    let kind: AgentRuntimeKind = serde_json::from_value(serde_json::json!(value))?;
    if kind.is_retired() {
        bail!("该 Agent 已移除或不受支持");
    }
    Ok(kind)
}
fn editable(runtime: &str) -> bool {
    RUNTIMES.iter().any(|(id, _)| *id == runtime)
}
fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
fn validate(runtime: &str, selection: &Selection, providers: &[ProviderConfig]) -> Result<()> {
    if !editable(runtime) {
        bail!("此 Agent 的统一模型写入尚未适配，请在原会话或成员设置中配置");
    }
    if let Some(model) = &selection.model {
        if model.len() > 200
            || model.starts_with('-')
            || !model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/@+[]".contains(c))
        {
            bail!("模型 ID 无效，请填写 CLI 支持的模型标识");
        }
    }
    if let Some(id) = selection
        .connection_id
        .as_deref()
        .filter(|id| *id != "local")
    {
        if runtime != "claude_code" {
            bail!("此 Agent 使用本机账号/CLI 配置，不能使用 Claude 服务连接");
        }
        let provider = providers
            .iter()
            .find(|provider| provider.id == id)
            .context("所选模型连接已不存在")?;
        crate::launcher::validate_provider(provider).map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

pub(crate) fn find_claude_provider<'a>(
    hint: &str,
    providers: &'a [ProviderConfig],
) -> Option<&'a ProviderConfig> {
    providers.iter().find(|provider| {
        provider.id.eq_ignore_ascii_case(hint.trim())
            || provider.name.eq_ignore_ascii_case(hint.trim())
    })
}
fn resolve(
    default: &Selection,
    member_model: Option<&str>,
    connection: Option<&Option<String>>,
    providers: &[ProviderConfig],
    runtime: &str,
) -> ResolvedSelection {
    // Historical Claude model strings sometimes named a Provider; new typed rows
    // explicitly distinguish that connection from a model whose ID happens to match.
    if runtime == "claude_code" && connection.is_none() {
        if let Some(provider) = member_model.and_then(|hint| find_claude_provider(hint, providers))
        {
            return ResolvedSelection {
                connection_id: provider.id.clone(),
                model: provider.env.get("ANTHROPIC_MODEL").cloned(),
                source: "legacy_connection".into(),
            };
        }
    }
    let connection_id = connection
        .and_then(|value| value.as_ref())
        .or(default.connection_id.as_ref())
        .cloned()
        .unwrap_or_else(|| "local".into());
    let model = normalized(member_model)
        .or_else(|| default.model.clone())
        .or_else(|| {
            providers
                .iter()
                .find(|provider| provider.id == connection_id)
                .and_then(|provider| provider.env.get("ANTHROPIC_MODEL").cloned())
        });
    let overridden = member_model.is_some_and(|model| !model.trim().is_empty())
        || connection.is_some_and(|value| value.is_some());
    ResolvedSelection {
        connection_id,
        model,
        source: if overridden {
            "member"
        } else {
            "runtime_default"
        }
        .into(),
    }
}

pub fn for_agent(
    agent: &AgentProfile,
    store: &MultiAgentStore,
    sessions: &SessionManager,
) -> Result<ResolvedSelection> {
    let mut result = resolve_agent(agent, store)?;
    let (_, bindings) = store.model_assignment_context(&agent.project_id)?;
    if let Some(session) = bindings
        .iter()
        .find(|binding| binding.agent_id == agent.id)
        .and_then(|binding| binding.app_session_id.as_deref())
        .and_then(|id| sessions.get(id))
    {
        if !session.retired_agent_runtime
            && crate::automation_runtime::session_runtime(&session) == Some(agent.runtime_kind)
        {
            apply_session_model(&mut result, &session);
        }
    }
    Ok(result)
}
fn resolve_agent(agent: &AgentProfile, store: &MultiAgentStore) -> Result<ResolvedSelection> {
    let _read = DEFAULT_WRITES
        .lock()
        .map_err(|_| anyhow!("模型默认配置锁异常"))?;
    let defaults = read_at(&config_path()?)?;
    let connection = store.model_connection(&agent.id, agent.runtime_kind)?;
    let providers = if agent.runtime_kind == AgentRuntimeKind::ClaudeCode {
        crate::providers::load()?.providers
    } else {
        Vec::new()
    };
    Ok(resolve(
        &default_for(&defaults, agent.runtime_kind.as_str()),
        agent.model.as_deref(),
        connection.as_ref(),
        &providers,
        agent.runtime_kind.as_str(),
    ))
}
fn apply_session_model(result: &mut ResolvedSelection, session: &SessionRecord) {
    if let Some(model) = normalized(session.launch_model.as_deref()) {
        result.model = Some(model);
        result.source = "session".into();
    }
}
/// Team-message wakeups must use the exact target member and session, just like task runs.
pub fn for_session(session: &SessionRecord, store: &MultiAgentStore) -> Result<ResolvedSelection> {
    let runtime =
        crate::automation_runtime::session_runtime(session).context("会话缺少 Agent 类型")?;
    let mut result = match (
        &session.multi_agent_project_id,
        &session.multi_agent_agent_id,
    ) {
        (Some(project), Some(agent_id)) => {
            let (agents, _) = store.model_assignment_context(project)?;
            let agent = agents
                .iter()
                .find(|agent| {
                    &agent.id == agent_id && agent.enabled && agent.runtime_kind == runtime
                })
                .context("目标成员已停用、删除或 Runtime 已变化")?;
            resolve_agent(agent, store)?
        }
        (None, None) => for_runtime(runtime.as_str(), None)?,
        _ => bail!("会话的项目成员关联不完整"),
    };
    apply_session_model(&mut result, session);
    Ok(result)
}
pub fn for_runtime(runtime: &str, override_model: Option<&str>) -> Result<ResolvedSelection> {
    let _read = DEFAULT_WRITES
        .lock()
        .map_err(|_| anyhow!("模型默认配置锁异常"))?;
    let providers = if runtime == "claude_code" {
        crate::providers::load()?.providers
    } else {
        Vec::new()
    };
    let mut result = resolve(
        &default_for(&read_at(&config_path()?)?, runtime),
        None,
        None,
        &providers,
        runtime,
    );
    if let Some(model) = normalized(override_model) {
        result.model = Some(model);
        result.source = "session".into();
    }
    Ok(result)
}
pub fn configure_connection(
    command: &mut Command,
    runtime: &str,
    selection: &ResolvedSelection,
) -> Result<Option<ProviderConfig>> {
    if selection.connection_id == "local" {
        return Ok(None);
    }
    if runtime != "claude_code" {
        bail!("此 Runtime 不能使用 Claude 服务连接");
    }
    let provider = crate::providers::load()?
        .providers
        .into_iter()
        .find(|provider| provider.id == selection.connection_id)
        .context("已分配的模型连接不存在，请重新配置")?;
    crate::launcher::validate_provider(&provider).map_err(anyhow::Error::msg)?;
    let key = crate::secrets::get(&provider.auth_token_ref)?.context("模型连接尚未配置 API Key")?;
    Ok(Some(apply_provider(
        command,
        provider,
        &key,
        selection.model.as_deref(),
    )))
}
fn apply_provider(
    command: &mut Command,
    mut provider: ProviderConfig,
    key: &str,
    model: Option<&str>,
) -> ProviderConfig {
    if let Some(model) = model {
        provider.env.insert("ANTHROPIC_MODEL".into(), model.into());
    }
    for variable in crate::launcher::CLEAR_VARS {
        command.env_remove(variable);
    }
    command.envs(crate::launcher::build_env(&provider, key));
    provider
}
pub fn configure_model(command: &mut Command, runtime: &str, model: Option<&str>) -> Result<()> {
    if model.is_none() {
        return Ok(());
    }
    validate(
        runtime,
        &Selection {
            model: normalized(model),
            connection_id: None,
        },
        &[],
    )?;
    if let Some(model) = model {
        command.arg("--model").arg(model);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentRow {
    pub id: String,
    pub scope: String,
    pub target_id: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub name: String,
    pub runtime: String,
    pub raw: Selection,
    pub effective: ResolvedSelection,
    pub version: String,
    pub editable: bool,
    pub connection_editable: bool,
    pub note: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub rows: Vec<AssignmentRow>,
    pub projects: Vec<crate::multi_agent::MultiAgentProject>,
}
fn row(
    scope: &str,
    target: &str,
    name: &str,
    runtime: &str,
    raw: Selection,
    effective: ResolvedSelection,
    version: String,
) -> AssignmentRow {
    AssignmentRow {
        id: format!("{scope}:{target}"),
        scope: scope.into(),
        target_id: target.into(),
        project_id: None,
        project_name: None,
        name: name.into(),
        runtime: runtime.into(),
        raw,
        effective,
        version,
        editable: editable(runtime),
        connection_editable: runtime == "claude_code",
        note: String::new(),
    }
}
pub fn snapshot(store: &MultiAgentStore, sessions: &SessionManager) -> Result<Snapshot> {
    let _read = DEFAULT_WRITES
        .lock()
        .map_err(|_| anyhow!("模型默认配置锁异常"))?;
    let defaults = read_at(&config_path()?)?;
    snapshot_with(&defaults, store, sessions)
}
fn snapshot_with(
    defaults: &Defaults,
    store: &MultiAgentStore,
    sessions: &SessionManager,
) -> Result<Snapshot> {
    let providers = crate::providers::load()?.providers;
    let mut rows = Vec::new();
    for (runtime, name) in RUNTIMES {
        let raw = default_for(defaults, runtime);
        rows.push(row(
            "runtime",
            runtime,
            name,
            runtime,
            raw.clone(),
            resolve(&raw, None, None, &providers, runtime),
            token(&raw),
        ));
    }
    let projects = store.list_projects()?;
    for project in &projects {
        for agent in store.model_assignment_context(&project.id)?.0 {
            if agent.runtime_kind.is_retired() {
                continue;
            }
            let connection = store.model_connection(&agent.id, agent.runtime_kind)?;
            let default = default_for(defaults, agent.runtime_kind.as_str());
            let effective = resolve(
                &default,
                agent.model.as_deref(),
                connection.as_ref(),
                &providers,
                agent.runtime_kind.as_str(),
            );
            let raw = Selection {
                connection_id: connection.clone().flatten(),
                model: agent.model.clone(),
            };
            let mut item = row(
                "member",
                &agent.id,
                &agent.name,
                agent.runtime_kind.as_str(),
                raw,
                effective,
                token(&(
                    &agent.model,
                    connection.is_some(),
                    &connection,
                    &default,
                    agent.runtime_kind,
                )),
            );
            item.project_id = Some(project.id.clone());
            item.project_name = Some(project.name.clone());
            if !item.editable {
                item.note = "专用适配器：请在原会话或团队成员设置中配置".into();
            }
            rows.push(item);
        }
    }
    for session in sessions.list().into_iter().filter(|session| {
        session.archived_at.is_none()
            && !session.retired_agent_runtime
            && !session.runtime_command_host
    }) {
        let Some(runtime) = crate::automation_runtime::session_runtime(&session) else {
            continue;
        };
        if runtime.is_retired() {
            continue;
        }
        let effective = session
            .model_assignment
            .clone()
            .unwrap_or_else(|| ResolvedSelection {
                connection_id: if session.kind == crate::sessions::SessionKind::Claude {
                    session.provider_id.clone()
                } else {
                    "local".into()
                },
                model: session.launch_model.clone(),
                source: if session.launch_model.is_some() {
                    "session"
                } else {
                    "unrecorded"
                }
                .into(),
            });
        let raw = Selection {
            connection_id: None,
            model: session.launch_model.clone(),
        };
        let mut item = row(
            "session",
            &session.id,
            &session.title,
            runtime.as_str(),
            raw,
            effective,
            token(&(
                &session.launch_model,
                &session.model_assignment,
                &session.status,
                &session.pty_id,
            )),
        );
        item.project_id = session.multi_agent_project_id.clone();
        item.editable = editable(runtime.as_str())
            && matches!(
                session.status,
                SessionStatus::Stopped | SessionStatus::Error | SessionStatus::Pending
            )
            && session.pty_id.is_none();
        item.connection_editable = false;
        item.note = if item.editable {
            "修改启动覆盖，恢复会话时使用"
        } else {
            "仅显示启动配置；运行中请在原会话切换模型"
        }
        .into();
        rows.push(item);
    }
    Ok(Snapshot { rows, projects })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveInput {
    pub id: String,
    pub version: String,
    pub connection_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub restore: bool,
}
pub fn save(
    input: SaveInput,
    store: &MultiAgentStore,
    sessions: &SessionManager,
) -> Result<Snapshot> {
    save_at_path(input, store, sessions, &config_path()?)
}
fn save_at_path(
    input: SaveInput,
    store: &MultiAgentStore,
    sessions: &SessionManager,
    path: &Path,
) -> Result<Snapshot> {
    let _lifecycle = store.agent_lifecycle_guard();
    // Session launch takes its lifecycle lock before reading defaults too.
    let session_lifecycle = input
        .id
        .strip_prefix("session:")
        .map(|id| sessions.lifecycle_lock(id));
    let _session_guard = session_lifecycle
        .as_ref()
        .map(|lifecycle| lifecycle.lock().unwrap());
    let _write = DEFAULT_WRITES
        .lock()
        .map_err(|_| anyhow!("模型默认配置锁异常"))?;
    let mut defaults = read_at(path)?;
    let before = snapshot_with(&defaults, store, sessions)?;
    let selected = before
        .rows
        .iter()
        .find(|row| row.id == input.id)
        .context("配置对象已不存在")?;
    if selected.version != input.version {
        bail!("配置已被其他操作修改，编辑内容已保留，请重新读取后重试");
    }
    if !selected.editable {
        bail!("{}", selected.note);
    }
    let selection = if input.restore {
        Selection::default()
    } else {
        Selection {
            connection_id: normalized(input.connection_id.as_deref()),
            model: normalized(input.model.as_deref()),
        }
    };
    let providers = crate::providers::load()?.providers;
    validate(&selected.runtime, &selection, &providers)?;
    match selected.scope.as_str() {
        "runtime" => {
            if selection == Selection::default() {
                defaults.runtimes.remove(&selected.runtime);
            } else {
                defaults
                    .runtimes
                    .insert(selected.runtime.clone(), selection);
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            crate::providers::write_atomic(path, &serde_json::to_vec_pretty(&defaults)?)?;
        }
        "member" => {
            crate::ensure_member_terminal_idle(sessions, &selected.target_id)
                .map_err(anyhow::Error::msg)?;
            let runtime = kind(&selected.runtime)?;
            let connection = store.model_connection(&selected.target_id, runtime)?;
            store.update_model_assignment(
                selected.project_id.as_deref().context("成员项目缺失")?,
                &selected.target_id,
                runtime,
                selected.raw.model.as_deref(),
                connection,
                selection.model.as_deref(),
                selection.connection_id.as_deref(),
            )?;
        }
        "session" => {
            if selection.connection_id.is_some() {
                bail!("会话连接不能在此更换");
            }
            let latest = sessions.get(&selected.target_id).context("会话已删除")?;
            if latest.pty_id.is_some()
                || !matches!(
                    latest.status,
                    SessionStatus::Stopped | SessionStatus::Error | SessionStatus::Pending
                )
            {
                bail!("会话状态已变化，请停止会话后再编辑启动覆盖");
            }
            sessions.set_launch_model(&selected.target_id, selection.model)?;
        }
        _ => bail!("未知配置范围"),
    }
    snapshot_with(&defaults, store, sessions)
        .context("模型配置已写入，但刷新分配表失败；请重新读取，勿重复提交")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn provider() -> ProviderConfig {
        serde_json::from_value(serde_json::json!({"id":"glm","name":"智谱服务","baseUrl":"https://fixture.invalid/anthropic","authTokenRef":"fixture","env":{"ANTHROPIC_MODEL":"glm-default"},"builtin":false,"disableNonessentialTraffic":false,"timeoutMs":null})).unwrap()
    }
    #[test]
    fn independent_connection_and_model_preserve_legacy_selection() {
        let providers = vec![provider()];
        let default = Selection {
            connection_id: Some("glm".into()),
            model: Some("default-model".into()),
        };
        let inherited = resolve(&default, None, None, &providers, "claude_code");
        assert_eq!(inherited.model.as_deref(), Some("default-model"));
        assert_eq!(inherited.source, "runtime_default");
        let member = resolve(
            &default,
            Some("member-model"),
            Some(&Some("local".into())),
            &providers,
            "claude_code",
        );
        assert_eq!(member.connection_id, "local");
        assert_eq!(member.model.as_deref(), Some("member-model"));
        assert_eq!(member.source, "member");
        let legacy = resolve(
            &Selection::default(),
            Some("智谱服务"),
            None,
            &providers,
            "claude_code",
        );
        assert_eq!(legacy.connection_id, "glm");
        assert_eq!(legacy.model.as_deref(), Some("glm-default"));
        assert_eq!(legacy.source, "legacy_connection");
        let literal = resolve(
            &Selection::default(),
            Some("glm"),
            Some(&None),
            &providers,
            "claude_code",
        );
        assert_eq!(literal.connection_id, "local");
        assert_eq!(literal.model.as_deref(), Some("glm"));
        let fallback = resolve(
            &Selection {
                connection_id: Some("glm".into()),
                model: None,
            },
            None,
            None,
            &providers,
            "claude_code",
        );
        assert_eq!(fallback.model.as_deref(), Some("glm-default"));
    }
    #[test]
    fn model_argument_contracts_do_not_convert_other_runtime_connections() {
        let mut local = Command::new("fixture");
        configure_model(&mut local, "zcode_cli", None).unwrap();
        assert_eq!(local.get_args().count(), 0);
        for (runtime, _) in RUNTIMES {
            let mut command = Command::new("not-executed-fixture");
            configure_model(&mut command, runtime, Some("provider/model:variant[1m]")).unwrap();
            assert_eq!(
                command
                    .get_args()
                    .map(|value| value.to_str().unwrap())
                    .collect::<Vec<_>>(),
                ["--model", "provider/model:variant[1m]"]
            );
            for bad in ["--flag", "model with spaces", "a;command", "a\nnext", "a&b"] {
                assert!(configure_model(&mut command, runtime, Some(bad)).is_err());
            }
        }
        assert!(validate(
            "codex",
            &Selection {
                connection_id: Some("glm".into()),
                model: None
            },
            &[provider()]
        )
        .is_err());
        assert!(validate(
            "claude_code",
            &Selection {
                connection_id: Some("missing".into()),
                model: None
            },
            &[provider()]
        )
        .is_err());
        assert!(validate("zcode", &Selection::default(), &[]).is_err());
    }
    #[test]
    fn provider_override_reaches_both_command_environment_and_returned_settings() {
        let original = provider();
        let mut command = Command::new("fixture");
        let effective = apply_provider(
            &mut command,
            original.clone(),
            "fixture-only-key",
            Some("member-model"),
        );
        let environment = command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            effective.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("member-model")
        );
        assert_eq!(
            environment.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("member-model")
        );
        assert_eq!(
            original.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("glm-default")
        );
        assert_eq!(effective.id, original.id);
    }
    struct Fixture {
        root: PathBuf,
        path: PathBuf,
        store: MultiAgentStore,
        sessions: SessionManager,
        project: String,
        agent: String,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "crewvela-model-assignment-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let store = MultiAgentStore::open_at(root.join("agents.db")).unwrap();
            let project = store.create_project(serde_json::from_value(serde_json::json!({"name":"model fixture","rootPath":root,"goal":"isolated test"})).unwrap()).unwrap().id;
            let agent = store.create_agent(serde_json::from_value(serde_json::json!({"projectId":project,"name":"worker","runtimeKind":"codex","role":"worker","model":null})).unwrap()).unwrap().id;
            let sessions =
                SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
            Self {
                path: root.join("defaults.json"),
                root,
                store,
                sessions,
                project,
                agent,
            }
        }
        fn snapshot(&self) -> Snapshot {
            snapshot_with(&read_at(&self.path).unwrap(), &self.store, &self.sessions).unwrap()
        }
        fn save(&self, id: &str, model: Option<&str>, restore: bool) -> Result<Snapshot> {
            let selected = self
                .snapshot()
                .rows
                .into_iter()
                .find(|row| row.id == id)
                .unwrap();
            save_at_path(
                SaveInput {
                    id: id.into(),
                    version: selected.version,
                    connection_id: None,
                    model: model.map(str::to_owned),
                    restore,
                },
                &self.store,
                &self.sessions,
                &self.path,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let checked = self.root.canonicalize().unwrap();
            assert!(checked.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            // SQLite handles are still open on Windows; clean after dropping the store explicitly in test process cleanup.
            let _ = std::fs::remove_file(&self.path);
        }
    }
    #[test]
    fn defaults_and_member_assignment_persist_and_restore_without_changing_profile() {
        let fixture = Fixture::new();
        fixture
            .save("runtime:codex", Some("runtime-model"), false)
            .unwrap();
        assert_eq!(
            read_at(&fixture.path).unwrap().runtimes["codex"]
                .model
                .as_deref(),
            Some("runtime-model")
        );
        let id = format!("member:{}", fixture.agent);
        let inherited = fixture
            .snapshot()
            .rows
            .into_iter()
            .find(|row| row.id == id)
            .unwrap();
        assert_eq!(inherited.effective.model.as_deref(), Some("runtime-model"));
        let before = fixture.store.agent_profile(&fixture.agent).unwrap();
        fixture.save(&id, Some("member-model"), false).unwrap();
        let after = fixture.store.agent_profile(&fixture.agent).unwrap();
        assert_eq!(after.name, before.name);
        assert_eq!(after.description, before.description);
        assert_eq!(after.runtime_kind, before.runtime_kind);
        assert_eq!(after.model.as_deref(), Some("member-model"));
        assert_eq!(
            fixture
                .store
                .model_connection(&fixture.agent, AgentRuntimeKind::Codex)
                .unwrap(),
            Some(None)
        );
        let reopened = MultiAgentStore::open_at(fixture.root.join("agents.db")).unwrap();
        assert_eq!(
            reopened
                .agent_profile(&fixture.agent)
                .unwrap()
                .model
                .as_deref(),
            Some("member-model")
        );
        fixture.save(&id, None, true).unwrap();
        assert!(fixture
            .store
            .agent_profile(&fixture.agent)
            .unwrap()
            .model
            .is_none());
        assert!(fixture
            .store
            .model_connection(&fixture.agent, AgentRuntimeKind::Codex)
            .unwrap()
            .is_none());
        fixture.save("runtime:codex", None, true).unwrap();
        assert!(read_at(&fixture.path).unwrap().runtimes.is_empty());
    }
    #[test]
    fn stale_member_edits_and_damaged_default_file_are_not_overwritten() {
        let fixture = Fixture::new();
        let id = format!("member:{}", fixture.agent);
        let stale = fixture
            .snapshot()
            .rows
            .into_iter()
            .find(|row| row.id == id)
            .unwrap();
        fixture.save(&id, Some("new-model"), false).unwrap();
        assert!(save_at_path(
            SaveInput {
                id,
                version: stale.version,
                connection_id: None,
                model: Some("stale-model".into()),
                restore: false
            },
            &fixture.store,
            &fixture.sessions,
            &fixture.path
        )
        .is_err());
        assert_eq!(
            fixture
                .store
                .agent_profile(&fixture.agent)
                .unwrap()
                .model
                .as_deref(),
            Some("new-model")
        );
        std::fs::write(&fixture.path, "damaged-json").unwrap();
        assert!(read_at(&fixture.path).is_err());
        assert_eq!(
            std::fs::read_to_string(&fixture.path).unwrap(),
            "damaged-json"
        );
    }
    #[test]
    fn session_override_is_saved_only_when_idle_and_keeps_other_sessions() {
        let fixture = Fixture::new();
        let session = fixture
            .sessions
            .create_runtime(crate::sessions::NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: fixture.root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        let other = fixture
            .sessions
            .create_runtime(crate::sessions::NewRuntimeSession {
                runtime_kind: "codex".into(),
                cwd: fixture.root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        let id = format!("session:{}", session.id);
        fixture.save(&id, Some("session-model"), false).unwrap();
        assert_eq!(
            fixture
                .sessions
                .get(&session.id)
                .unwrap()
                .launch_model
                .as_deref(),
            Some("session-model")
        );
        assert!(fixture
            .sessions
            .get(&other.id)
            .unwrap()
            .launch_model
            .is_none());
        fixture.sessions.attach_runtime(&session.id, 999).unwrap();
        assert!(fixture
            .save(&id, Some("unsafe-live-change"), false)
            .is_err());
        assert_eq!(
            fixture
                .sessions
                .get(&session.id)
                .unwrap()
                .launch_model
                .as_deref(),
            Some("session-model")
        );
    }
    #[test]
    fn bound_session_override_wins_over_member_without_changing_other_members() {
        let fixture = Fixture::new();
        fixture
            .save(
                &format!("member:{}", fixture.agent),
                Some("member-model"),
                false,
            )
            .unwrap();
        let session = fixture
            .sessions
            .create_agent(crate::sessions::NewAgentSession {
                avatar_id: None,
                project_id: fixture.project.clone(),
                agent_id: fixture.agent.clone(),
                runtime_kind: "codex".into(),
                agent_name: "worker".into(),
                cwd: fixture.root.display().to_string(),
            })
            .unwrap();
        fixture
            .store
            .link_agent_app_session(&fixture.project, &fixture.agent, &session.id)
            .unwrap();
        fixture
            .sessions
            .set_launch_model(&session.id, Some("session-model".into()))
            .unwrap();
        let agent = fixture.store.agent_profile(&fixture.agent).unwrap();
        let effective = for_agent(&agent, &fixture.store, &fixture.sessions).unwrap();
        assert_eq!(effective.model.as_deref(), Some("session-model"));
        assert_eq!(effective.source, "session");
        fixture
            .sessions
            .set_launch_model(&session.id, None)
            .unwrap();
        let inherited = for_agent(&agent, &fixture.store, &fixture.sessions).unwrap();
        assert_eq!(inherited.model.as_deref(), Some("member-model"));
        assert_eq!(
            fixture.store.agent_profile(&fixture.agent).unwrap().model,
            agent.model
        );
    }
    #[test]
    fn direct_wakeup_uses_exact_member_and_target_session() {
        let fixture = Fixture::new();
        fixture
            .save(
                &format!("member:{}", fixture.agent),
                Some("member-model"),
                false,
            )
            .unwrap();
        let mut session = fixture
            .sessions
            .create_agent(crate::sessions::NewAgentSession {
                avatar_id: None,
                project_id: fixture.project.clone(),
                agent_id: fixture.agent.clone(),
                runtime_kind: "codex".into(),
                agent_name: "worker".into(),
                cwd: fixture.root.display().to_string(),
            })
            .unwrap();
        fixture
            .store
            .link_agent_app_session(&fixture.project, &fixture.agent, &session.id)
            .unwrap();
        fixture
            .sessions
            .set_launch_model(&session.id, Some("other-bound-record".into()))
            .unwrap();
        let inherited = for_session(&session, &fixture.store).unwrap();
        assert_eq!(inherited.model.as_deref(), Some("member-model"));
        assert_eq!(inherited.source, "member");
        session.launch_model = Some("exact-target-model".into());
        assert_eq!(
            for_session(&session, &fixture.store)
                .unwrap()
                .model
                .as_deref(),
            Some("exact-target-model")
        );
        session.agent_runtime_kind = Some("pi".into());
        assert!(for_session(&session, &fixture.store).is_err());
        session.agent_runtime_kind = Some("codex".into());
        session.multi_agent_agent_id = Some("missing-member".into());
        assert!(for_session(&session, &fixture.store).is_err());
    }
    #[test]
    fn readonly_defaults_reject_save_and_keep_previous_bytes() {
        let fixture = Fixture::new();
        fixture
            .save("runtime:codex", Some("original-model"), false)
            .unwrap();
        let before = std::fs::read(&fixture.path).unwrap();
        let mut permissions = std::fs::metadata(&fixture.path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&fixture.path, permissions).unwrap();
        let result = fixture.save("runtime:codex", Some("new-model"), false);
        let mut permissions = std::fs::metadata(&fixture.path).unwrap().permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(&fixture.path, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
    }
    #[test]
    fn typed_migration_invalidates_a_same_value_legacy_draft() {
        let fixture = Fixture::new();
        let mut legacy =
            serde_json::to_value(fixture.store.agent_profile(&fixture.agent).unwrap()).unwrap();
        legacy["agentId"] = fixture.agent.clone().into();
        legacy["model"] = "same-model".into();
        fixture
            .store
            .update_agent(serde_json::from_value(legacy).unwrap())
            .unwrap();
        let id = format!("member:{}", fixture.agent);
        let before = fixture
            .snapshot()
            .rows
            .into_iter()
            .find(|row| row.id == id)
            .unwrap();
        fixture
            .store
            .update_model_assignment(
                &fixture.project,
                &fixture.agent,
                AgentRuntimeKind::Codex,
                Some("same-model"),
                None,
                Some("same-model"),
                None,
            )
            .unwrap();
        let after = fixture
            .snapshot()
            .rows
            .into_iter()
            .find(|row| row.id == id)
            .unwrap();
        assert_ne!(
            before.version, after.version,
            "a missing connection row and a typed inheritance row must not share a version"
        );
        assert!(save_at_path(
            SaveInput {
                id,
                version: before.version,
                connection_id: None,
                model: Some("stale-edit".into()),
                restore: false
            },
            &fixture.store,
            &fixture.sessions,
            &fixture.path
        )
        .is_err());
        assert_eq!(
            fixture
                .store
                .agent_profile(&fixture.agent)
                .unwrap()
                .model
                .as_deref(),
            Some("same-model")
        );
    }
    #[test]
    fn active_task_rejects_member_model_change_atomically() {
        let fixture = Fixture::new();
        let task = fixture.store.create_task(serde_json::from_value(serde_json::json!({"projectId":fixture.project,"title":"active model guard","description":"isolated fixture","acceptanceCriteria":"fixture only","assigneeAgentId":fixture.agent,"workspaceMode":"read_only"})).unwrap()).unwrap();
        fixture
            .store
            .claim_task(crate::multi_agent::ClaimTaskInput {
                task_id: task.id,
                agent_id: fixture.agent.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        let result = fixture.save(
            &format!("member:{}", fixture.agent),
            Some("must-not-apply"),
            false,
        );
        assert!(result.is_err());
        assert!(fixture
            .store
            .agent_profile(&fixture.agent)
            .unwrap()
            .model
            .is_none());
        assert!(fixture
            .store
            .model_connection(&fixture.agent, AgentRuntimeKind::Codex)
            .unwrap()
            .is_none());
    }
}

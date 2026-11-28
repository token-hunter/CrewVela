//! Project member model preferences and immutable discussion-round selections.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::{AgentRuntimeKind, MultiAgentStore};
use crate::acp::ManagedAcpKind;
use crate::codex_storage::CodexStorage;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Selection {
    pub model: Option<String>,
    pub reasoning: Option<String>,
}

impl Selection {
    pub(crate) fn is_custom(&self) -> bool { self.model.is_some() || self.reasoning.is_some() }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemberOptions {
    supported: bool,
    observed: bool,
    selection: Selection,
    model: Vec<Choice>,
    reasoning: Vec<Choice>,
}

#[derive(Serialize)]
struct Choice {
    value: String,
    label: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemberModelSummary {
    model: Option<String>,
    reasoning: Option<String>,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS ma_member_model_options (
        project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
        agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE CASCADE,
        runtime TEXT NOT NULL, options_json TEXT NOT NULL, selection_json TEXT NOT NULL,
        PRIMARY KEY(project_id, agent_id));
      CREATE TABLE IF NOT EXISTS ma_round_member_models (
        round_id TEXT NOT NULL REFERENCES ma_conversation_rounds(id) ON DELETE CASCADE,
        agent_id TEXT NOT NULL, runtime TEXT NOT NULL, selection_json TEXT NOT NULL,
        PRIMARY KEY(round_id, agent_id));")?;
    Ok(())
}

fn acp_kind(runtime: AgentRuntimeKind) -> Option<ManagedAcpKind> {
    match runtime {
        AgentRuntimeKind::ClaudeCode => Some(ManagedAcpKind::ClaudeCode),
        AgentRuntimeKind::Codex => Some(ManagedAcpKind::Codex),
        AgentRuntimeKind::KimiCode => Some(ManagedAcpKind::Kimi),
        AgentRuntimeKind::OpenCode => Some(ManagedAcpKind::OpenCode),
        AgentRuntimeKind::DeepSeekHarness => Some(ManagedAcpKind::DeepSeekHarness),
        AgentRuntimeKind::Antigravity => Some(ManagedAcpKind::Antigravity),
        _ => None,
    }
}

fn runtime_at(conn: &Connection, project: &str, agent: &str) -> Result<AgentRuntimeKind> {
    let value: String = conn.query_row("SELECT runtime_kind FROM ma_agents WHERE project_id=?1 AND id=?2",
        params![project, agent], |row| row.get(0)).optional()?.context("该成员已从当前项目移除")?;
    Ok(serde_json::from_value(json!(value))?)
}

fn option(catalog: &[Value], model: bool) -> Option<&Value> {
    catalog.iter().find(|item| {
        let id = item["id"].as_str().unwrap_or_default();
        let category = item["category"].as_str().unwrap_or_default();
        let keys: &[_] = if model { &["model"] } else { &["effort", "reasoning", "thinking", "thought_level", "reasoning_effort"] };
        !id.is_empty() && (keys.contains(&id) || keys.contains(&category)) && item["type"] == "select"
    })
}

fn choices(root: Option<&Value>) -> Vec<Choice> {
    fn visit(nodes: &Value, output: &mut Vec<Choice>) {
        let Some(nodes) = nodes.as_array() else { return };
        for node in nodes {
            if node["disabled"].as_bool().unwrap_or(false) { continue; }
            if let Some(value) = node["value"].as_str().filter(|value| !value.is_empty()) {
                if !output.iter().any(|choice| choice.value == value) {
                    output.push(Choice {value:value.into(), label:node["name"].as_str().unwrap_or(value).into()});
                }
            }
            visit(&node["options"], output);
        }
    }
    let mut output = Vec::new();
    if let Some(root) = root { visit(&root["options"], &mut output); }
    output
}

fn saved(conn: &Connection, project: &str, agent: &str, runtime: AgentRuntimeKind) -> Result<Option<(Vec<Value>, Selection)>> {
    let raw = conn.query_row(
        "SELECT options_json,selection_json FROM ma_member_model_options WHERE project_id=?1 AND agent_id=?2 AND runtime=?3",
        params![project, agent, runtime.as_str()], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?)),
    ).optional()?;
    raw.map(|(options, selection)| Ok((serde_json::from_str(&options)?, serde_json::from_str(&selection)?))).transpose()
}

fn response(runtime: AgentRuntimeKind, saved: Option<(Vec<Value>, Selection)>) -> MemberOptions {
    let observed = saved.is_some();
    let (catalog, selection) = saved.unwrap_or_default();
    MemberOptions { supported: supports_options(runtime), observed, selection, model:choices(option(&catalog,true)), reasoning:choices(option(&catalog,false)) }
}

fn supports_options(runtime: AgentRuntimeKind) -> bool {
    runtime == AgentRuntimeKind::Pi || acp_kind(runtime).is_some()
}

fn exposed_options(catalog: &[Value]) -> Vec<&Value> {
    [option(catalog, true), option(catalog, false)].into_iter().flatten().collect()
}

fn validate_selection(catalog: &[Value], selection: &Selection) -> Result<()> {
    for (value, model) in [(&selection.model, true), (&selection.reasoning, false)] {
        if let Some(value) = value {
            ensure!(choices(option(catalog, model)).iter().any(|choice| choice.value == *value), "此选项已不可用，请重新选择");
        }
    }
    Ok(())
}

pub(crate) fn draft_options(runtime: AgentRuntimeKind, model: Option<&str>) -> Result<MemberOptions> {
    if !supports_options(runtime) { return Ok(response(runtime, None)); }
    let _lease = crate::runtime_preferences::acquire(runtime)?;
    let catalog = discover_model_options(runtime, model)?;
    Ok(response(runtime, Some((catalog, Selection::default()))))
}

pub(crate) fn cli_options(runtime: AgentRuntimeKind, selection: &Selection) -> Result<MemberOptions> {
    crate::runtime_launch::validate_models(runtime.as_str(), selection)?;
    let _lease = crate::runtime_preferences::acquire(runtime)?;
    let model = if crate::runtime_launch::supports_reasoning(runtime.as_str()) {
        selection.model.as_deref()
    } else {
        None
    };
    let catalog = discover_model_options_with_storage(runtime, model, CodexStorage::Personal)?;
    let antigravity_orchestrator = runtime == AgentRuntimeKind::Antigravity
        && model.or_else(|| option(&catalog, true)?.get("currentValue")?.as_str())
            .is_some_and(|value| value.contains("orchestrator"));
    ensure!(!antigravity_orchestrator || selection.reasoning.is_none(),
        "Antigravity 编排模型不支持单独指定推理级别");
    validate_selection(&catalog, selection)?;
    let mut options = response(runtime, Some((catalog, Selection::default())));
    options.model.retain(|choice| crate::runtime_launch::valid_model_id(&choice.value));
    options.reasoning.retain(|choice| crate::runtime_launch::valid_reasoning(runtime.as_str(), &choice.value));
    if antigravity_orchestrator { options.reasoning.clear(); }
    Ok(options)
}

/// A validated selection and its catalog travel together into the profile transaction.
pub(crate) struct PreparedSelection(Option<(Vec<Value>, Selection)>);

impl PreparedSelection {
    pub(crate) fn check(runtime: AgentRuntimeKind, selection: Selection) -> Result<Self> {
        if !selection.is_custom() { return Ok(Self(None)); }
        let _lease = crate::runtime_preferences::acquire(runtime)?;
        let catalog = discover_model_options(runtime, selection.model.as_deref())?;
        Self::from_catalog(catalog, selection)
    }

    pub(super) fn from_catalog(catalog: Vec<Value>, selection: Selection) -> Result<Self> {
        validate_selection(&catalog, &selection)?;
        Ok(Self(Some((catalog, selection))))
    }

    pub(super) fn save(&self, conn: &Connection, agent: &super::AgentProfile) -> Result<()> {
        if let Some((catalog, selection)) = &self.0 {
            conn.execute("INSERT INTO ma_member_model_options(project_id,agent_id,runtime,options_json,selection_json)
                VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project_id,agent_id) DO UPDATE SET
                runtime=excluded.runtime, options_json=excluded.options_json, selection_json=excluded.selection_json",
                params![agent.project_id, agent.id, agent.runtime_kind.as_str(),
                    serde_json::to_string(&exposed_options(catalog))?, serde_json::to_string(selection)?])?;
        } else {
            conn.execute("DELETE FROM ma_member_model_options WHERE project_id=?1 AND agent_id=?2 AND runtime=?3",
                params![agent.project_id, agent.id, agent.runtime_kind.as_str()])?;
        }
        Ok(())
    }
}

pub(super) fn member_model_summaries(conn: &Connection, project: &str) -> Result<BTreeMap<String, MemberModelSummary>> {
    let mut statement = conn.prepare(
        "SELECT m.agent_id,m.options_json,m.selection_json FROM ma_member_model_options m
         JOIN ma_agents a ON a.id=m.agent_id AND a.project_id=m.project_id AND a.runtime_kind=m.runtime
         WHERE m.project_id=?1",
    )?;
    let rows = statement.query_map([project], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
    let mut summaries = BTreeMap::new();
    for row in rows {
        let (id, catalog, selection) = row?;
        let Ok(selection) = serde_json::from_str::<Selection>(&selection) else { continue; };
        if !selection.is_custom() { continue; }
        let catalog: Vec<Value> = serde_json::from_str(&catalog).unwrap_or_default();
        let label = |value: Option<String>, is_model: bool| value.map(|value| {
            choices(option(&catalog, is_model)).into_iter()
                .find(|choice| choice.value == value).map(|choice| choice.label).unwrap_or(value)
        });
        summaries.insert(id, MemberModelSummary {
            model: label(selection.model, true),
            reasoning: label(selection.reasoning, false),
        });
    }
    Ok(summaries)
}

impl MultiAgentStore {
    pub(crate) fn conversation_member_models(&self, project: &str, agent: &str) -> Result<MemberOptions> {
        let conn = self.connection.lock().unwrap();
        let runtime = runtime_at(&conn, project, agent)?;
        Ok(response(runtime, saved(&conn, project, agent, runtime)?))
    }

    pub(crate) fn load_conversation_member_models(&self, project: &str, agent: &str, refresh: bool) -> Result<MemberOptions> {
        self.load_member_models_with(project, agent, refresh, discover_model_options)
    }

    pub(super) fn load_member_models_with(&self, project: &str, agent: &str, refresh: bool,
        discover: impl FnOnce(AgentRuntimeKind, Option<&str>) -> Result<Vec<Value>>,
    ) -> Result<MemberOptions> {
        let profile = self.agent_profile(agent)?;
        ensure!(profile.project_id == project, "该成员不属于当前项目");
        let runtime = profile.runtime_kind;
        let options = self.conversation_member_models(project, agent)?;
        if !options.supported || (!refresh && (!options.model.is_empty() || !options.reasoning.is_empty())) {
            return Ok(options);
        }
        ensure!(profile.enabled, "请先启用该成员再读取可用选项");
        let _lease = crate::runtime_preferences::acquire(runtime)?;
        let catalog = discover(runtime, options.selection.model.as_deref())?;
        self.observe_conversation_models(project, agent, runtime, &catalog)?;
        self.conversation_member_models(project, agent)
    }

    pub(crate) fn set_conversation_member_models(&self, project: &str, agent: &str, selection: Selection) -> Result<MemberOptions> {
        let conn = self.connection.lock().unwrap();
        let runtime = runtime_at(&conn, project, agent)?;
        let (catalog, _) = saved(&conn, project, agent, runtime)?.context("尚未取得此成员的模型和推理选项")?;
        validate_selection(&catalog, &selection)?;
        conn.execute("UPDATE ma_member_model_options SET selection_json=?4 WHERE project_id=?1 AND agent_id=?2 AND runtime=?3",
            params![project, agent, runtime.as_str(), serde_json::to_string(&selection)?])?;
        Ok(response(runtime, Some((catalog, selection))))
    }

    pub(super) fn observe_conversation_models(&self, project: &str, agent: &str, runtime: AgentRuntimeKind, catalog: &[Value]) -> Result<()> {
        let conn = self.connection.lock().unwrap();
        if runtime_at(&conn, project, agent)? != runtime { return Ok(()); }
        let exposed = exposed_options(catalog);
        conn.execute("INSERT INTO ma_member_model_options(project_id,agent_id,runtime,options_json,selection_json) VALUES(?1,?2,?3,?4,'{}')
            ON CONFLICT(project_id,agent_id) DO UPDATE SET options_json=excluded.options_json,
              selection_json=CASE WHEN runtime=excluded.runtime THEN selection_json ELSE '{}' END, runtime=excluded.runtime",
            params![project,agent,runtime.as_str(),serde_json::to_string(&exposed)?])?;
        Ok(())
    }

    pub(super) fn discussion_model_selection(&self, task: &str, agent: &str, runtime: AgentRuntimeKind) -> Result<Selection> {
        let raw: Option<String> = self.connection.lock().unwrap().query_row(
            "SELECT m.selection_json FROM ma_conversation_turns t JOIN ma_round_member_models m ON m.round_id=t.round_id
             WHERE t.task_id=?1 AND m.agent_id=?2 AND m.runtime=?3",
            params![task,agent,runtime.as_str()], |row| row.get(0),
        ).optional()?;
        raw.map(|value| Ok(serde_json::from_str(&value)?)).transpose().map(Option::unwrap_or_default)
    }
}

fn discover_model_options(runtime: AgentRuntimeKind, saved_model: Option<&str>) -> Result<Vec<Value>> {
    discover_model_options_with_storage(runtime, saved_model, CodexStorage::CrewVela)
}

fn discover_model_options_with_storage(runtime: AgentRuntimeKind, saved_model: Option<&str>, codex_storage: CodexStorage) -> Result<Vec<Value>> {
    // Do not load project-local hooks, extensions or scripts for a menu read.
    let directory = std::env::temp_dir().join(format!("crewvela-model-catalog-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).context("无法创建模型目录读取所需的临时目录")?;
    let cwd = directory.to_string_lossy();
    let result = if runtime == AgentRuntimeKind::Pi {
        crate::native_pi_sessions::conversation_config_options(&cwd, saved_model, None)
    } else {
        acp_kind(runtime).context("此 Runtime 未提供模型选项")
            .and_then(|kind| crate::acp::conversation_model_catalog(kind, &cwd, saved_model, codex_storage))
    };
    // The scratch directory does not own provider home data.
    let _ = std::fs::remove_dir(directory);
    result
}

pub(super) fn snapshot_round(conn: &Connection, round: &str, project: &str) -> Result<()> {
    conn.execute("INSERT INTO ma_round_member_models(round_id,agent_id,runtime,selection_json)
        SELECT ?1,m.agent_id,m.runtime,m.selection_json FROM ma_member_model_options m
        JOIN ma_agents a ON a.id=m.agent_id AND a.runtime_kind=m.runtime
        WHERE m.project_id=?2 AND m.selection_json <> '{}'", params![round, project])?;
    Ok(())
}

pub(crate) fn apply_acp(connection: &mut crate::acp::AcpConnection, selection: &Selection) -> Result<()> {
    for (value, model) in [(&selection.model, true), (&selection.reasoning, false)] {
        let Some(value) = value else { continue };
        let item = option(connection.config_options(), model).context("当前会话没有公布所选配置")?;
        if item["currentValue"] == *value { continue; }
        ensure!(choices(Some(item)).iter().any(|choice| choice.value == *value), "此选项已不可用，请重新选择");
        let id = item["id"].as_str().context("配置选项缺少身份")?.to_owned();
        let options = connection.set_config_option(&id, value)?;
        ensure!(options.iter().any(|item| item["id"] == id && item["currentValue"] == *value),
            "Agent 未确认{}已切换，请在成员菜单重新选择", if model {"模型"} else {"推理级别"});
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::AcpConnection;
    use std::{path::Path, process::Command, sync::{Arc, atomic::AtomicU32}};

    #[test]
    #[ignore = "Requires locally installed runtimes and their adapters. Reads catalogs only, sends no prompt."]
    fn installed_cli_model_catalogs_before_launch() {
        let mut failures = Vec::new();
        for runtime in [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex, AgentRuntimeKind::Pi,
            AgentRuntimeKind::Antigravity, AgentRuntimeKind::KimiCode, AgentRuntimeKind::OpenCode] {
            let result = (|| -> Result<()> {
                let options = cli_options(runtime, &Selection::default())?;
                eprintln!("{}: {} models, {} reasoning levels", runtime.as_str(), options.model.len(), options.reasoning.len());
                let candidate = options.model.iter().find(|choice| choice.value != "default"
                    && !choice.value.contains("orchestrator")).context("目录未返回可选模型")?;
                let selected = cli_options(runtime, &Selection { model: Some(candidate.value.clone()), reasoning: None })?;
                if crate::runtime_launch::supports_reasoning(runtime.as_str()) {
                    let mut reasoning_available = !selected.reasoning.is_empty();
                    for candidate in options.model.iter().filter(|choice| choice.value.contains("gemini")).take(3) {
                        if reasoning_available { break; }
                        let selected = cli_options(runtime, &Selection { model: Some(candidate.value.clone()), reasoning: None })?;
                        reasoning_available = !selected.reasoning.is_empty();
                    }
                    ensure!(reasoning_available, "未从测试模型获取推理级别");
                } else {
                    ensure!(selected.reasoning.is_empty(), "只支持模型选择的 CLI 返回了推理级别");
                }
                if runtime == AgentRuntimeKind::Antigravity {
                    if let Some(model) = options.model.iter().find(|choice| choice.value.contains("orchestrator")) {
                        let selected = cli_options(runtime, &Selection { model: Some(model.value.clone()), reasoning: None })?;
                        ensure!(selected.reasoning.is_empty(), "编排模型仍返回推理级别");
                        eprintln!("antigravity: confirmed Orchestrator has no separate reasoning selector");
                    }
                }
                Ok(())
            })();
            if let Err(error) = result { failures.push(format!("{}: {error}", runtime.as_str())); }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Starts installed CLI terminals in fresh temporary directories without sending prompts or accepting trust."]
    fn installed_cli_launch_accepts_model_arguments() {
        use crate::{pty::PtyManager, sessions::{NewRuntimeSession, SessionManager}};
        use std::num::NonZeroU16;
        use std::time::{Duration, Instant};

        let only_runtime = std::env::var("CREWVELA_STARTUP_TEST_RUNTIME").ok();
        let mut tested = 0;
        let mut failed = Vec::new();
        for (runtime, mode, terminal_marker) in [
            (AgentRuntimeKind::ClaudeCode, "plan", "Claude Code"),
            (AgentRuntimeKind::Codex, "read_only", "OpenAI Codex"),
            (AgentRuntimeKind::Pi, "read_tools", "pi v"),
            (AgentRuntimeKind::Antigravity, "cli_default", "Antigravity"),
            (AgentRuntimeKind::KimiCode, "plan", "Kimi"),
            (AgentRuntimeKind::OpenCode, "build", "OpenCode"),
        ] {
            if only_runtime.as_deref().is_some_and(|kind| kind != runtime.as_str()) { continue; }
            tested += 1;
            let options = cli_options(runtime, &Selection::default()).unwrap();
            let model = options.model.iter().find(|choice| choice.value != "default" && !choice.value.contains("orchestrator")).unwrap();
            let options = cli_options(runtime, &Selection { model: Some(model.value.clone()), reasoning: None }).unwrap();
            let reasoning = options.reasoning.iter().find(|choice| choice.value == "high").or(options.reasoning.first());
            let selection = Selection { model: Some(model.value.clone()), reasoning: reasoning.map(|choice| choice.value.clone()) };
            cli_options(runtime, &selection).unwrap();

            let root = std::env::temp_dir().join(format!("crewvela-launch-models-{}", uuid::Uuid::new_v4()));
            let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
            let record = sessions.create_runtime_with_launch_mode(NewRuntimeSession {
                runtime_kind: runtime.as_str().into(), cwd: root.display().to_string(), project_id: None,
            }, mode).unwrap();
            let ptys = PtyManager::new();
            let started = crate::open_runtime_session_checked(&record, &ptys, &sessions, false, None, None, Some(&selection)).unwrap();
            let id = started.pty_id.unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut reached_terminal = false;
            let mut model_visible = false;
            let mut reasoning_visible = None;
            let new_screen = || vt100::Parser::new(NonZeroU16::new(24).unwrap(), NonZeroU16::new(80).unwrap(), 0);
            let mut screen = new_screen();
            while ptys.is_running(id) && Instant::now() < deadline {
                let chunks = ptys.poll_all();
                crate::mark_interactive_runtime_prompts(&chunks, &sessions);
                let bytes = ptys.transcript_tail(id, 64 * 1024).unwrap();
                screen = new_screen();
                screen.process(&bytes);
                let output = crate::multi_agent::strip_terminal_control_sequences(&String::from_utf8_lossy(&bytes));
                let visible = screen.screen().contents();
                model_visible = [&output, &visible].iter().any(|text| text.contains(&model.value) || text.contains(&model.label));
                reasoning_visible = reasoning.map(|choice| model_visible && visible.split_whitespace().any(|word| word.eq_ignore_ascii_case(&choice.value)));
                let waiting_for_trust = output.contains("Do you trust") || output.contains("trust this");
                reached_terminal |= model_visible || waiting_for_trust || output.contains(terminal_marker)
                    || sessions.is_ready_for_prompt(&record.id);
                if (model_visible && reasoning_visible != Some(false)) || waiting_for_trust { break; }
                std::thread::sleep(Duration::from_millis(150));
            }
            let running = ptys.is_running(id);
            ptys.kill_all().unwrap();
            drop(ptys);
            drop(sessions);
            let temp = std::env::temp_dir().canonicalize().unwrap();
            if root.canonicalize().is_ok_and(|path| path.parent() == Some(temp.as_path())) {
                let _ = std::fs::remove_dir_all(root);
            }
            eprintln!("{}: interactive terminal={}, selected model visible={}, reasoning visible={:?}",
                runtime.as_str(), reached_terminal && running, model_visible, reasoning_visible);
            if !reached_terminal || !running { failed.push((runtime.as_str(), running, reached_terminal, screen.screen().contents().len())); }
        }
        assert!(tested > 0, "Unknown CREWVELA_STARTUP_TEST_RUNTIME");
        assert!(failed.is_empty(), "Failed to reach interactive terminal: {failed:?}");
    }

    #[test]
    fn member_model_protocol_requires_confirmation_from_adapter() {
        let cwd = env!("CARGO_MANIFEST_DIR");
        let mut command = Command::new(which::which("node").unwrap());
        command.arg(Path::new(cwd).join("../tests/fixtures/native-harness-acp.mjs")).arg("--team-models");
        let mut connection = AcpConnection::spawn_native_deepseek_harness(
            cwd, None, vec![], Box::new(|_, _| Ok(false)),
            Arc::new(AtomicU32::new(0)), Some(command),
        ).unwrap();
        apply_acp(&mut connection, &Selection {model:Some("model-b".into()), reasoning:Some("high".into())}).unwrap();
        assert_eq!(option(connection.config_options(), true).unwrap()["currentValue"], "model-b");
        assert_eq!(option(connection.config_options(), false).unwrap()["currentValue"], "high");
        assert!(apply_acp(&mut connection, &Selection {model:None, reasoning:Some("max".into())})
            .unwrap_err().to_string().contains("未确认"));
    }

    #[test]
    fn member_model_summaries_are_scoped_and_use_saved_choice_labels() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE ma_projects (id TEXT PRIMARY KEY);
            CREATE TABLE ma_agents (id TEXT PRIMARY KEY, project_id TEXT, runtime_kind TEXT);
            INSERT INTO ma_projects VALUES ('a'), ('b');
            INSERT INTO ma_agents VALUES ('a-codex','a','codex'), ('a-pi','a','pi'),
                ('a-default','a','codex'), ('b-codex','b','codex');")?;
        migrate(&conn)?;
        let catalog = json!([
            {"id":"model","type":"select","options":[{"value":"terra","name":"6.1 Terra"}]},
            {"id":"effort","type":"select","options":[{"value":"medium","name":"Medium"}]}
        ]).to_string();
        for (project, agent, model, reasoning) in [
            ("a","a-codex",Some("terra"),Some("medium")),
            ("a","a-pi",Some("stale"),None),
            ("a","a-default",None,None),
            ("b","b-codex",Some("different"),None),
        ] {
            conn.execute("INSERT INTO ma_member_model_options VALUES (?1,?2,'codex',?3,?4)",
                params![project, agent, catalog, serde_json::to_string(&Selection {
                    model: model.map(str::to_owned), reasoning: reasoning.map(str::to_owned),
                })?])?;
        }
        let summaries = member_model_summaries(&conn, "a")?;
        assert_eq!(serde_json::to_value(summaries)?, json!({
            "a-codex":{"model":"6.1 Terra","reasoning":"Medium"}
        }));
        Ok(())
    }
}

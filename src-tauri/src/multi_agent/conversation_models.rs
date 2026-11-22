//! Project member model preferences and immutable discussion-round selections.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{AgentRuntimeKind, MultiAgentStore};
use crate::acp::ManagedAcpKind;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Selection {
    pub model: Option<String>,
    pub reasoning: Option<String>,
}

impl Selection {
    pub(super) fn is_custom(&self) -> bool { self.model.is_some() || self.reasoning.is_some() }
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
    MemberOptions { supported:runtime == AgentRuntimeKind::Pi || acp_kind(runtime).is_some(), observed, selection, model:choices(option(&catalog,true)), reasoning:choices(option(&catalog,false)) }
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
        for (value, model) in [(&selection.model, true), (&selection.reasoning, false)] {
            if let Some(value) = value {
                ensure!(choices(option(&catalog, model)).iter().any(|choice| choice.value == *value), "此选项已不可用，请重新选择");
            }
        }
        conn.execute("UPDATE ma_member_model_options SET selection_json=?4 WHERE project_id=?1 AND agent_id=?2 AND runtime=?3",
            params![project, agent, runtime.as_str(), serde_json::to_string(&selection)?])?;
        Ok(response(runtime, Some((catalog, selection))))
    }

    pub(super) fn observe_conversation_models(&self, project: &str, agent: &str, runtime: AgentRuntimeKind, catalog: &[Value]) -> Result<()> {
        let conn = self.connection.lock().unwrap();
        if runtime_at(&conn, project, agent)? != runtime { return Ok(()); }
        let exposed: Vec<_> = [option(catalog, true), option(catalog, false)].into_iter().flatten().collect();
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
    // Do not load project-local hooks, extensions or scripts for a menu read.
    let directory = std::env::temp_dir().join(format!("crewvela-model-catalog-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).context("无法创建模型目录读取所需的临时目录")?;
    let cwd = directory.to_string_lossy();
    let result = if runtime == AgentRuntimeKind::Pi {
        crate::native_pi_sessions::conversation_config_options(&cwd, saved_model, None)
    } else {
        crate::acp::conversation_model_catalog(acp_kind(runtime).context("此 Runtime 未提供模型选项")?, &cwd)
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

pub(super) fn apply_acp(connection: &mut crate::acp::AcpConnection, selection: &Selection) -> Result<()> {
    for (value, model) in [(&selection.model, true), (&selection.reasoning, false)] {
        let Some(value) = value else { continue };
        let item = option(connection.config_options(), model).context("当前会话没有公布所选配置")?;
        if item["currentValue"] == *value { continue; }
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
    #[ignore = "Requires locally installed Claude Code ACP. Reads catalog only, sends no prompt."]
    fn installed_claude_catalog_before_chat() {
        let catalog = discover_model_options(AgentRuntimeKind::ClaudeCode, None).unwrap();
        let models = choices(option(&catalog, true));
        let reasoning = choices(option(&catalog, false));
        eprintln!("Claude Code catalog: {} models, {} reasoning levels", models.len(), reasoning.len());
        assert!(!models.is_empty(), "The installed Claude adapter must expose selectable models");
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
}

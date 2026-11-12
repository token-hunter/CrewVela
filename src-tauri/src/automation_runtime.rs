//! Automation capability checks. No credentials or guessed executable paths are persisted here.
use crate::multi_agent::AgentRuntimeKind;
use crate::sessions::{SessionKind, SessionRecord, SessionStatus};
use anyhow::{bail, Result};

pub fn terminal_kind(kind: AgentRuntimeKind) -> bool {
    matches!(
        kind,
        AgentRuntimeKind::ClaudeCode
            | AgentRuntimeKind::Codex
            | AgentRuntimeKind::KimiCode
            | AgentRuntimeKind::Pi
            | AgentRuntimeKind::OpenCode
            | AgentRuntimeKind::ZCodeCli
    )
}
pub fn project_kind(kind: AgentRuntimeKind) -> bool {
    terminal_kind(kind)
        || matches!(
            kind,
            AgentRuntimeKind::DeepSeekHarness | AgentRuntimeKind::ZCode
        )
}
fn validate_model(kind: AgentRuntimeKind, model: Option<&str>) -> Result<()> {
    if let Some(model) = model {
        if !matches!(kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex) {
            bail!("该 Agent 的自动化暂只支持本地默认模型");
        }
        if model.is_empty()
            || model.len() > 200
            || model.starts_with('-')
            || !model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/@+[]".contains(c))
        {
            bail!("模型标识无效");
        }
    }
    Ok(())
}
pub fn configure_model(
    kind: AgentRuntimeKind,
    command: &mut std::process::Command,
    model: Option<&str>,
) -> Result<()> {
    validate_model(kind, model)?;
    if let Some(model) = model {
        command.arg("--model").arg(model);
    }
    Ok(())
}
pub fn validate_launch(
    kind: AgentRuntimeKind,
    provider: Option<&str>,
    model: Option<&str>,
    cwd: &str,
) -> Result<()> {
    if !terminal_kind(kind) {
        bail!("该 Agent 不支持自动化新建 CLI 会话，请使用项目成员或已有会话");
    }
    if cwd.trim().is_empty() || !std::path::Path::new(cwd).is_absolute() {
        bail!("请选择工作目录的完整路径");
    }
    if provider.is_some_and(|id| id.trim().is_empty()) {
        bail!("Provider ID 不能为空");
    }
    if provider.is_some() && kind != AgentRuntimeKind::ClaudeCode {
        bail!("仅 Claude Code 支持指定 CrewVela Provider");
    }
    validate_model(kind, model)?;
    Ok(())
}
pub fn session_runtime(session: &SessionRecord) -> Option<AgentRuntimeKind> {
    session
        .agent_runtime_kind
        .as_ref()
        .map(|kind| {
            serde_json::from_value(serde_json::json!(kind)).unwrap_or(AgentRuntimeKind::Unsupported)
        })
        .or_else(|| (session.kind == SessionKind::Claude).then_some(AgentRuntimeKind::ClaudeCode))
}
pub fn message_candidate(session: &SessionRecord, kind: Option<AgentRuntimeKind>) -> bool {
    !session.retired_agent_runtime
        && session.archived_at.is_none()
        && session_runtime(session).is_some_and(|actual| {
            !actual.is_retired() && kind.is_none_or(|expected| actual == expected)
        })
}
pub fn live_candidate(session: &SessionRecord, kind: AgentRuntimeKind) -> bool {
    message_candidate(session, Some(kind))
        && !matches!(
            session.status,
            SessionStatus::Stopped | SessionStatus::Error
        )
        && (session.pty_id.is_some()
            || session.runtime_command_host
            || matches!(
                session.status,
                SessionStatus::Running | SessionStatus::Completed | SessionStatus::Waiting
            ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_model_default_adds_no_args_and_override_rejects_shell_syntax() {
        let mut command = std::process::Command::new("fixture");
        configure_model(AgentRuntimeKind::Codex, &mut command, None).unwrap();
        assert_eq!(command.get_args().count(), 0);
        configure_model(
            AgentRuntimeKind::Codex,
            &mut command,
            Some("provider/model:variant[1m]"),
        )
        .unwrap();
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["--model", "provider/model:variant[1m]"]
        );
        assert!(configure_model(AgentRuntimeKind::Pi, &mut command, Some("model")).is_err());
        for model in [
            "name&echo",
            "name%PATH%",
            "name;command",
            "name with spaces",
            "--flag",
        ] {
            assert!(configure_model(AgentRuntimeKind::Codex, &mut command, Some(model)).is_err());
        }
    }
    #[test]
    fn launch_configuration_preserves_local_defaults_and_rejects_unsupported_overrides() {
        let cwd = std::env::current_dir().unwrap().display().to_string();
        for kind in [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
            AgentRuntimeKind::ZCodeCli,
        ] {
            assert!(validate_launch(kind, None, None, &cwd).is_ok());
        }
        assert!(validate_launch(AgentRuntimeKind::Codex, Some("provider"), None, &cwd).is_err());
        assert!(validate_launch(AgentRuntimeKind::ZCode, None, None, &cwd).is_err());
        assert!(validate_launch(
            AgentRuntimeKind::Codex,
            None,
            Some("provider/model:variant"),
            &cwd
        )
        .is_ok());
        assert!(validate_launch(AgentRuntimeKind::Pi, None, Some("model"), &cwd).is_err());
        assert!(validate_launch(AgentRuntimeKind::Codex, None, Some("line\nbreak"), &cwd).is_err());
        assert!(validate_launch(AgentRuntimeKind::ClaudeCode, None, None, "relative").is_err());
    }
}

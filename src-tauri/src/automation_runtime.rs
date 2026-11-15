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
pub fn validate_launch(
    kind: AgentRuntimeKind,
    _provider: Option<&str>,
    _model: Option<&str>,
    cwd: &str,
) -> Result<()> {
    if !terminal_kind(kind) {
        bail!("该 Agent 不支持自动化新建 CLI 会话，请使用项目成员或已有会话");
    }
    if cwd.trim().is_empty() || !std::path::Path::new(cwd).is_absolute() {
        bail!("请选择工作目录的完整路径");
    }
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
        assert!(validate_launch(AgentRuntimeKind::Codex, Some("retired-provider"), None, &cwd).is_ok());
        assert!(validate_launch(AgentRuntimeKind::ZCode, None, None, &cwd).is_err());
        assert!(validate_launch(
            AgentRuntimeKind::Codex,
            None,
            Some("provider/model:variant"),
            &cwd
        )
        .is_ok());
        assert!(validate_launch(AgentRuntimeKind::Pi, None, Some("ignored-model"), &cwd).is_ok());
        assert!(validate_launch(AgentRuntimeKind::Codex, None, Some("ignored\nbreak"), &cwd).is_ok());
        assert!(validate_launch(AgentRuntimeKind::ClaudeCode, None, None, "relative").is_err());
    }
}

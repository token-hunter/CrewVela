//! Native Chat adapters: use each manager's own history and send/stop interfaces.
use crate::native_acp_sessions::{
    NativeAcpSessions, NativeClaudeSessions, NativeCodexChatSessions, NativeKimiSessions,
    NativeOpenCodeChatSessions,
};
use crate::native_pi_sessions::NativePiSessions;
use crate::native_zcode_sessions::NativeZCodeSessions;
use crate::session_switch::{source_runtime, CHAT_RUNTIMES};
use crate::sessions::{SessionManager, SessionRecord};
use crate::transcript::TranscriptMessage;
use anyhow::{bail, Context, Result};
use tauri::{AppHandle, Manager};

pub(crate) fn is_native(record: &SessionRecord) -> bool {
    CHAT_RUNTIMES.contains(&source_runtime(record))
        || crate::session_switch::structured_zcode_cli(&record.id)
}
fn acp<R: tauri::Runtime>(app: &AppHandle<R>, kind: &str) -> Option<NativeAcpSessions> {
    match kind {
        "deepseek_harness" => Some(app.state::<NativeAcpSessions>().inner().clone()),
        "claude_chat" => Some(app.state::<NativeClaudeSessions>().0.clone()),
        "kimi_chat" => Some(app.state::<NativeKimiSessions>().0.clone()),
        "codex_chat" => Some(app.state::<NativeCodexChatSessions>().0.clone()),
        "opencode_chat" => Some(app.state::<NativeOpenCodeChatSessions>().0.clone()),
        _ => None,
    }
}
pub(crate) fn history<R: tauri::Runtime>(
    app: Option<&AppHandle<R>>,
    record: &SessionRecord,
    sessions: &SessionManager,
) -> Result<Vec<TranscriptMessage>> {
    if !is_native(record) {
        return crate::session_switch_history::read(record, sessions);
    }
    let app = app.context("原生历史需要应用上下文")?;
    let mut task_state = Vec::new();
    let messages = if let Some(manager) = acp(app, source_runtime(record)) {
        let snapshot = manager.snapshot(&record.id)?;
        if let Some(goal) = snapshot.codex_goal {
            task_state.push(TranscriptMessage {
                role: "system".into(),
                text: format!(
                    "[来源 Agent 的用户目标及状态；需核实剩余工作，不继承执行器内部状态]\n{}",
                    serde_json::to_string(&goal)?
                ),
                at: None,
            });
        }
        if let Some(goal) = snapshot
            .control_state
            .get("goal")
            .filter(|value| !value.is_null())
        {
            task_state.push(TranscriptMessage {
                role: "system".into(),
                text: format!("[来源 Agent 的目标/进度线索；不代表已完成或验证通过]\n{goal}"),
                at: None,
            });
        }
        snapshot.history
    } else if source_runtime(record) == "pi_chat" {
        app.state::<NativePiSessions>()
            .snapshot(&record.id)?
            .history
    } else {
        return Ok(zcode_history(
            &app.state::<NativeZCodeSessions>()
                .snapshot(&record.id)?
                .rows,
        ));
    };
    let mut result = messages
        .into_iter()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| {
            let mut text = m.text;
            for attachment in m.attachments {
                text.push_str(&format!(
                    "\n[原输入附件：{}；路径：{}；内容需用目标的工具和权限重新读取]",
                    attachment.name, attachment.path
                ));
            }
            TranscriptMessage {
                role: m.role,
                text,
                at: None,
            }
        })
        .collect::<Vec<_>>();
    result.extend(task_state);
    Ok(result)
}
fn zcode_history(rows: &[serde_json::Value]) -> Vec<TranscriptMessage> {
    rows.iter()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter_map(|row| {
            let role = row
                .get("role")
                .and_then(|v| v.as_str())
                .or_else(|| row.get("kind").and_then(|v| v.as_str()))?;
            let text = row
                .get("text")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| row.to_string());
            Some(TranscriptMessage {
                role: match role {
                    "userInput" => "user",
                    "assistantMessage" | "text" => "assistant",
                    _ => role,
                }
                .into(),
                text,
                at: None,
            })
        })
        .collect()
}
pub(crate) fn permission_mode<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: &SessionRecord,
) -> Result<Option<String>> {
    if let Some(manager) = acp(app, source_runtime(record)) {
        return Ok(Some(manager.handoff_permission_mode(&record.id)?));
    }
    if matches!(source_runtime(record), "zcode" | "zcode_cli") && is_native(record) {
        return Ok(app
            .state::<NativeZCodeSessions>()
            .snapshot(&record.id)?
            .permission_mode);
    }
    Ok(record
        .permission_mode
        .map(|mode| format!("{mode:?}").to_lowercase()))
}
pub(crate) fn stop<R: tauri::Runtime>(app: &AppHandle<R>, record: &SessionRecord) -> Result<()> {
    let sessions = app.state::<SessionManager>();
    if let Some(manager) = acp(app, source_runtime(record)) {
        manager.stop_for_handoff(&record.id, &sessions, app)?;
    } else if source_runtime(record) == "pi_chat" {
        app.state::<NativePiSessions>()
            .stop_for_handoff(&record.id, &sessions, app)?;
    } else {
        app.state::<NativeZCodeSessions>()
            .stop_for_handoff(&record.id, &sessions, app)?;
    }
    Ok(())
}
pub(crate) fn start<R: tauri::Runtime>(app: &AppHandle<R>, record: &SessionRecord) -> Result<()> {
    let sessions = app.state::<SessionManager>();
    if let Some(manager) = acp(app, source_runtime(record)) {
        if !manager.contains(&record.id) {
            manager.register(&record.id, &record.cwd)?;
        }
        manager.start(app.clone(), &sessions, record)?;
        if source_runtime(record) == "kimi_chat" {
            let snapshot = manager.snapshot(&record.id)?;
            if !snapshot.config_options.iter().any(|option| {
                option["id"] == "mode"
                    && matches!(option["currentValue"].as_str(), Some("default" | "manual"))
            }) {
                bail!("Kimi Chat 未确认进入手动审批模式；未发送交接指令，请核对权限或恢复来源");
            }
        }
    } else if source_runtime(record) == "pi_chat" {
        let manager = app.state::<NativePiSessions>();
        if !manager.contains(&record.id) {
            manager.register(&record.id, &record.cwd)?;
        }
        manager.start(app.clone(), &sessions, record)?;
    } else {
        let manager = app.state::<NativeZCodeSessions>();
        if !manager.contains(&record.id) {
            manager.register(&record.id, &record.cwd)?;
        }
        manager.prepare_handoff(&record.id)?;
        manager.start(app.clone(), &sessions, record)?;
    }
    Ok(())
}
pub(crate) fn send<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: &SessionRecord,
    id: &str,
    body: &str,
) -> Result<()> {
    let sessions = app.state::<SessionManager>();
    if let Some(manager) = acp(app, source_runtime(record)) {
        manager.send_message_with_attachments(app.clone(), &sessions, &record.id, body, &[])?;
    } else if source_runtime(record) == "pi_chat" {
        app.state::<NativePiSessions>()
            .send_message_with_attachments(app.clone(), &sessions, &record.id, body, &[])?;
    } else {
        app.state::<NativeZCodeSessions>()
            .send_handoff(&record.id, id, body)?;
    }
    Ok(())
}
pub(crate) fn has_submission<R: tauri::Runtime>(
    app: &AppHandle<R>,
    record: &SessionRecord,
    id: &str,
) -> Result<bool> {
    let marker = format!("[CrewVela handoff {id}]");
    if matches!(source_runtime(record), "zcode" | "zcode_cli") {
        let snapshot = app.state::<NativeZCodeSessions>().snapshot(&record.id)?;
        if snapshot
            .recoverable_inputs
            .iter()
            .any(|v| v["sourceCommandId"] == id)
        {
            bail!("ZCODE_INPUT_UNCERTAIN: 原生待核对输入仍未确认，不能重复发送");
        }
        return Ok(snapshot
            .rows
            .iter()
            .any(|v| v.to_string().contains(&marker))
            || snapshot
                .queued_inputs
                .iter()
                .any(|v| v["sourceCommandId"] == id));
    }
    Ok(history(Some(app), record, &app.state::<SessionManager>())?
        .iter()
        .any(|m| m.role == "user" && m.text.starts_with(&marker)))
}

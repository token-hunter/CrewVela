use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};

use crate::control::{MessageOrigin, OutboxMessage};
use crate::sessions::{self, SessionRecord};

/// 解析 `send` 子命令参数：--to <会话 id 前缀> --body <内容>。
pub fn parse_send_args<I: Iterator<Item = String>>(args: I) -> Result<(String, String), String> {
    let mut to = None;
    let mut body = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--to" => to = Some(args.next().ok_or("send 缺少 --to 的值")?),
            "--body" => body = Some(args.next().ok_or("send 缺少 --body 的值")?),
            other => return Err(format!("send 无法识别的参数: {other}")),
        }
    }
    let (Some(to), Some(body)) = (to, body) else {
        return Err("用法: crewvela send --to <会话id前缀> --body <内容>".to_string());
    };
    if to.trim().is_empty() || body.trim().is_empty() {
        return Err("send 的 --to 与 --body 不能为空".to_string());
    }
    Ok((to, body))
}

fn sessions_path() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("sessions.json"))
}

fn outbox_dir() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("outbox"))
}

/// `crewvela sessions`：以 JSON 列出全部会话，供会话内 agent 发现消息目标。
pub fn run_sessions_command() -> Result<()> {
    let view = session_view_at(&sessions_path()?)?;
    println!("{}", serde_json::to_string_pretty(&view)?);
    Ok(())
}

pub(crate) fn session_view_at(path: &Path) -> Result<Vec<serde_json::Value>> {
    let records = sessions::load_records(path)?;
    let view = records
        .iter()
        .filter(|record| !record.retired_agent_runtime)
        .map(|record| {
            serde_json::json!({
                "id": record.id,
                "title": record.title,
                "projectId": record.multi_agent_project_id,
                "agentId": record.multi_agent_agent_id,
                "providerName": record.provider_name,
                "status": record.status,
                "cwd": record.cwd,
                "runnable": record.pty_id.is_some() || record.runtime_command_host,
                "processRunning": record.pty_id.is_some(),
                "runtimeKind": record.agent_runtime_kind.as_deref().or_else(||
                    (record.kind == sessions::SessionKind::Claude).then_some("claude_code")),
                "deliveryMode": if record.runtime_command_host { "scheduled_turn" } else { "terminal" },
                "preferredForRuntime": is_live_terminal(record),
                "selectionHint": if is_live_terminal(record) { "用户已打开的交互会话，优先选择" } else { "仅在用户明确指定此项目成员时选择" },
                "pendingReason": terminal_pending_hint(record),
                "deliveryError": record.last_error,
            })
        })
        .collect::<Vec<_>>();
    Ok(view)
}

/// `crewvela send --to <前缀> --body <内容>`：把消息写入 outbox，
/// 由运行中的 app 在下一轮轮询摄取并投递（会话就绪时写入其终端）。
pub fn run_send_command(to: &str, body: &str) -> Result<()> {
    let identity = std::env::var("CREWVELA_SESSION_ID").ok();
    let receipt = send_at(&sessions_path()?, &outbox_dir()?, to, body, identity.as_deref(), crate::runtime_preferences::ensure_session_available)?;
    println!("{receipt}");
    Ok(())
}

pub(crate) fn send_at(
    session_file: &Path,
    outbox: &Path,
    to: &str,
    body: &str,
    identity: Option<&str>,
    ensure_available: impl Fn(&SessionRecord) -> Result<()>,
) -> Result<String> {
    if to.trim().is_empty() || body.trim().is_empty() {
        return Err(anyhow!("消息目标和内容不能为空"));
    }
    if body.chars().count() > 32_000 {
        return Err(anyhow!("消息不能超过 32000 个字符"));
    }
    let records = sessions::load_records(session_file)?;
    let target = resolve_session(&records, to)?;
    ensure_available(&target)?;
    if let Some(sender_id) = identity {
        if let Some(sender) = records
            .iter()
            .find(|record| record.id == sender_id)
        {
            ensure_available(sender)?;
        }
    }
    // 发送方身份来自 CREWVELA_SESSION_ID（app 启动会话时注入）；
    // 人在外部终端手动发送时该变量缺失，消息按无来源逐字投递。
    let from = resolve_sender(&records, identity)?;
    if from.as_ref().is_some_and(|sender| sender.session_id == target.id) {
        return Err(anyhow!("不能向自己发送会话消息；请重新查询 sessions 并选择另一位参与者的完整会话 ID"));
    }
    write_outbox(
        outbox,
        &OutboxMessage {
            to: target.id.clone(),
            from,
            body: body.to_string(),
        },
    )?;
    Ok(send_receipt(&records, &target))
}

fn send_receipt(records: &[SessionRecord], target: &SessionRecord) -> String {
    let label = format!("「{}」（会话 ID：{}）", target.title, target.id);
    if target.runtime_command_host {
        return format!("已排队交给{label}的后台消息轮次处理（项目暂停或成员禁用时保留等待）");
    }
    if target.pty_id.is_none() {
        let runtime = target
            .agent_runtime_kind
            .as_deref()
            .or_else(|| (target.kind == sessions::SessionKind::Claude).then_some("claude_code"));
        let candidates = records
            .iter()
            .filter(|record| {
                let kind = record.agent_runtime_kind.as_deref().or_else(|| {
                    (record.kind == sessions::SessionKind::Claude).then_some("claude_code")
                });
                runtime.is_some() && kind == runtime && is_live_terminal(record)
            })
            .map(|record| format!("{}（{}）", record.id, record.title))
            .collect::<Vec<_>>();
        let mut receipt =
            format!("已排队发送给{label}；该目标会话未运行，消息留在此 ID 等待恢复，尚未投递");
        if !candidates.is_empty() {
            receipt.push_str(&format!("。当前已打开的同 Runtime 交互会话：{}。请重新运行 sessions 核对当前目标；用户泛指当前 Agent 时重新选择，不需要在外部终端启动新 Agent。本条消息未自动转发，不要重复发送同一条消息", candidates.join("、")));
        }
        return receipt;
    }
    if let Some(hint) = terminal_pending_hint(target) {
        format!("已排队给{label}；{hint}")
    } else {
        format!("已排队发送给{label}")
    }
}

fn resolve_sender(records: &[SessionRecord], id: Option<&str>) -> Result<Option<MessageOrigin>> {
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let sender = records
        .iter()
        .find(|record| record.id == id && !record.retired_agent_runtime)
        .ok_or_else(|| {
            anyhow!("发送方会话身份 {id} 已失效；请在新构建恢复原会话，未降级为无来源消息")
        })?;
    Ok(Some(MessageOrigin {
        session_id: sender.id.clone(),
        label: format!("{} ({})", sender.title, sender.provider_name),
    }))
}

fn is_live_terminal(record: &SessionRecord) -> bool {
    !record.retired_agent_runtime
        && record.pty_id.is_some()
        && !record.runtime_command_host
        && record.archived_at.is_none()
}

fn terminal_pending_hint(record: &SessionRecord) -> Option<&'static str> {
    if record.runtime_command_host {
        return None;
    }
    match record.agent_runtime_kind.as_deref() {
        Some("pi") => Some("消息留在此交互会话，等待Pi扩展上报就绪；旧进程需用新构建恢复或重新启动以加载扩展，不转发项目成员"),
        _ => None,
    }
}

fn runtime_alias(target: &str) -> Option<&'static str> {
    match target.trim().to_ascii_lowercase().as_str() {
        "pi" | "pi agent" => Some("pi"),
        "claude" | "claude code" => Some("claude_code"),
        "codex" | "codex cli" => Some("codex"),
        "kimi" | "kimi code" => Some("kimi_code"),
        "opencode" => Some("opencode"),
        "zcode" => Some("zcode"),
        "zcode cli" | "zcode_cli" => Some("zcode_cli"),
        "dsh" | "deepseek harness" | "deepseek_harness" => Some("deepseek_harness"),
        _ => None,
    }
}

/// 完整 id 精确匹配优先，其次 id 前缀，最后标题精确匹配。
fn resolve_session(records: &[SessionRecord], target: &str) -> Result<SessionRecord> {
    let historical_records = records;
    if records
        .iter()
        .any(|record| record.retired_agent_runtime && record.id == target)
    {
        return Err(anyhow!(
            "目标是 Runtime 切换前的只读历史会话，消息未发送；请重新选择成员当前会话"
        ));
    }
    let records = records
        .iter()
        .filter(|record| !record.retired_agent_runtime)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(record) = records.iter().find(|record| record.id == target) {
        return Ok(record.clone());
    }
    let prefix_matches = records
        .iter()
        .filter(|record| record.id.starts_with(target))
        .collect::<Vec<_>>();
    match prefix_matches.len() {
        1 => return Ok((*prefix_matches[0]).clone()),
        len if len > 1 => {
            let candidates = prefix_matches
                .iter()
                .map(|record| format!("{}（{}）", record.id, record.title))
                .collect::<Vec<_>>()
                .join("、");
            return Err(anyhow!(
                "会话 id 前缀「{target}」不唯一，候选：{candidates}"
            ));
        }
        _ => {}
    }
    let titles = records
        .iter()
        .filter(|record| record.title == target)
        .collect::<Vec<_>>();
    match titles.as_slice() {
        [record] => return Ok((*record).clone()),
        [] => {}
        _ => return Err(anyhow!("会话标题「{target}」不唯一，请使用会话 id")),
    }
    if let Some(runtime) = runtime_alias(target) {
        let candidates = records
            .iter()
            .filter(|record| {
                is_live_terminal(record)
                    && (record.agent_runtime_kind.as_deref() == Some(runtime)
                        || (runtime == "claude_code"
                            && record.kind == sessions::SessionKind::Claude))
            })
            .collect::<Vec<_>>();
        return match candidates.as_slice() {
            [record] => Ok((*record).clone()),
            [] => Err(anyhow!(
                "没有已打开的 {target} 交互会话；请打开目标会话，或明确指定项目成员的 id/完整标题"
            )),
            _ => Err(anyhow!(
                "已打开多个 {target} 交互会话，请指定 id/完整标题：{}",
                candidates
                    .iter()
                    .map(|record| format!("{}（{}）", record.id, record.title))
                    .collect::<Vec<_>>()
                    .join("、")
            )),
        };
    }
    if historical_records.iter().any(|record| {
        record.retired_agent_runtime && (record.id.starts_with(target) || record.title == target)
    }) {
        return Err(anyhow!(
            "目标是 Runtime 切换前的只读历史会话，消息未发送；请重新选择成员当前会话"
        ));
    }
    Err(anyhow!(
        "未找到会话「{target}」；运行 crewvela sessions 查看可发送的会话"
    ))
}

/// outbox 原子写入（temp + rename），文件名带纳秒前缀保证摄取顺序。
pub(crate) fn write_outbox(dir: &Path, message: &OutboxMessage) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("创建消息 outbox 目录失败: {}", dir.display()))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u64::MAX as u128) as u64;
    let id = uuid::Uuid::new_v4();
    let temp_path = dir.join(format!("{nanos:020}-{id}.tmp"));
    let message_path = dir.join(format!("{nanos:020}-{id}.json"));
    std::fs::write(&temp_path, serde_json::to_vec(message)?)
        .with_context(|| format!("写入会话消息失败: {}", temp_path.display()))?;
    std::fs::rename(&temp_path, &message_path)
        .with_context(|| format!("提交会话消息失败: {}", message_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{ApprovalPolicy, SessionKind, SessionStatus};

    #[test]
    fn isolated_cli_child_process() {
        let Some(root) = std::env::var_os("CREWVELA_MESSAGE_TEST_ROOT") else { return; };
        let root = PathBuf::from(root).canonicalize().unwrap();
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert_eq!(std::fs::read_to_string(root.join(".cli-fixture")).unwrap(), "test-only");
        if std::env::var("CREWVELA_MESSAGE_ASSERT_READONLY").as_deref() == Ok("1") {
            assert!(std::fs::write(root.join("business/forbidden.txt"), "must remain read-only").is_err());
            assert!(std::fs::write(root.join("codex-home/config.toml"), "must preserve configuration").is_err());
            println!("CLI_FIXTURE_BUSINESS_WRITE_DENIED=true");
        }
        let argv: Vec<String> = serde_json::from_str(&std::env::var("CREWVELA_MESSAGE_TEST_ARGS").unwrap()).unwrap();
        let result = match argv.first().map(String::as_str) {
            Some("sessions") => session_view_at(&root.join("sessions.json")).map(|value| {
                println!("CLI_FIXTURE_JSON={}", serde_json::to_string(&value).unwrap());
            }),
            Some("send") => parse_send_args(argv.into_iter().skip(1)).map_err(anyhow::Error::msg).and_then(|(to, body)| {
                let identity = std::env::var("CREWVELA_SESSION_ID").ok();
                send_at(&root.join("sessions.json"), &root.join("outbox"), &to, &body, identity.as_deref(), |_| Ok(())).map(|receipt| println!("CLI_FIXTURE_RECEIPT={receipt}"))
            }),
            _ => panic!("Unsupported fixture command"),
        };
        result.unwrap();
    }

    #[test]
    fn isolated_cli_processes_discover_send_reply_and_preserve_the_durable_mailbox() {
        use crate::control::SessionActionQueue;
        let root = std::env::temp_dir().join(format!("crewvela-cli-process-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join(".cli-fixture"), "test-only").unwrap();
        let mut sender = record("sender-precise-id", "发送方");
        sender.agent_runtime_kind = Some("codex".into());
        let mut target = record("target-precise-id", "目标方");
        target.agent_runtime_kind = Some("kimi_code".into());
        let mut removed = record("removed-id", "历史成员");
        removed.agent_runtime_kind = Some("minimax_code".into());
        let records = vec![sender.clone(), target.clone(), removed];
        std::fs::write(root.join("sessions.json"), serde_json::to_vec(&serde_json::json!({"version":1,"sessions":records})).unwrap()).unwrap();
        let run = |identity: &str, arguments: Vec<&str>| {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "messaging::tests::isolated_cli_child_process", "--nocapture"])
                .env("CREWVELA_MESSAGE_TEST_ROOT", &root)
                .env("CREWVELA_MESSAGE_TEST_ARGS", serde_json::to_string(&arguments).unwrap())
                .env("CREWVELA_SESSION_ID", identity);
            #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
            command.output().unwrap()
        };
        let listed = run(&sender.id, vec!["sessions"]);
        assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
        let stdout = String::from_utf8(listed.stdout).unwrap();
        let line = stdout.lines().find_map(|line| line.strip_prefix("CLI_FIXTURE_JSON=")).unwrap();
        let listed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 2);
        assert!(listed.as_array().unwrap().iter().any(|row| row["id"] == target.id && row["preferredForRuntime"] == true));
        let body = "核对通信链路\n保留 '引号' $变量 `反引号` 和中文";
        let sent = run(&sender.id, vec!["send", "--to", "target-precise", "--body", body]);
        assert!(sent.status.success(), "{}", String::from_utf8_lossy(&sent.stderr));
        assert!(String::from_utf8_lossy(&sent.stdout).contains(&target.id));
        let manager = crate::sessions::SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 1);
        let first = queue.peek_next(&target.id).unwrap();
        assert_eq!(first.body, body);
        assert_eq!(first.from.as_ref().unwrap().session_id, sender.id);
        assert!(crate::control::render_delivery(&first).contains(&format!("send --to '{}'", sender.id)));
        let reply = "已收到；回复仍指向原始发送方";
        let replied = run(&target.id, vec!["send", "--to", &first.from.unwrap().session_id, "--body", reply]);
        assert!(replied.status.success(), "{}", String::from_utf8_lossy(&replied.stderr));
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 1);
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 0);
        let answer = queue.peek_next(&sender.id).unwrap();
        assert_eq!(answer.body, reply);
        assert_eq!(answer.from.unwrap().session_id, target.id);
        let reopened = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert_eq!(reopened.peek_next(&sender.id).unwrap().body, reply);
        assert!(!run("unknown-sender", vec!["send", "--to", &target.id, "--body", "invalid identity"]).status.success());
        assert!(!run(&sender.id, vec!["send", "--to", "removed-id", "--body", "must reject"]).status.success());
        assert!(!run(&sender.id, vec!["send", "--to", &target.id, "--body", ""]).status.success());
        assert_eq!(reopened.ingest_outbox(&manager, |_| Ok(())).unwrap(), 0);
        assert_eq!(std::fs::read_dir(root.join("outbox")).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn record(id: &str, title: &str) -> SessionRecord {
        SessionRecord {
            launch_model: None,            id: id.into(),
            claude_session_id: id.into(),
            kind: SessionKind::Claude,
            shell: None,
            multi_agent_project_id: None,
            multi_agent_agent_id: None,
            agent_runtime_kind: None,
            retired_agent_runtime: false,
            agent_avatar_id: None,
            runtime_command_host: false,
            project_id: None,
            worktree: None,
            provider_id: "glm".into(),
            provider_name: "Z.AI GLM".into(),
            cwd: ".".into(),
            title: title.into(),
            status: SessionStatus::Running,
            pty_id: Some(1),
            yolo: false,
            auto_mode: false,
            permission_mode: None,
            permission_mode_revision: 0,
            approval_policy: ApprovalPolicy::default(),
            waiting_reason: None,
            waiting_since: None,
            title_locked: false,
            exit_code: None,
            last_error: None,
            archived_at: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn args<'a>(values: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
        values.iter().map(|value| value.to_string())
    }

    #[test]
    fn runtime_switch_old_ids_are_not_deliverable_and_current_session_is_selected() {
        let mut old = record("old", "Codex · 成员");
        old.agent_runtime_kind = Some("codex".into());
        old.retired_agent_runtime = true;
        old.pty_id = Some(1);
        let mut new = record("new", "Pi · 成员");
        new.agent_runtime_kind = Some("pi".into());
        new.pty_id = Some(2);
        let records = vec![old, new];
        assert!(resolve_session(&records, "old").is_err());
        assert!(resolve_session(&records, "Codex · 成员").is_err());
        assert_eq!(resolve_session(&records, "pi").unwrap().id, "new");
        assert!(resolve_sender(&records, Some("old")).is_err());
        let mut switched_back = record("fresh-codex", "Codex · 成员");
        switched_back.agent_runtime_kind = Some("codex".into());
        let mut records = records;
        records.push(switched_back);
        assert_eq!(
            resolve_session(&records, "Codex · 成员").unwrap().id,
            "fresh-codex"
        );
    }

    #[test]
    fn six_runtime_aliases_and_sender_identity_keep_original_session_ids() {
        for (index, alias) in [
            "claude", "codex", "kimi", "pi", "opencode",
        ]
        .iter()
        .enumerate()
        {
            let kind = runtime_alias(alias).unwrap();
            let mut live = record(&format!("live-{index}"), &format!("用户会话-{index}"));
            live.kind = SessionKind::Terminal;
            live.agent_runtime_kind = Some(kind.into());
            let mut member = live.clone();
            member.id = format!("member-{index}");
            member.title = format!("后台成员-{index}");
            member.runtime_command_host = true;
            let records = [live.clone(), member.clone()];
            assert_eq!(resolve_session(&records, alias).unwrap().id, live.id);
            assert_eq!(resolve_session(&records, &member.id).unwrap().id, member.id);
            assert_eq!(
                resolve_sender(&records, Some(&live.id))
                    .unwrap()
                    .unwrap()
                    .session_id,
                live.id
            );
            assert!(resolve_sender(&records, Some("deleted-sender")).is_err());
            assert!(resolve_sender(&records, None).unwrap().is_none());
            let mut duplicate = live.clone();
            duplicate.id = format!("another-{index}");
            assert!(resolve_session(&[live, duplicate, member], alias)
                .unwrap_err()
                .to_string()
                .contains("多个"));
        }
    }

    #[test]
    fn stopped_exact_target_receipt_lists_live_runtime_without_redirecting() {
        let mut stopped = record("old-kimi-id", "旧 Kimi Code");
        stopped.kind = SessionKind::Terminal;
        stopped.agent_runtime_kind = Some("kimi_code".into());
        stopped.status = SessionStatus::Stopped;
        stopped.pty_id = None;
        let mut live = stopped.clone();
        live.id = "current-kimi-id".into();
        live.title = "当前 Kimi Code".into();
        live.pty_id = Some(3);
        live.status = SessionStatus::Running;
        let records = [stopped.clone(), live.clone()];
        let target = resolve_session(&records, &stopped.id).unwrap();
        assert_eq!(
            target.id, stopped.id,
            "explicit historical ID must remain the original target"
        );
        let receipt = send_receipt(&records, &target);
        assert!(receipt.contains("会话 ID：old-kimi-id"));
        assert!(receipt.contains("该目标会话未运行"));
        assert!(receipt.contains("current-kimi-id"));
        assert!(receipt.contains("重新运行 sessions"));
        assert!(receipt.contains("未自动转发"));
        assert_eq!(resolve_session(&records, "kimi").unwrap().id, live.id);
        assert_eq!(resolve_session(&records, "kimi").unwrap().id, live.id);
        let mut other = live.clone();
        other.id = "second-live-kimi-id".into();
        let multiple = [stopped.clone(), live, other];
        let receipt = send_receipt(&multiple, &stopped);
        assert!(receipt.contains("current-kimi-id") && receipt.contains("second-live-kimi-id"));
        assert!(resolve_session(&multiple, "kimi")
            .unwrap_err()
            .to_string()
            .contains("多个"));
    }

    #[test]
    fn stopped_receipt_filters_other_runtime_archived_and_background_candidates() {
        let mut stopped = record("old-id", "旧会话");
        stopped.agent_runtime_kind = Some("kimi_code".into());
        stopped.pty_id = None;
        let mut other = stopped.clone();
        other.id = "other-kind-id".into();
        other.agent_runtime_kind = Some("codex".into());
        other.pty_id = Some(1);
        let mut archived = stopped.clone();
        archived.id = "archived-id".into();
        archived.pty_id = Some(2);
        archived.archived_at = Some(1);
        let mut background = stopped.clone();
        background.id = "background-id".into();
        background.pty_id = Some(3);
        background.runtime_command_host = true;
        let receipt = send_receipt(&[stopped.clone(), other, archived, background], &stopped);
        assert!(receipt.contains("会话 ID：old-id") && receipt.contains("尚未投递"));
        assert!(
            !receipt.contains("other-kind-id")
                && !receipt.contains("archived-id")
                && !receipt.contains("background-id")
        );
    }

    #[test]
    fn runtime_alias_prefers_existing_live_pi_and_never_redirects_exact_target() {
        let mut live = record("live-pi-id", "Pi Agent");
        live.agent_runtime_kind = Some("pi".into());
        let mut background = record("background-pi-id", "Pi · Pi 审查工程师");
        background.agent_runtime_kind = Some("pi".into());
        background.runtime_command_host = true;
        background.multi_agent_agent_id = Some("reviewer".into());
        let records = vec![background.clone(), live.clone()];
        assert_eq!(resolve_session(&records, "pi").unwrap().id, live.id);
        assert_eq!(
            resolve_session(&records, &background.title).unwrap().id,
            background.id
        );
        assert_eq!(
            resolve_session(&records, &background.id).unwrap().id,
            background.id
        );
        let mut other = live.clone();
        other.id = "other-pi-id".into();
        other.title = "另一个Pi".into();
        assert!(
            resolve_session(&[live.clone(), other.clone(), background.clone()], "pi")
                .unwrap_err()
                .to_string()
                .contains("多个")
        );
        other.title = live.title.clone();
        assert!(resolve_session(&[live, other], "Pi Agent").is_err());
        assert!(resolve_session(&[background], "pi")
            .unwrap_err()
            .to_string()
            .contains("没有已打开"));
    }

    #[test]
    fn runtime_pending_hints_are_specific_and_do_not_claim_old_process_has_extension() {
        let mut record = record("target", "Pi Agent");
        record.agent_runtime_kind = Some("pi".into());
        assert!(terminal_pending_hint(&record).unwrap().contains("旧进程"));
        record.runtime_command_host = true;
        assert!(terminal_pending_hint(&record).is_none());
    }

    #[test]
    fn send_args_require_to_and_body() {
        assert_eq!(
            parse_send_args(args(&["--to", "abc123", "--body", "回归一下"])).unwrap(),
            ("abc123".to_string(), "回归一下".to_string())
        );
        assert!(parse_send_args(args(&["--to", "abc123"])).is_err());
        assert!(parse_send_args(args(&["--body", "内容"])).is_err());
        assert!(parse_send_args(args(&["--to", " ", "--body", "内容"])).is_err());
        assert!(parse_send_args(args(&["--verbose"])).is_err());
    }

    #[test]
    fn resolve_session_matches_exact_id_then_prefix_then_title() {
        let records = vec![
            record("abcdef12-0000-0000-0000-000000000001", "需求拆解"),
            record("bbbbbb22-0000-0000-0000-000000000002", "接口检查"),
        ];

        assert_eq!(
            resolve_session(&records, "abcdef12-0000-0000-0000-000000000001")
                .unwrap()
                .title,
            "需求拆解"
        );
        assert_eq!(
            resolve_session(&records, "abcdef12").unwrap().title,
            "需求拆解"
        );
        assert_eq!(
            resolve_session(&records, "接口检查").unwrap().title,
            "接口检查"
        );
        assert!(resolve_session(&records, "不存在的会话").is_err());
    }

    #[test]
    fn resolve_session_rejects_ambiguous_prefix() {
        let records = vec![
            record("aaaa1111-0000-0000-0000-000000000001", "甲"),
            record("aaaa2222-0000-0000-0000-000000000002", "乙"),
        ];

        let error = resolve_session(&records, "aaaa").unwrap_err();

        assert!(error.to_string().contains("不唯一"));
    }

    #[test]
    fn outbox_write_is_ingestible() {
        let dir = std::env::temp_dir().join(format!("crewvela-send-{}", uuid::Uuid::new_v4()));
        let message = OutboxMessage {
            to: "session-b".into(),
            from: Some(MessageOrigin {
                session_id: "session-a".into(),
                label: "需求拆解 (Z.AI GLM)".into(),
            }),
            body: "接口定义发你了".into(),
        };

        write_outbox(&dir, &message).unwrap();

        let entries = std::fs::read_dir(&dir)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        let parsed: OutboxMessage =
            serde_json::from_str(&std::fs::read_to_string(entries[0].path()).unwrap()).unwrap();
        assert_eq!(parsed.to, "session-b");
        assert_eq!(parsed.body, "接口定义发你了");
        assert_eq!(parsed.from.unwrap().session_id, "session-a");

        let _ = std::fs::remove_dir_all(dir);
    }
}

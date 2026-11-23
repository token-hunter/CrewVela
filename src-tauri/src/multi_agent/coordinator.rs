use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use super::{
    contract_requires_session_cli, AgentProfile, AgentRole, AgentRuntimeKind,
    CoordinatorHistoryMessage, CoordinatorIntent, CoordinatorPlan, MultiAgentSnapshot,
    WorkspaceMode,
};

const COORDINATOR_TIMEOUT: Duration = Duration::from_secs(60);
const COORDINATOR_RETRY_TIMEOUT: Duration = Duration::from_secs(60);

struct CoordinatorPromptTransport {
    arguments: Vec<String>,
    stdin_payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorStreamUpdate {
    Reply(String),
    Reset,
}

static COORDINATOR_CANCELLATIONS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    OnceLock::new();

pub struct CoordinatorCancellationToken {
    request_id: Option<String>,
    cancelled: Arc<AtomicBool>,
}

impl CoordinatorCancellationToken {
    pub fn register(request_id: Option<&str>) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let request_id = request_id.map(str::to_owned);
        if let Some(request_id) = request_id.as_ref() {
            COORDINATOR_CANCELLATIONS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .expect("协调器取消状态已损坏")
                .insert(request_id.clone(), cancelled.clone());
        }
        Self {
            request_id,
            cancelled,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Drop for CoordinatorCancellationToken {
    fn drop(&mut self) {
        let Some(request_id) = self.request_id.as_ref() else {
            return;
        };
        let Some(registry) = COORDINATOR_CANCELLATIONS.get() else {
            return;
        };
        let Ok(mut registry) = registry.lock() else {
            return;
        };
        if registry
            .get(request_id)
            .is_some_and(|value| Arc::ptr_eq(value, &self.cancelled))
        {
            registry.remove(request_id);
        }
    }
}

pub fn cancel_coordinator_request(request_id: &str) -> bool {
    COORDINATOR_CANCELLATIONS
        .get()
        .and_then(|registry| registry.lock().ok())
        .and_then(|registry| registry.get(request_id).cloned())
        .is_some_and(|cancelled| {
            cancelled.store(true, Ordering::Relaxed);
            true
        })
}

fn coordinator_prompt_transport(prompt: String) -> CoordinatorPromptTransport {
    CoordinatorPromptTransport {
        arguments: Vec::new(),
        stdin_payload: prompt,
    }
}

fn truncated(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn compact_history(history: &[CoordinatorHistoryMessage]) -> Vec<CoordinatorHistoryMessage> {
    // 前端固定发送最近 10 条（MultiAgentControl.tsx），这里必须整条收下，
    // 否则第 7~10 条会在协调器规划前被静默丢弃。
    history
        .iter()
        .rev()
        .take(10)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|mut item| {
            item.content = truncated(&item.content, 1_200);
            item
        })
        .collect()
}

fn selected_fields(value: &serde_json::Value, fields: &[&str]) -> serde_json::Value {
    let mut selected = serde_json::Map::new();
    for field in fields {
        if let Some(item) = value.get(*field) {
            selected.insert((*field).to_string(), item.clone());
        }
    }
    serde_json::Value::Object(selected)
}

fn compact_project_context(snapshot: Option<&MultiAgentSnapshot>) -> Result<serde_json::Value> {
    let Some(snapshot) = snapshot else {
        return Ok(serde_json::Value::Null);
    };
    let value = serde_json::to_value(snapshot)?;
    let compact_array = |name: &str, fields: &[&str]| {
        value[name]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| selected_fields(item, fields))
            .collect::<Vec<_>>()
    };
    Ok(serde_json::json!({
        "project": selected_fields(&value["project"], &["id", "name", "goal", "rootPath", "defaultBranch", "status"]),
        "agents": compact_array("agents", &["id", "name", "runtimeKind", "role", "description", "model", "permissionProfile", "enabled"]),
        "tasks": compact_array("tasks", &["id", "parentId", "stage", "title", "description", "acceptanceCriteria", "verificationCommand", "status", "assigneeAgentId", "reviewerAgentId", "workspaceMode", "priority", "requiredCapabilities"]),
        "runs": compact_array("runs", &["id", "taskId", "agentId", "status", "attempt", "startedAt", "finishedAt"]),
        "approvals": compact_array("approvals", &["id", "taskId", "runId", "kind", "status", "requestedByAgentId"]),
    }))
}

/// 连续多少次 429 限流重试后放弃本次协调：claude 内部退避可长达数分钟，
/// 快速失败并把真实原因告诉用户，比陪跑超时体验好得多。
const RATE_LIMIT_ABORT_THRESHOLD: u32 = 3;

fn run_coordinator_attempt(
    claude: &Path,
    schema: &str,
    system_prompt: &str,
    user_prompt: String,
    _model: Option<&str>,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
    on_update: &mut dyn FnMut(CoordinatorStreamUpdate),
) -> Result<String> {
    let mut command = crate::title_summary::claude_command(claude)?;
    let prompt = coordinator_prompt_transport(user_prompt);
    command
        .arg("-p")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--verbose")
        .arg("--json-schema")
        .arg(schema)
        .arg("--no-session-persistence")
        .arg("--safe-mode")
        .arg("--tools")
        .arg("")
        .arg("--permission-mode")
        .arg("dontAsk")
        .arg("--max-turns")
        .arg("1")
        .arg("--system-prompt")
        .arg(system_prompt)
        .args(&prompt.arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    run_coordinator_command(command, prompt.stdin_payload, timeout, is_cancelled, on_update)
}

fn run_coordinator_command(
    mut command: std::process::Command,
    stdin_payload: String,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
    on_update: &mut dyn FnMut(CoordinatorStreamUpdate),
) -> Result<String> {
    let started = Instant::now();
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let (mut child, identity) = crate::acp::spawn_owned_command(&mut command).context("启动 Multi-Agent 协调器失败")?;
    let result = (|| -> Result<String> {
    let mut stdin = child.stdin.take().context("Multi-Agent 协调器 stdin 不可用")?;
    // Input can exceed the pipe buffer. Never block the deadline/cancellation
    // owner before stdout/stderr have started draining.
    let stdin_writer = thread::spawn(move || -> std::io::Result<()> {
        stdin.write_all(stdin_payload.as_bytes())?;
        stdin.flush()
    });
    let stdout = child
        .stdout
        .take()
        .context("Multi-Agent 协调器 stdout 不可用")?;
    let stderr = child
        .stderr
        .take()
        .context("Multi-Agent 协调器 stderr 不可用")?;
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let stdout_reader = thread::spawn(move || -> std::io::Result<String> {
        let mut raw = String::new();
        const OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;
        for line in BufReader::new(stdout.take(OUTPUT_LIMIT + 1)).lines() {
            let line = line?;
            raw.push_str(&line);
            raw.push('\n');
            if raw.len() as u64 > OUTPUT_LIMIT { return Err(std::io::Error::other("协调器输出超过 8 MiB 限制")); }
            let _ = line_tx.send(line);
        }
        Ok(raw)
    });
    let stderr_reader = thread::spawn(move || -> std::io::Result<String> {
        let mut raw = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut buffer = [0u8; 8192];
        let mut truncated = false;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 { break; }
            raw.extend_from_slice(&buffer[..count]);
            if raw.len() > 256 * 1024 {
                raw.drain(..raw.len() - 256 * 1024);
                truncated = true;
            }
        }
        Ok(format!("{}{}", if truncated { "[diagnostics truncated; showing last 256 KiB]\n" } else { "" }, String::from_utf8_lossy(&raw)))
    });

    let mut last_progress = started;
    let mut consecutive_rate_limits = 0_u32;
    let mut partial_json = String::new();
    let mut exit_status = None;
    let status = loop {
        for _ in 0..256 {
            let Ok(line) = line_rx.try_recv() else { break; };
            if is_api_retry_line(&line) {
                consecutive_rate_limits += 1;
                if consecutive_rate_limits >= RATE_LIMIT_ABORT_THRESHOLD {
                    return Err(anyhow!(
                        "协调器模型请求被限流（429 rate_limit），请稍后重试或检查模型额度"
                    ));
                }
                continue;
            }
            consecutive_rate_limits = 0;
            last_progress = Instant::now();
            if let Some(reply) = streamed_reply_from_line(&line, &mut partial_json) {
                on_update(CoordinatorStreamUpdate::Reply(reply));
            }
        }
        if is_cancelled() {
            return Err(anyhow!("Multi-Agent 协调器已取消"));
        }
        if exit_status.is_none() { exit_status = crate::acp::try_wait_child_preserving_identity(&mut child)?; }
        if exit_status.is_some() && stdin_writer.is_finished() && stdout_reader.is_finished() && stderr_reader.is_finished() {
            break exit_status.unwrap();
        }
        if coordinator_attempt_timed_out(started.elapsed(), last_progress.elapsed(), timeout) {
            return Err(anyhow!("Multi-Agent 协调器解析超时"));
        }
        thread::sleep(Duration::from_millis(20));
    };
    stdin_writer.join().map_err(|_| anyhow!("写入协调器 Prompt 的线程异常"))?
        .context("写入协调器 Prompt 失败")?;
    let raw = stdout_reader
        .join()
        .map_err(|_| anyhow!("读取 Multi-Agent 协调器输出的线程异常"))?
        .context("读取 Multi-Agent 协调器输出失败")?;
    while let Ok(line) = line_rx.try_recv() {
        if let Some(reply) = streamed_reply_from_line(&line, &mut partial_json) {
            on_update(CoordinatorStreamUpdate::Reply(reply));
        }
    }
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow!("读取 Multi-Agent 协调器错误输出的线程异常"))?
        .context("读取 Multi-Agent 协调器错误输出失败")?;
    if !status.success() {
        let stderr = stderr.trim().to_string();
        return Err(anyhow!(
            "Multi-Agent 协调器失败{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!("：{stderr}")
            }
        ));
    }
    stream_result(&raw)
    })();
    if !identity.is_verified() { crate::acp::stop_child_checked(&mut child)?; }
    child.wait().map_err(|error| anyhow!(crate::acp::UnverifiedStop(format!("等待协调器退出失败：{error}"))))?;
    result
}

fn coordinator_attempt_timed_out(
    total_elapsed: Duration,
    idle_elapsed: Duration,
    idle_timeout: Duration,
) -> bool {
    idle_elapsed >= idle_timeout || total_elapsed >= idle_timeout.saturating_mul(3)
}

fn streamed_reply_from_line(line: &str, partial_json: &mut String) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let delta = value.get("event")?.get("delta")?;
    if delta.get("type")?.as_str()? != "input_json_delta" {
        return None;
    }
    partial_json.push_str(delta.get("partial_json")?.as_str()?);
    extract_partial_json_string(partial_json, "reply")
}

/// claude stream-json 中的限流重试事件：不算进展，连续出现应快速失败，
/// 否则协调器会陪着 claude 的内部退避空等数分钟。
fn is_api_retry_line(line: &str) -> bool {
    line.contains("\"subtype\":\"api_retry\"")
}

fn extract_partial_json_string(input: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\"");
    let after_marker = &input[input.find(&marker)? + marker.len()..];
    let after_colon = after_marker[after_marker.find(':')? + 1..].trim_start();
    let mut chars = after_colon.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut decoded = String::new();
    while let Some(character) = chars.next() {
        match character {
            '"' => break,
            '\\' => {
                let Some(escaped) = chars.next() else { break };
                match escaped {
                    '"' | '\\' | '/' => decoded.push(escaped),
                    'b' => decoded.push('\u{0008}'),
                    'f' => decoded.push('\u{000c}'),
                    'n' => decoded.push('\n'),
                    'r' => decoded.push('\r'),
                    't' => decoded.push('\t'),
                    'u' => {
                        let digits = chars.by_ref().take(4).collect::<String>();
                        if digits.len() != 4 {
                            break;
                        }
                        if let Ok(value) = u16::from_str_radix(&digits, 16) {
                            if let Some(value) = char::from_u32(u32::from(value)) {
                                decoded.push(value);
                            }
                        }
                    }
                    _ => break,
                }
            }
            value => decoded.push(value),
        }
    }
    Some(decoded)
}

fn stream_result(raw: &str) -> Result<String> {
    let value = raw.lines().rev()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value.get("type").and_then(serde_json::Value::as_str) == Some("result"))
        .ok_or_else(|| anyhow!("协调器流式输出缺少最终结果"))?;
    if value.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(anyhow!("协调器返回错误：{}", value.get("result").unwrap_or(&value)));
    }
    if let Some(structured) = value.get("structured_output").filter(|output| !output.is_null()) {
        return Ok(serde_json::to_string(structured)?);
    }
    value.get("result").and_then(serde_json::Value::as_str)
        .filter(|result| !result.trim().is_empty()).map(str::to_owned)
        .ok_or_else(|| anyhow!("协调器流式输出的最终结果为空"))
}

pub fn requested_agent_runtimes(message: &str) -> Vec<AgentRuntimeKind> {
    mentioned_agent_runtimes(message, false)
}

pub fn explicitly_mentioned_agent_runtimes(message: &str) -> Vec<AgentRuntimeKind> {
    mentioned_agent_runtimes(message, true)
}

fn mentioned_agent_runtimes(message: &str, explicit_only: bool) -> Vec<AgentRuntimeKind> {
    let normalized = message.to_ascii_lowercase();
    let aliases = [
        (
            AgentRuntimeKind::ClaudeCode,
            [
                "@claudecode",
                "@claude-code",
                "@claude_code",
                "@claude",
                "claude code",
                "claudecode",
                "claude-code",
                "claude_code",
            ]
            .as_slice(),
        ),
        (AgentRuntimeKind::Codex, ["@codex", "codex"].as_slice()),
        (
            AgentRuntimeKind::KimiCode,
            [
                "@kimicode",
                "@kimi-code",
                "@kimi_code",
                "@kimi",
                "kimi code",
                "kimicode",
            ]
            .as_slice(),
        ),
        (AgentRuntimeKind::Pi, ["@pi", "pi agent"].as_slice()),
        (
            AgentRuntimeKind::OpenCode,
            ["@opencode", "opencode"].as_slice(),
        ),
        (AgentRuntimeKind::ZCode, ["@zcode", "zcode"].as_slice()),
        (
            AgentRuntimeKind::ZCodeCli,
            ["@zcodecli", "@zcode_cli", "@zcode-cli", "zcode cli", "zcodecli", "zcode_cli", "zcode-cli"].as_slice(),
        ),
        (
            AgentRuntimeKind::DeepSeekHarness,
            ["@deepseek_harness", "deepseek harness", "@dsh"].as_slice(),
        ),
    ];
    // Select the longest alias at each position before deduplicating runtimes:
    // `zcode cli` must not also select its `zcode` prefix.
    let mut matches = aliases.into_iter().flat_map(|(runtime, names)| {
        names.iter().filter(|alias| !explicit_only || alias.starts_with('@')).flat_map(|alias| normalized.match_indices(alias)
            .filter_map(|(position, _)| {
                mention_matches_at(&normalized, alias, position)
                    .then_some((position, alias.len(), runtime))
            })).collect::<Vec<_>>()
    }).collect::<Vec<_>>();
    matches.sort_by_key(|(position, length, _)| (*position, std::cmp::Reverse(*length)));
    let mut mentioned = Vec::new();
    let mut covered_until = 0;
    for (position, length, runtime) in matches {
        if position < covered_until { continue; }
        covered_until = position + length;
        if !mentioned.contains(&runtime) { mentioned.push(runtime); }
    }
    mentioned

}

fn mention_matches_at(message: &str, alias: &str, position: usize) -> bool {
        let prefix = &message[..position];
        let suffix = &message[position + alias.len()..];
        let continues_from_identifier = prefix.chars().next_back().is_some_and(|character| {
            if alias.starts_with('@') {
                character.is_alphanumeric() || matches!(character, '_' | '-' | '.')
            } else {
                character.is_ascii_alphanumeric() || matches!(character, '@' | '_' | '-' | '.')
            }
        });
        let continues_identifier = suffix.chars().next().is_some_and(|character| {
            if alias.starts_with('@') {
                character.is_alphanumeric() || matches!(character, '_' | '-')
            } else {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
            }
        });
        let excluded = !alias.starts_with('@')
            && prefix
                .rsplit(['，', ',', '。', ';', '；', '\n'])
                .next()
                .is_some_and(|clause| {
                    [
                        "不需要",
                        "不要",
                        "不使用",
                        "不用",
                        "无需",
                        "without",
                        "exclude",
                        "don't use",
                    ]
                    .iter()
                    .any(|word| clause.contains(word))
                });
        !continues_from_identifier && !continues_identifier && !excluded
}

pub fn interpret_collaboration_message_streaming(
    message: &str,
    history: &[CoordinatorHistoryMessage],
    snapshot: Option<&MultiAgentSnapshot>,
    target_agent_ids: &[String],
    is_cancelled: impl Fn() -> bool,
    mut on_update: impl FnMut(CoordinatorStreamUpdate),
) -> Result<CoordinatorPlan> {
    let _coordinator_lease = crate::runtime_preferences::acquire(AgentRuntimeKind::ClaudeCode)?;
    let message = message.trim();
    if message.is_empty() {
        return Err(anyhow!("项目协作消息不能为空"));
    }
    let requested_runtimes = requested_agent_runtimes(message);
    for runtime in &requested_runtimes {
        runtime.ensure_available()?;
    }
    let target_agents = resolve_target_agents(snapshot, target_agent_ids)?;
    for agent in &target_agents {
        agent.runtime_kind.ensure_available()?;
    }
    let missing_group_runtimes = requested_runtimes
        .iter()
        .copied()
        .filter(|runtime| {
            snapshot.map_or(true, |snapshot| {
                !snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.enabled && agent.runtime_kind == *runtime)
            })
        })
        .collect::<Vec<_>>();
    let mut dispatch_target_agents = target_agents.clone();
    if !dispatch_target_agents.is_empty() {
        if let Some(snapshot) = snapshot {
            for agent in snapshot
                .agents
                .iter()
                .filter(|agent| agent.enabled && requested_runtimes.contains(&agent.runtime_kind))
            {
                if !dispatch_target_agents
                    .iter()
                    .any(|item| item.id == agent.id)
                {
                    dispatch_target_agents.push(agent);
                }
            }
        }
    }
    let mut constrained_runtimes = requested_runtimes.clone();
    for agent in &target_agents {
        if !constrained_runtimes.contains(&agent.runtime_kind) {
            constrained_runtimes.push(agent.runtime_kind);
        }
    }

    let project_context = compact_project_context(snapshot)?;
    let recent_history = compact_history(history);

    let system_prompt = r#"你是 CrewVela Multi-Agent 项目协调器。你只生成结构化、可审计的控制计划，不直接修改文件或声称任务已经执行。

新成员 Runtime 只有七个 CLI 执行引擎：claude_code、codex、kimi_code、pi、opencode、deepseek_harness（dsh）、zcode_cli；角色只有 orchestrator、worker、reviewer、integrator。zcode 仅兼容当前项目已有 ZCode Desktop 成员，使用时必须填写对应 existingAgentId，不得新建或自动替换为 zcode_cli。独立 Chat 界面不是新的协作成员类型。

规则：
1. 新项目需求使用 plan_project；向已有项目追加任务使用 add_tasks；启动、暂停、恢复、取消、查看分别使用 start_project、pause_project、resume_project、cancel_project、inspect_project；重试任务、重派任务、批准任务分别使用 retry_task、reassign_task、approve_task。
2. plan_project/add_tasks 必须生成可执行任务图。每个 key 唯一；dependsOn 只能引用本计划中的 task key；不得循环依赖。
3. 会写代码的并行任务默认 workspaceMode=worktree；只读研究/审查使用 read_only；只有明确要求同目录协作才使用 shared_write。
4. assigneeKey/reviewerKey 必须引用 agents 中的 key。Agent 审查必须建成一个独立任务并依赖被审任务；reviewerKey 只表示任务完成后还需要人工批准，通常设为 null。
5. 把验收要求写进 acceptanceCriteria，不允许用“完成即可”这类不可验证措辞。能用命令验证的编码任务必须给 verificationCommand（例如 npm test、cargo test 或针对性检查）；纯设计/研究任务可为 null，届时进入人工审批而不是自动完成。
6. permissionProfile 只能是 default、auto、yolo；没有明确指定时用 auto。YOLO 仅在用户明确要求时使用。
7. 用户明确指定 Runtime 的职责（例如 Codex 后端、Claude Code 前端）时，必须按指定职责生成对应任务；现有成员的名称/模板职责不能覆盖用户分工。描述中明确文件/模块边界和接口协商，先协商接口，再并行实现与交叉验证，最终集成交付。只使用实际需要的 Agent，不为凑数强行加入全部 Runtime；按用户要求组合当前可用的 Runtime。
8. plan_project/add_tasks 的 requiresConfirmation=true；纯查询或运行控制为 false。
9. reply 用简短中文说明本次计划或动作，不使用 Markdown。
10. 无法可靠判断时 intent=unknown，agents/tasks 为空，不猜测。
11. retry_task/approve_task 必须从项目快照选择精确 targetTaskId；reassign_task 还必须选择精确 targetAgentId。不能按标题伪造 ID。
12. 多个 Agent 并行写代码时，必须创建最后的集成任务：由 integrator 角色执行，依赖全部实现/测试任务，在独立 Worktree 汇总交付并运行总体验收；该任务完成后由用户批准，CrewVela 才合并回主工作区。
13. plan_project 的 projectName 和 goal 必须是非空字符串；不得返回字符串 "null"、"undefined" 或其他占位词。
14. 用户要求 Agent 互相聊天、通信、发消息、问候或打招呼时，必须使用 crewvela Skill 和对应 CLI：先通过 CREWVELA_CLI sessions 发现目标，再执行 CREWVELA_CLI send --to <目标> --body <完整消息>；接收方按注入消息中的回复命令异步回复。仅纯聊天/问候任务的 Agent role 必须为 orchestrator，workspaceMode 必须为 read_only（纯通信不写文件，且兼容各 Runtime 的 Auto 权限限制，shared_write 会被部分 Runtime 拒绝），开发任务同时需要通信时仍使用 worker/reviewer/integrator 与 worktree，不得降为只读问候任务。确保 Runtime 可以执行 CLI，且不会被误判为代码实现任务。不得用共享文件或 cc_message_send/cc_thread_read 模拟会话通信，也不得生成无关代码文件或验收命令。
"#;

    let system_prompt = format!(
        "{system_prompt}\n14. plan_project 必须从用户消息中提取明确的绝对项目目录并写入 rootPath；没有明确目录时设为 null。涉及前后端并行开发时，必须创建独立的交叉 Review 任务：前端 Agent 审查后端接口契约，后端 Agent 审查前端调用与类型，Review 任务依赖相应实现任务，修复或集成任务再依赖 Review。Review Agent 的 role 必须是 reviewer，workspaceMode=read_only，reviewerKey 通常为 null。"
    );
    let system_prompt = format!(
        "{system_prompt}\n16. 任务可用 requiredCapabilities 声明硬能力要求，只能从 filesystem、shell、resume、interrupt、mcp、approvals、usage、diff、commands、subagents、permission_modes 中选择；没有硬要求时返回空数组。commands 仅表示任务明确要求 Runtime 的斜杠/自定义命令发现能力，执行 shell、CLI、crewvela Skill 或 Agent 通信不能标记 commands。maxAttempts 默认 3，范围 1-10；retryBackoffSeconds 默认 5，范围 1-3600。不要把偏好写成硬能力。"
    );
    let system_prompt = if requested_runtimes.is_empty() {
        system_prompt
    } else {
        let requested = requested_runtimes
            .iter()
            .map(|runtime| runtime.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{system_prompt}\n17. 用户通过 Runtime 群组 @Agent 明确指定了参与 Runtime：{requested}。这是硬性约束：每种群组 Runtime 至少出现一次；不得自行遗漏。若还存在实例级 @，允许的 Agent 是这些 Runtime 群组与规则18所列实例的并集。若当前项目中已经存在某个群组 Runtime 的启用 Agent，add_tasks 必须复用该 Runtime 的全部现有实例，并为每个实例填写 existingAgentId、至少分配一个任务，不得用新 Agent 替换；某个群组 Runtime 没有现有实例时才允许为它新建 Agent。plan_project 按群组 Runtime 创建新 Agent。为多 Runtime 项目建立清晰的实现、Review、修复/集成依赖，并让最终任务汇总交付结果。"
        )
    };
    let system_prompt = if dispatch_target_agents.is_empty() {
        system_prompt
    } else {
        let selected = dispatch_target_agents
            .iter()
            .map(|agent| {
                serde_json::json!({
                    "id": agent.id,
                    "name": agent.name,
                    "runtimeKind": agent.runtime_kind,
                    "role": agent.role,
                    "description": agent.description,
                    "model": agent.model,
                    "permissionProfile": agent.permission_profile,
                })
            })
            .collect::<Vec<_>>();
        let creatable_runtime_groups = if missing_group_runtimes.is_empty() {
            "无".to_string()
        } else {
            missing_group_runtimes
                .iter()
                .map(|runtime| runtime.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "{system_prompt}\n18. 用户通过实例级 @ 选择了现有 Agent；若同时存在 Runtime 群组 @，现有目标是实例选择与该群组全部现有实例的并集：{}。追加任务时必须逐个返回这些现有 Agent，并把 existingAgentId 设置为对应 id，每个目标 Agent 至少分配一个任务。只有当前没有任何启用实例的显式群组 Runtime 才允许新建 Agent，本次允许新建的 Runtime：{creatable_runtime_groups}；除此之外不得新建或替换 Agent。实例级 @ 不能用于创建新项目。",
            serde_json::to_string(&selected)?,
        )
    };

    let mut execution_runtimes = super::COLLABORATION_CLI_RUNTIMES.iter()
        .filter(|runtime| **runtime != AgentRuntimeKind::Antigravity)
        .map(|runtime| runtime.as_str()).collect::<Vec<_>>();
    if snapshot.is_some_and(|project| project.agents.iter().any(|agent| agent.runtime_kind == AgentRuntimeKind::ZCode)) { execution_runtimes.push("zcode"); }
    let schema = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "intent": {"type": "string", "enum": ["plan_project", "add_tasks", "start_project", "pause_project", "resume_project", "cancel_project", "inspect_project", "retry_task", "reassign_task", "approve_task", "unknown"]},
            "reply": {"type": "string"},
            "projectName": {"type": ["string", "null"]},
            "goal": {"type": ["string", "null"]},
            "rootPath": {"type": ["string", "null"]},
            "targetTaskId": {"type": ["string", "null"]},
            "targetAgentId": {"type": ["string", "null"]},
            "requiresConfirmation": {"type": "boolean"},
            "agents": {
                "type": "array",
                "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {
                        "key": {"type": "string"}, "name": {"type": "string"},
                        "runtimeKind": {"type": "string", "enum": execution_runtimes},
                        "role": {"type": "string", "enum": ["orchestrator", "worker", "reviewer", "integrator"]},
                        "description": {"type": "string"}, "model": {"type": ["string", "null"]},
                        "permissionProfile": {"type": "string", "enum": ["default", "auto", "yolo"]},
                        "existingAgentId": {"type": ["string", "null"]}
                    },
                    "required": ["key", "name", "runtimeKind", "role", "description", "model", "permissionProfile", "existingAgentId"]
                }
            },
            "tasks": {
                "type": "array",
                "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {
                        "key": {"type": "string"}, "stage": {"type": "integer", "minimum": 0, "maximum": 99},
                        "title": {"type": "string"}, "description": {"type": "string"},
                        "acceptanceCriteria": {"type": "string"}, "assigneeKey": {"type": "string"},
                        "verificationCommand": {"type": ["string", "null"]},
                        "reviewerKey": {"type": ["string", "null"]},
                        "dependsOn": {"type": "array", "items": {"type": "string"}},
                        "workspaceMode": {"type": "string", "enum": ["read_only", "worktree", "shared_write"]},
                        "priority": {"type": "integer", "minimum": -100, "maximum": 100},
                        "requiredCapabilities": {
                            "type": "array",
                            "items": {"type": "string", "enum": ["filesystem", "shell", "resume", "interrupt", "mcp", "approvals", "usage", "diff", "commands", "subagents", "permission_modes"]}
                        },
                        "maxAttempts": {"type": "integer", "minimum": 1, "maximum": 10},
                        "retryBackoffSeconds": {"type": "integer", "minimum": 1, "maximum": 3600}
                    },
                    "required": ["key", "stage", "title", "description", "acceptanceCriteria", "verificationCommand", "assigneeKey", "reviewerKey", "dependsOn", "workspaceMode", "priority", "requiredCapabilities", "maxAttempts", "retryBackoffSeconds"]
                }
            }
        },
        "required": ["intent", "reply", "projectName", "goal", "rootPath", "targetTaskId", "targetAgentId", "agents", "tasks", "requiresConfirmation"]
    })
    .to_string();
    let user_prompt = format!(
        "当前项目快照：\n{}\n\n最近对话：\n{}\n\n用户消息：\n{}",
        serde_json::to_string(&project_context)?,
        serde_json::to_string(&recent_history)?,
        message,
    );

    let claude = crate::launcher::resolve_claude()?;
    let raw = match run_coordinator_attempt(
        &claude,
        &schema,
        &system_prompt,
        user_prompt.clone(),
        None,
        COORDINATOR_TIMEOUT,
        &is_cancelled,
        &mut on_update,
    ) {
        Ok(raw) => raw,
        Err(first_error) => {
            if is_cancelled() {
                return Err(anyhow!("Multi-Agent 协调器已取消"));
            }
            on_update(CoordinatorStreamUpdate::Reset);
            let retry_prompt = format!(
                "首次协调请求失败：{}。请忽略此前失败，严格按 JSON Schema 直接给出计划，不要解释。\n\n{}",
                truncated(&first_error.to_string(), 400),
                user_prompt
            );
            run_coordinator_attempt(
                &claude,
                &schema,
                &system_prompt,
                retry_prompt,
                Some("haiku"),
                COORDINATOR_RETRY_TIMEOUT,
                &is_cancelled,
                &mut on_update,
            )
            .map_err(|retry_error| {
                anyhow!(
                    "Multi-Agent 协调器两次解析均失败：首次 {first_error:#}；重试 {retry_error:#}"
                )
            })?
        }
    };
    let mut plan = parse_plan(&raw)?;
    for agent in &plan.agents {
        agent.runtime_kind.ensure_available()?;
    }
    if communication_only_requested(message) {
        normalize_communication_workspaces(&mut plan);
    }
    add_project_communication_contract(&mut plan, snapshot);
    validate_communication_plan(message, &plan)?;
    validate_requested_runtimes(&plan, &constrained_runtimes)?;
    validate_explicit_runtime_roles(message, &plan)?;
    let mut effective_target_agent_ids = target_agent_ids.to_vec();
    if plan.intent == CoordinatorIntent::AddTasks {
        if let Some(snapshot) = snapshot {
            for agent in snapshot
                .agents
                .iter()
                .filter(|agent| agent.enabled && requested_runtimes.contains(&agent.runtime_kind))
            {
                if !effective_target_agent_ids.contains(&agent.id) {
                    effective_target_agent_ids.push(agent.id.clone());
                }
            }
        }
    }
    let allowed_new_runtimes = if plan.intent == CoordinatorIntent::AddTasks {
        missing_group_runtimes
    } else {
        Vec::new()
    };
    validate_target_agents(&plan, &effective_target_agent_ids, &allowed_new_runtimes)?;
    plan.requested_runtimes = constrained_runtimes;
    Ok(plan)
}

pub fn communication_plan_requested(message: &str) -> bool {
    if super::direct_message::direct_message_request(message).is_some() {
        return true;
    }
    let normalized = message.to_ascii_lowercase();
    [
        "聊天",
        "通信",
        "发消息",
        "问候",
        "打招呼",
        "打个招呼",
        "message",
    ]
    .iter()
    .any(|k| normalized.contains(k))
}

fn validate_explicit_runtime_roles(message: &str, plan: &CoordinatorPlan) -> Result<()> {
    if !matches!(
        plan.intent,
        CoordinatorIntent::PlanProject | CoordinatorIntent::AddTasks
    ) {
        return Ok(());
    }
    for clause in message.split(['，', ',', '；', ';', '\n']) {
        if !["负责", "服务", "承担"]
            .iter()
            .any(|word| clause.contains(word))
        {
            continue;
        }
        let runtimes = requested_agent_runtimes(clause);
        if runtimes.len() != 1 {
            continue;
        }
        for responsibility in ["前端", "后端"] {
            if !clause.contains(responsibility) {
                continue;
            }
            let assigned = plan.tasks.iter().any(|task| {
                plan.agents.iter().any(|agent| {
                    agent.key == task.assignee_key && agent.runtime_kind == runtimes[0]
                }) && task.workspace_mode != WorkspaceMode::ReadOnly
                    && !task.title.contains("审查")
                    && !task.title.to_ascii_lowercase().contains("review")
                    && std::iter::once(task.title.as_str())
                        .chain(task.description.lines())
                        .any(|line| {
                            [
                                format!("负责{responsibility}"),
                                format!("承担{responsibility}"),
                                format!("实现{responsibility}"),
                            ]
                            .iter()
                            .any(|owned| line.contains(owned))
                                || (line.contains(responsibility)
                                    && !["同伴", "另一 Agent", "另一Agent", "对方"]
                                        .iter()
                                        .any(|word| line.contains(word)))
                        })
            });
            if !assigned {
                return Err(anyhow!(
                    "计划未遵循用户分工：{} 必须承担{responsibility}任务",
                    runtimes[0].as_str()
                ));
            }
        }
    }
    Ok(())
}

fn communication_only_requested(message: &str) -> bool {
    communication_plan_requested(message)
        && ![
            "开发",
            "实现",
            "修复",
            "重构",
            "编写",
            "完成",
            "制作",
            "做一个",
            "做个",
            "develop",
            "implement",
            "build",
        ]
        .iter()
        .any(|word| message.to_ascii_lowercase().contains(word))
}

fn add_project_communication_contract(
    plan: &mut CoordinatorPlan,
    snapshot: Option<&MultiAgentSnapshot>,
) {
    let mut runtimes = plan
        .agents
        .iter()
        .map(|agent| agent.runtime_kind.as_str())
        .collect::<HashSet<_>>();
    if let Some(snapshot) = snapshot {
        runtimes.extend(
            snapshot
                .agents
                .iter()
                .filter(|agent| agent.enabled && agent.runtime_kind != AgentRuntimeKind::Antigravity)
                .map(|agent| agent.runtime_kind.as_str()),
        );
    }
    if runtimes.len() < 2 {
        return;
    }
    for task in &mut plan.tasks {
        if !contract_requires_session_cli(&task.description, &task.acceptance_criteria) {
            task.description.push_str("\n跨运行时协作：使用 crewvela Skill，通过 CREWVELA_CLI sessions 查询同 projectId 的指定成员原会话，再 send --to <完整ID> --body <接口契约、进展或阻塞问题>。只在当前任务范围内实现和验证；不能用寒暄代替开发交付，也不能用共享消息文件模拟通信。不要同步等待或轮询回信，必要结果异步到达。只有实质问题和必要协调结果才回复，不对确认或结束语再次回信。");
        }
    }
}

fn normalize_communication_workspaces(plan: &mut CoordinatorPlan) {
    let roles = plan
        .agents
        .iter()
        .map(|agent| (agent.key.as_str(), agent.role))
        .collect::<HashMap<_, _>>();
    for task in &mut plan.tasks {
        if roles.get(task.assignee_key.as_str()) == Some(&AgentRole::Orchestrator)
            && contract_requires_session_cli(&task.description, &task.acceptance_criteria)
        {
            // 纯通信不写文件：read_only 已足够（非 Git 项目落在项目根目录，PTY 路径注入
            // CREWVELA_CLI），且能通过 OpenCode 等 Runtime 的 Auto 权限隔离检查；
            // shared_write 会被这些 Runtime 确定性拒绝。
            task.workspace_mode = WorkspaceMode::ReadOnly;
        }
    }
}
fn validate_communication_plan(message: &str, plan: &CoordinatorPlan) -> Result<()> {
    if !communication_only_requested(message)
        || !matches!(
            plan.intent,
            CoordinatorIntent::PlanProject | CoordinatorIntent::AddTasks
        )
    {
        return Ok(());
    }
    let contract = plan
        .tasks
        .iter()
        .flat_map(|task| [&task.description, &task.acceptance_criteria])
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if !contract.contains("CREWVELA_CLI")
        || !contract.contains("sessions")
        || !contract.contains("send --to")
        || !contract.contains("--body")
    {
        return Err(anyhow!(
            "Agent 通信计划必须使用 crewvela Skill 和 CREWVELA_CLI sessions/send"
        ));
    }
    let roles = plan
        .agents
        .iter()
        .map(|agent| (agent.key.as_str(), agent.role))
        .collect::<HashMap<_, _>>();
    if plan
        .tasks
        .iter()
        .any(|task| roles.get(task.assignee_key.as_str()).copied() != Some(AgentRole::Orchestrator))
    {
        return Err(anyhow!("Agent 通信任务必须使用 orchestrator 角色"));
    }
    Ok(())
}
fn resolve_target_agents<'a>(
    snapshot: Option<&'a MultiAgentSnapshot>,
    target_agent_ids: &[String],
) -> Result<Vec<&'a AgentProfile>> {
    if target_agent_ids.is_empty() {
        return Ok(Vec::new());
    }
    let snapshot = snapshot.ok_or_else(|| anyhow!("精确 @Agent 只能用于当前协作项目"))?;
    let mut seen = HashSet::new();
    target_agent_ids
        .iter()
        .filter(|agent_id| seen.insert((*agent_id).clone()))
        .map(|agent_id| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.id == *agent_id && agent.enabled)
                .ok_or_else(|| {
                    anyhow!("精确 @ 的 Agent 不存在、已禁用或不属于当前项目：{agent_id}")
                })
        })
        .collect()
}

fn runtime_label(runtime: AgentRuntimeKind) -> &'static str {
    match runtime {
        AgentRuntimeKind::ClaudeCode => "Claude Code",
        AgentRuntimeKind::Codex => "Codex",
        AgentRuntimeKind::KimiCode => "Kimi Code",
        AgentRuntimeKind::MiniMaxCode | AgentRuntimeKind::Unsupported => "已停用的 CLI",
        AgentRuntimeKind::Pi => "Pi",
        AgentRuntimeKind::OhMyPi => "Oh My Pi（已移除）",
        AgentRuntimeKind::OpenCode => "OpenCode",
        AgentRuntimeKind::DeepSeekHarness => "DeepSeek Harness",
        AgentRuntimeKind::ClaudeChat => "Claude Code Chat",
        AgentRuntimeKind::KimiChat => "Kimi Code Chat",
        AgentRuntimeKind::PiChat => "Pi Chat",
        AgentRuntimeKind::CodexChat => "Codex Chat",
        AgentRuntimeKind::OpenCodeChat => "OpenCode Chat",
        AgentRuntimeKind::ZCode => "ZCode Desktop",
        AgentRuntimeKind::ZCodeCli => "ZCode CLI",
        AgentRuntimeKind::Antigravity => "Antigravity CLI",
    }
}

fn validate_requested_runtimes(
    plan: &CoordinatorPlan,
    requested: &[AgentRuntimeKind],
) -> Result<()> {
    if requested.is_empty()
        || !matches!(
            plan.intent,
            super::CoordinatorIntent::PlanProject | super::CoordinatorIntent::AddTasks
        )
    {
        return Ok(());
    }
    let missing = requested
        .iter()
        .copied()
        .filter(|runtime| {
            !plan
                .agents
                .iter()
                .any(|agent| agent.runtime_kind == *runtime)
        })
        .map(runtime_label)
        .collect::<Vec<_>>();
    let unexpected = plan
        .agents
        .iter()
        .map(|agent| agent.runtime_kind)
        .filter(|runtime| !requested.contains(runtime))
        .map(runtime_label)
        .collect::<Vec<_>>();
    let unassigned = requested
        .iter()
        .copied()
        .filter(|runtime| {
            let agent_keys = plan
                .agents
                .iter()
                .filter(|agent| agent.runtime_kind == *runtime)
                .map(|agent| agent.key.as_str())
                .collect::<Vec<_>>();
            !agent_keys.is_empty()
                && !plan
                    .tasks
                    .iter()
                    .any(|task| agent_keys.contains(&task.assignee_key.as_str()))
        })
        .map(runtime_label)
        .collect::<Vec<_>>();
    if missing.is_empty() && unexpected.is_empty() && unassigned.is_empty() {
        return Ok(());
    }
    Err(anyhow!(
        "协调器未遵守 @Agent 约束{}{}{}，请重新生成计划",
        if missing.is_empty() {
            String::new()
        } else {
            format!("；缺少：{}", missing.join("、"))
        },
        if unexpected.is_empty() {
            String::new()
        } else {
            format!("；包含未指定 Agent：{}", unexpected.join("、"))
        },
        if unassigned.is_empty() {
            String::new()
        } else {
            format!("；未分配任务：{}", unassigned.join("、"))
        }
    ))
}

fn validate_target_agents(
    plan: &CoordinatorPlan,
    selected: &[String],
    allowed_new_runtimes: &[AgentRuntimeKind],
) -> Result<()> {
    if selected.is_empty() || plan.intent != CoordinatorIntent::AddTasks {
        if !selected.is_empty() && plan.intent == CoordinatorIntent::PlanProject {
            return Err(anyhow!(
                "实例级 @Agent 只能用于当前项目追加任务，不能创建新项目"
            ));
        }
        return Ok(());
    }

    let selected = selected.iter().map(String::as_str).collect::<HashSet<_>>();
    let planned = plan
        .agents
        .iter()
        .filter_map(|agent| agent.existing_agent_id.as_deref())
        .collect::<HashSet<_>>();
    let missing = selected.difference(&planned).copied().collect::<Vec<_>>();
    let unexpected = planned.difference(&selected).copied().collect::<Vec<_>>();
    let creates_unexpected_new_agent = plan.agents.iter().any(|agent| {
        agent.existing_agent_id.is_none() && !allowed_new_runtimes.contains(&agent.runtime_kind)
    });
    let unassigned = plan
        .agents
        .iter()
        .filter(|agent| {
            agent
                .existing_agent_id
                .as_deref()
                .is_some_and(|agent_id| selected.contains(agent_id))
                && !plan.tasks.iter().any(|task| task.assignee_key == agent.key)
        })
        .filter_map(|agent| agent.existing_agent_id.as_deref())
        .collect::<Vec<_>>();

    if missing.is_empty()
        && unexpected.is_empty()
        && !creates_unexpected_new_agent
        && unassigned.is_empty()
    {
        return Ok(());
    }
    Err(anyhow!(
        "协调器未遵守实例级 @Agent 约束{}{}{}{}",
        if missing.is_empty() {
            String::new()
        } else {
            format!("；缺少：{}", missing.join("、"))
        },
        if unexpected.is_empty() {
            String::new()
        } else {
            format!("；包含未选择实例：{}", unexpected.join("、"))
        },
        if creates_unexpected_new_agent {
            "；错误地创建了未指定的新 Agent".to_string()
        } else {
            String::new()
        },
        if unassigned.is_empty() {
            String::new()
        } else {
            format!("；未分配任务：{}", unassigned.join("、"))
        },
    ))
}

fn parse_plan(raw: &str) -> Result<CoordinatorPlan> {
    let root: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|error| {
        anyhow!(
            "解析协调器 JSON 输出失败：{error}；输出片段：{}",
            text_excerpt(raw)
        )
    })?;
    if let Some(value) = root.get("structured_output") {
        return parse_plan_value(value, "解析结构化协作计划失败");
    }
    if let Some(value) = root.get("result").and_then(|value| value.as_str()) {
        let value: serde_json::Value = serde_json::from_str(value).map_err(|error| {
            anyhow!(
                "解析协作计划文本失败：{error}；输出片段：{}",
                text_excerpt(value)
            )
        })?;
        return parse_plan_value(&value, "解析协作计划文本失败");
    }
    parse_plan_value(&root, "解析协作计划失败")
}

fn parse_plan_value(value: &serde_json::Value, context: &str) -> Result<CoordinatorPlan> {
    let plan: CoordinatorPlan = serde_json::from_value(value.clone()).map_err(|error| {
        anyhow!(
            "{context}：{error}；输出片段：{}",
            text_excerpt(&value.to_string())
        )
    })?;
    // Historical records tolerate removed kinds; new execution plans must not.
    for (index, agent) in plan.agents.iter().enumerate() {
        if agent.runtime_kind == AgentRuntimeKind::Antigravity {
            bail!("{context}：Antigravity CLI 目前仅支持团队普通对话");
        }
        if agent.runtime_kind.is_retired() {
            bail!("{context}：agents[{index}].runtimeKind 不受支持：{}；输出片段：{}",
                value["agents"][index]["runtimeKind"], text_excerpt(&value.to_string()));
        }
        if !agent.runtime_kind.is_collaboration_cli()
            && !(agent.runtime_kind == AgentRuntimeKind::ZCode && agent.existing_agent_id.as_deref().is_some_and(|id| !id.trim().is_empty())) {
            bail!("{context}：新增协作成员仅支持七个 CLI；ZCode Desktop 只能引用已有成员，独立 Chat 不用于协作执行");
        }
    }
    if plan.requested_runtimes.iter().any(|kind| kind.is_retired()) {
        bail!("{context}：requestedRuntimes 包含已移除或未知 Runtime；输出片段：{}", text_excerpt(&value.to_string()));
    }
    Ok(plan)
}

fn text_excerpt(value: &str) -> String {
    const MAX_CHARS: usize = 600;
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut excerpt = compact.chars().take(MAX_CHARS).collect::<String>();
    if compact.chars().count() > MAX_CHARS {
        excerpt.push('…');
    }
    excerpt
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn coordinator_success_stops_grandchild_after_root_and_tool_exit() {
        crate::title_summary::assert_runner_stops_inherited_child(false, |command| {
            let result = super::run_coordinator_command(command, String::new(), std::time::Duration::from_secs(10), &|| false, &mut |_| {})?;
            assert_eq!(serde_json::from_str::<serde_json::Value>(&result)?["reply"], "done");
            Ok(())
        });
    }

    #[test]
    fn directed_greeting_with_classifier_routes_to_communication() {
        assert!(super::communication_plan_requested(
            "@OpenCode设计师 @Pi审查工程师 你两打个招呼"
        ));
        assert!(!super::communication_plan_requested(
            "@OpenCode设计师 实现首页"
        ));
    }
    #[test]
    fn structured_stream_result_and_longest_runtime_aliases() {
        let plan = serde_json::json!({"reply":"完成", "tasks":[]});
        let envelope = serde_json::json!({"type":"result","result":"","structured_output":plan});
        assert_eq!(serde_json::from_str::<serde_json::Value>(&super::stream_result(&envelope.to_string()).unwrap()).unwrap(), plan);
        assert!(super::stream_result(r#"{"type":"result","result":"bad","is_error":true}"#).is_err());
        assert!(super::stream_result(r#"{"type":"result","result":""}"#).is_err());
        assert_eq!(super::requested_agent_runtimes("使用 zcode cli"), vec![AgentRuntimeKind::ZCodeCli]);
        assert_eq!(super::requested_agent_runtimes("使用 zcode cli，再使用 zcode"), vec![AgentRuntimeKind::ZCodeCli, AgentRuntimeKind::ZCode]);
        assert_eq!(super::requested_agent_runtimes("使用 @zcode_cli"), vec![AgentRuntimeKind::ZCodeCli]);
    }

    #[test]
    fn coordinator_drains_output_while_writing_large_literal_prompt() {
        let prompt = "纯文本 & % ! () 中文\n".repeat(100_000);
        let mut command = std::process::Command::new(which::which("node").unwrap());
        command.args(["-e", "process.stderr.write('e'.repeat(400000));process.stdout.write(JSON.stringify({event:{delta:{type:'input_json_delta',partial_json:'{\"reply\":\"进度\"'}}})+'\\n');let s='';process.stdin.setEncoding('utf8');process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{if(s!==process.env.EXPECTED.repeat(100000))process.exit(2);console.log(JSON.stringify({type:'result',result:'',structured_output:{reply:'完成'}}));});"]);
        command.env("EXPECTED", "纯文本 & % ! () 中文\n");
        let mut updates = Vec::new();
        let result = super::run_coordinator_command(command, prompt, std::time::Duration::from_secs(10), &|| false, &mut |update| updates.push(update)).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["reply"], "完成");
        assert!(!updates.is_empty(), "partial progress must still stream during input");
    }

    #[test]
    fn coordinator_reports_output_overflow_and_bounded_diagnostics() {
        for overflow in [false, true] {
            let mut command = std::process::Command::new(which::which("node").unwrap());
            command.args(["-e", if overflow {
                "process.stdin.resume();process.stdout.write('x'.repeat(9*1024*1024));"
            } else {
                "process.stdin.resume();process.stderr.write('x'.repeat(400000)+'diagnostic-tail');process.exitCode=2;"
            }]);
            let error = super::run_coordinator_command(command, String::new(), std::time::Duration::from_secs(5), &|| false, &mut |_| {}).unwrap_err();
            let diagnostic = format!("{error:#}");
            if overflow { assert!(diagnostic.contains("8 MiB"), "{diagnostic}"); }
            else {
                assert!(diagnostic.contains("diagnostics truncated"));
                assert!(diagnostic.ends_with("diagnostic-tail"));
                assert!(diagnostic.len() < 270000);
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn coordinator_wrapper_preserves_literal_arguments_and_uses_local_model() {
        let root = std::env::temp_dir().join(format!("coordinator args 中文 & () {}", uuid::Uuid::new_v4()));
        let entry = root.join("node_modules/@anthropic-ai/claude-code/cli.js");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, r#"const assert=require('node:assert/strict');const args=process.argv.slice(2);assert.equal(args[args.indexOf('--system-prompt')+1],'literal & % ! () 中文');assert(!args.includes('--model'));assert.equal(args[args.indexOf('--json-schema')+1],'{"type":"object"}');let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{assert.equal(s,'plain & % ! () 中文');console.log(JSON.stringify({type:'result',result:'{"reply":"ok"}'}));});"#).unwrap();
        let wrapper = root.join("claude.cmd");
        std::fs::write(&wrapper, "@node @anthropic-ai wrapper fixture").unwrap();
        let result = super::run_coordinator_attempt(&wrapper, r#"{"type":"object"}"#, "literal & % ! () 中文", "plain & % ! () 中文".into(), Some("literal & % ! () 中文"), std::time::Duration::from_secs(5), &|| false, &mut |_| {});
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(result.unwrap(), r#"{"reply":"ok"}"#);
    }

    #[test]
    fn coordinator_deadline_and_cancel_cover_blocked_stdin() {
        for cancelled in [false, true] {
            let mut command = std::process::Command::new(which::which("node").unwrap());
            command.args(["-e", "process.stdin.pause();setInterval(()=>{},1000)"]);
            let started = std::time::Instant::now();
            let error = super::run_coordinator_command(command, "x".repeat(4*1024*1024), std::time::Duration::from_millis(120), &|| cancelled && started.elapsed() > std::time::Duration::from_millis(60), &mut |_| {}).unwrap_err();
            assert!(error.to_string().contains(if cancelled {"已取消"} else {"超时"}), "{error:#}");
            assert!(started.elapsed() < std::time::Duration::from_secs(8));
        }
    }

    use super::{
        add_project_communication_contract, cancel_coordinator_request,
        communication_only_requested, compact_history, coordinator_attempt_timed_out,
        coordinator_prompt_transport, extract_partial_json_string, is_api_retry_line,
        normalize_communication_workspaces, parse_plan, requested_agent_runtimes, stream_result,
        streamed_reply_from_line, validate_communication_plan, validate_explicit_runtime_roles,
        validate_requested_runtimes, validate_target_agents, CoordinatorCancellationToken,
    };
    use crate::multi_agent::{
        AgentRole, AgentRuntimeKind, CoordinatorHistoryMessage, CoordinatorIntent,
        CoordinatorMessageRole, WorkspaceMode,
    };

    #[test]
    fn api_retry_lines_are_not_treated_as_progress() {
        assert!(is_api_retry_line(
            r#"{"type":"system","subtype":"api_retry","attempt":3,"error_status":429,"error":"rate_limit"}"#
        ));
        assert!(!is_api_retry_line(
            r#"{"type":"system","subtype":"init","tools":[]}"#
        ));
        assert!(!is_api_retry_line(
            r#"{"type":"stream_event","event":{"delta":{"type":"input_json_delta","partial_json":"{}"}}}"#
        ));
    }

    #[test]
    fn compact_history_keeps_the_full_frontend_window() {
        let history: Vec<CoordinatorHistoryMessage> = (0..10)
            .map(|index| CoordinatorHistoryMessage {
                role: if index % 2 == 0 {
                    CoordinatorMessageRole::User
                } else {
                    CoordinatorMessageRole::Assistant
                },
                content: format!("消息 {index}"),
            })
            .collect();
        let compacted = compact_history(&history);
        assert_eq!(compacted.len(), 10, "前端发送最近 10 条，协调器应全部保留");
        assert_eq!(compacted[0].content, "消息 0");
        assert_eq!(compacted[9].content, "消息 9");
    }

    #[test]
    fn coordinator_progress_extends_the_idle_deadline_but_not_the_hard_limit() {
        let timeout = std::time::Duration::from_secs(60);

        assert!(!coordinator_attempt_timed_out(
            std::time::Duration::from_secs(61),
            std::time::Duration::from_secs(1),
            timeout,
        ));
        assert!(coordinator_attempt_timed_out(
            std::time::Duration::from_secs(61),
            std::time::Duration::from_secs(60),
            timeout,
        ));
        assert!(coordinator_attempt_timed_out(
            std::time::Duration::from_secs(180),
            std::time::Duration::from_secs(1),
            timeout,
        ));
    }

    #[test]
    fn structured_reply_is_extracted_while_json_is_still_streaming() {
        let mut partial_json = String::new();
        let first = serde_json::json!({
            "type": "stream_event",
            "event": {"delta": {"type": "input_json_delta", "partial_json": "{\"reply\":\"第一句。"}}
        })
        .to_string();
        let second = serde_json::json!({
            "type": "stream_event",
            "event": {"delta": {"type": "input_json_delta", "partial_json": "第二句。\",\"intent\":\"unknown\"}"}}
        })
        .to_string();

        assert_eq!(
            streamed_reply_from_line(&first, &mut partial_json).as_deref(),
            Some("第一句。")
        );
        assert_eq!(
            streamed_reply_from_line(&second, &mut partial_json).as_deref(),
            Some("第一句。第二句。")
        );
    }

    #[test]
    fn streamed_reply_decodes_json_escapes_before_emitting_text() {
        assert_eq!(
            extract_partial_json_string("{\"reply\":\"第一行\\n第二行\\\"引用\\\"\"}", "reply")
                .as_deref(),
            Some("第一行\n第二行\"引用\"")
        );
    }

    #[test]
    fn final_result_is_read_from_stream_json_footer() {
        let raw = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\"}\n",
            "{\"type\":\"result\",\"result\":\"{\\\"reply\\\":\\\"完成\\\"}\"}\n"
        );
        assert_eq!(stream_result(raw).unwrap(), "{\"reply\":\"完成\"}");
    }

    #[test]
    fn registered_coordinator_request_can_be_cancelled_and_is_removed_on_drop() {
        let request_id = "coordinator-cancellation-test";
        let token = CoordinatorCancellationToken::register(Some(request_id));

        assert!(!token.is_cancelled());
        assert!(cancel_coordinator_request(request_id));
        assert!(token.is_cancelled());
        drop(token);
        assert!(!cancel_coordinator_request(request_id));
    }

    fn structured_plan(runtime_kind: &str) -> String {
        serde_json::json!({
            "structured_output": {
                "intent": "plan_project",
                "reply": "已生成计划",
                "projectName": "贪吃蛇",
                "goal": "完成可运行的贪吃蛇游戏",
                "rootPath": "F:\\\\project\\\\snake-demo",
                "targetTaskId": null,
                "targetAgentId": null,
                "requiresConfirmation": true,
                "agents": [{
                    "key": "worker",
                    "name": "OpenCode 工程师",
                    "runtimeKind": runtime_kind,
                    "role": "worker",
                    "description": "实现游戏",
                    "model": null,
                    "permissionProfile": "auto"
                }],
                "tasks": [{
                    "key": "game",
                    "stage": 1,
                    "title": "实现游戏",
                    "description": "实现游戏逻辑",
                    "acceptanceCriteria": "浏览器可运行",
                    "verificationCommand": null,
                    "assigneeKey": "worker",
                    "reviewerKey": null,
                    "dependsOn": [],
                    "workspaceMode": "worktree",
                    "priority": 0
                }]
            }
        })
        .to_string()
    }

    #[test]
    fn parses_opencode_from_the_coordinator_protocol() {
        let plan = parse_plan(&structured_plan("opencode")).unwrap();

        assert_eq!(plan.agents[0].runtime_kind.as_str(), "opencode");
    }

    #[test]
    fn cli_plan_admission_keeps_legacy_desktop_only_as_an_existing_member() {
        for runtime in ["zcode","claude_chat","codex_chat","kimi_chat","pi_chat","opencode_chat"] {
            assert!(parse_plan(&structured_plan(runtime)).unwrap_err().to_string().contains("CLI"));
        }
        for runtime in super::super::COLLABORATION_CLI_RUNTIMES {
            assert_eq!(parse_plan(&structured_plan(runtime.as_str())).is_ok(),
                runtime != super::AgentRuntimeKind::Antigravity);
        }
        let mut old:serde_json::Value=serde_json::from_str(&structured_plan("zcode")).unwrap();
        old["structured_output"]["intent"]=serde_json::json!("add_tasks");
        old["structured_output"]["agents"][0]["existingAgentId"]=serde_json::json!("original-desktop-member");
        assert_eq!(parse_plan(&old.to_string()).unwrap().agents[0].runtime_kind,super::AgentRuntimeKind::ZCode);
    }

    #[test]
    fn parse_errors_include_the_invalid_value_and_output_excerpt() {
        let error = parse_plan(&structured_plan("open_code")).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("open_code"));
        assert!(message.contains("runtimeKind"));
    }

    #[test]
    fn unknown_runtime_is_rejected_in_every_new_plan_envelope() {
        let root: serde_json::Value = serde_json::from_str(&structured_plan("removed_runtime")).unwrap();
        let plan = root["structured_output"].clone();
        for raw in [root.to_string(), plan.to_string(), serde_json::json!({"result":plan.to_string()}).to_string()] {
            let error = parse_plan(&raw).unwrap_err().to_string();
            assert!(error.contains("removed_runtime") && error.contains("runtimeKind"), "{error}");
        }
    }

    #[test]
    fn coordinator_dynamic_prompt_uses_stdin_transport() {
        let prompt = "项目快照\n".repeat(20_000);
        let transport = coordinator_prompt_transport(prompt.clone());

        assert!(transport.arguments.is_empty());
        assert_eq!(transport.stdin_payload, prompt);
    }

    #[test]
    fn parses_claude_and_codex_mentions_as_an_ordered_unique_selection() {
        let runtimes = requested_agent_runtimes(
            "@ClaudeCode @Codex 请协作开发；@codex 负责复查，忽略 @CodexExtra。",
        );

        assert_eq!(
            runtimes,
            vec![AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex]
        );
    }

    #[test]
    fn parses_all_supported_runtime_group_mentions() {
        assert_eq!(
            requested_agent_runtimes(
                "@KimiCode @Pi @OpenCode 协作验证并集成交付"
            ),
            vec![
                AgentRuntimeKind::KimiCode,
                AgentRuntimeKind::Pi,
                AgentRuntimeKind::OpenCode,
            ]
        );
    }

    #[test]
    fn does_not_treat_email_addresses_as_agent_mentions() {
        assert!(requested_agent_runtimes("联系 user@codex.com 获取资料").is_empty());
    }

    #[test]
    fn does_not_treat_instance_handles_as_runtime_group_mentions() {
        assert!(
            requested_agent_runtimes("@Codex审查者 请检查接口，@Claude执行者-2 负责修改")
                .is_empty()
        );
    }

    #[test]
    fn mentioned_runtimes_are_a_hard_constraint_for_generated_plans() {
        let mut plan = parse_plan(&structured_plan("claude_code")).unwrap();
        let mut codex_plan = parse_plan(&structured_plan("codex")).unwrap();
        let requested = vec![AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex];
        codex_plan.agents[0].key = "codex_worker".to_string();
        codex_plan.tasks[0].assignee_key = "codex_worker".to_string();

        let error = validate_requested_runtimes(&plan, &requested).unwrap_err();
        assert!(error.to_string().contains("Codex"));

        plan.agents.push(codex_plan.agents.remove(0));
        let error = validate_requested_runtimes(&plan, &requested).unwrap_err();
        assert!(error.to_string().contains("未分配任务"));

        plan.tasks.push(codex_plan.tasks.remove(0));
        assert!(validate_requested_runtimes(&plan, &requested).is_ok());
    }

    #[test]
    fn excluded_bare_runtime_does_not_become_a_required_participant() {
        for exclusion in ["不需要Codex参与", "不要Codex", "不使用Codex"] {
            assert_eq!(
                requested_agent_runtimes(&format!("用ClaudeCode开发，{exclusion}")),
                vec![AgentRuntimeKind::ClaudeCode]
            );
        }
        assert_eq!(
            requested_agent_runtimes("@Codex 不需要开发"),
            vec![AgentRuntimeKind::Codex]
        );
    }

    #[test]
    fn natural_frontend_backend_assignment_preserves_writable_cli_tasks() {
        let message = "claude code和codex一起完成俄罗斯方块游戏的开发，codex服务后端，claude code负责前端，并通过CLI互相通信";
        assert_eq!(
            requested_agent_runtimes(message),
            vec![AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Codex]
        );
        assert!(!communication_only_requested(message));
        let mut plan = parse_plan(&structured_plan("claude_code")).unwrap();
        let mut backend = parse_plan(&structured_plan("codex")).unwrap();
        plan.tasks[0].title = "实现前端界面".into();
        backend.agents[0].key = "backend".into();
        backend.tasks[0].key = "backend_task".into();
        backend.tasks[0].assignee_key = "backend".into();
        backend.tasks[0].title = "实现后端服务".into();
        plan.agents.push(backend.agents.remove(0));
        plan.tasks.push(backend.tasks.remove(0));
        add_project_communication_contract(&mut plan, None);
        assert!(validate_explicit_runtime_roles(message, &plan).is_ok());
        assert!(validate_communication_plan(message, &plan).is_ok());
        for task in &plan.tasks {
            assert_eq!(task.workspace_mode, WorkspaceMode::Worktree);
            assert!(super::contract_requires_session_cli(
                &task.description,
                &task.acceptance_criteria
            ));
        }
        plan.tasks[0].title = "实现 React 界面".into();
        plan.tasks[0].description.push_str("\n负责前端交互与界面");
        assert!(validate_explicit_runtime_roles(message, &plan).is_ok());
        plan.tasks[1].title = "后端接口 Review".into();
        plan.tasks[1].workspace_mode = WorkspaceMode::ReadOnly;
        assert!(validate_explicit_runtime_roles(message, &plan).is_err());
        plan.tasks[1].workspace_mode = WorkspaceMode::Worktree;
        plan.tasks[1].title = "实现前端".into();
        plan.tasks[1]
            .description
            .push_str("后端同伴的接口由另一 Agent 提供");
        assert!(validate_explicit_runtime_roles(message, &plan).is_err());
    }

    #[test]
    fn communication_plan_requires_crewvela_cli() {
        let mut plan = parse_plan(&structured_plan("claude_code")).unwrap();
        assert!(validate_communication_plan("让两个 Agent 互相问候", &plan).is_err());
        plan.tasks[0].description =
            "读取 CREWVELA_CLI，运行 sessions，再执行 send --to target --body hello".into();
        assert!(validate_communication_plan("让两个 Agent 互相问候", &plan).is_err());
        plan.agents[0].role = AgentRole::Orchestrator;
        assert!(validate_communication_plan("让两个 Agent 互相问候", &plan).is_ok());
    }
    #[test]
    fn communication_tasks_use_a_cli_capable_workspace() {
        let mut plan = parse_plan(&structured_plan("claude_code")).unwrap();
        plan.agents[0].role = AgentRole::Orchestrator;
        plan.tasks[0].workspace_mode = WorkspaceMode::SharedWrite;
        plan.tasks[0].description =
            "读取 CREWVELA_CLI，运行 sessions，再执行 send --to target --body hello".into();

        assert!(communication_only_requested("@ClaudeCode @Codex 打个招呼"));
        normalize_communication_workspaces(&mut plan);

        assert_eq!(plan.tasks[0].workspace_mode, WorkspaceMode::ReadOnly);
    }
    #[test]
    fn exact_agent_mentions_require_every_selected_instance_and_no_substitutes() {
        let mut plan = parse_plan(&structured_plan("claude_code")).unwrap();
        let mut second = parse_plan(&structured_plan("codex")).unwrap();
        plan.intent = CoordinatorIntent::AddTasks;
        plan.agents[0].existing_agent_id = Some("agent-a".to_string());
        second.agents[0].key = "codex_worker".to_string();
        second.agents[0].existing_agent_id = Some("agent-b".to_string());
        second.tasks[0].assignee_key = "codex_worker".to_string();
        let selected = vec!["agent-a".to_string(), "agent-b".to_string()];

        let error = validate_target_agents(&plan, &selected, &[]).unwrap_err();
        assert!(error.to_string().contains("agent-b"));

        plan.agents.push(second.agents.remove(0));
        plan.tasks.push(second.tasks.remove(0));
        assert!(validate_target_agents(&plan, &selected, &[]).is_ok());

        plan.agents[1].existing_agent_id = Some("agent-c".to_string());
        let error = validate_target_agents(&plan, &selected, &[]).unwrap_err();
        assert!(error.to_string().contains("未选择实例"));
    }

    #[test]
    fn runtime_group_without_existing_instance_may_create_that_runtime() {
        let mut plan = parse_plan(&structured_plan("claude_code")).unwrap();
        let mut codex = parse_plan(&structured_plan("codex")).unwrap();
        plan.intent = CoordinatorIntent::AddTasks;
        plan.agents[0].existing_agent_id = Some("claude-a".to_string());
        codex.agents[0].key = "codex_worker".to_string();
        codex.tasks[0].assignee_key = "codex_worker".to_string();
        plan.agents.push(codex.agents.remove(0));
        plan.tasks.push(codex.tasks.remove(0));

        assert!(validate_target_agents(
            &plan,
            &["claude-a".to_string()],
            &[AgentRuntimeKind::Codex],
        )
        .is_ok());
        assert!(validate_target_agents(&plan, &["claude-a".to_string()], &[]).is_err());
    }
}

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::launcher::{build_env, resolve_claude};
use crate::providers::ProviderConfig;

const SUMMARY_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_SUMMARY_TITLE_CHARS: usize = 15;

const SYSTEM_PROMPT: &str = "你是会话标题生成器。用户消息是用户的首条对话请求。\n\n输出要求：\n- 严格 ≤ 10 个字（汉字、字母、数字都算一个字）；\n- 使用与请求相同的语言；\n- 只输出标题文本本身，不要任何前缀、解释、引号、书名号、句号、感叹号、问号等任何符号；\n- 不要写“标题：xxx”这样的格式，直接写标题；\n- 一行结束，不要换行；\n- 若无法概括，输出\"新会话\"。";

/// 用会话自己的 provider 环境调 `claude -p`，把首条消息总结成标题。
/// 失败时由调用方保留截断标题。
pub fn summarize_title(provider: &ProviderConfig, key: &str, prompt: &str) -> Result<String> {
    let _runtime_lease =
        crate::runtime_preferences::acquire(crate::multi_agent::AgentRuntimeKind::ClaudeCode)?;
    let env = build_env(provider, key);
    // user settings.json 的 env 优先级高于进程 env，必须走 --settings（同 PTY 启动逻辑）。
    let settings_json = serde_json::json!({ "env": env }).to_string();

    let claude = resolve_claude()?;
    let mut command = claude_command(&claude)?;
    command
        .arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--no-session-persistence")
        .arg("--safe-mode")
        .arg("--tools")
        .arg("")
        .arg("--permission-mode")
        .arg("dontAsk")
        .arg("--max-turns")
        .arg("1")
        .arg("--settings")
        .arg(settings_json)
        .arg("--system-prompt")
        .arg(SYSTEM_PROMPT)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // 在 home 目录运行，避免加载项目 CLAUDE.md 影响标题生成。
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let output = run_cli_with_timeout(command, Some(prompt.as_bytes().to_vec()), SUMMARY_TIMEOUT, 64 * 1024, "标题摘要")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let snippet: String = stderr.chars().take(400).collect();
        return Err(anyhow!(
            "标题摘要进程退出码 {}（stderr: {}）",
            output.status,
            snippet
        ));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let snippet: String = raw.chars().take(200).collect();
    parse_claude_title_response(&raw)
        .ok_or_else(|| anyhow!("标题摘要无效（非 JSON 或 num_turns=0），stdout 起始: {snippet}"))
}

/// 本地运行时使用自己的登录和配置，绝不借用 Claude provider 凭据。
pub fn summarize_runtime(kind: &str, prompt: &str, model: Option<&str>) -> Result<String> {
    if matches!(kind, "kimi_code" | "kimi_chat") { return summarize_kimi(prompt, model); }
    if matches!(kind, "codex" | "codex_chat") { return crate::codex_title_summary::summarize(prompt, model); }
    use crate::multi_agent::AgentRuntimeKind;
    let runtime = match kind {
        "pi" | "pi_chat" => AgentRuntimeKind::Pi,
        "claude_code" | "claude_chat" => AgentRuntimeKind::ClaudeCode,
        _ => return Err(anyhow!("{kind} 尚无安全临时摘要接口")),
    };
    let _lease = crate::runtime_preferences::acquire(runtime)?;
    let path = if runtime == AgentRuntimeKind::ClaudeCode { resolve_claude()? }
        else { crate::multi_agent::resolve_runtime_executable(runtime).ok_or_else(|| anyhow!("摘要运行时未安装"))? };
    let mut command = summary_command(&path, runtime)?;
    if runtime == AgentRuntimeKind::Pi {
        command.args(["--print", "--no-session", "--no-tools", "--no-extensions", "--no-mcp", "--no-skills", "--no-prompt-templates", "--no-context-files", "--no-approve", "--system-prompt", SYSTEM_PROMPT, "--", prompt]);
    } else {
        command.args(["-p", "--output-format", "json", "--no-session-persistence", "--safe-mode", "--tools", "", "--permission-mode", "dontAsk", "--max-turns", "1", "--system-prompt", SYSTEM_PROMPT, "--", prompt]);
    }
    // 独立临时目录隔离项目上下文；运行时仍读取自己的账户配置。
    let directory = std::env::temp_dir().join(format!("cc-title-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory)?;
    command.current_dir(&directory).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let result = run_runtime_summary(command, runtime == AgentRuntimeKind::Pi);
    let _ = std::fs::remove_dir(&directory);
    result
}

pub(crate) fn summary_command(path: &std::path::Path, runtime: crate::multi_agent::AgentRuntimeKind) -> Result<Command> {
    if !cfg!(windows) || !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")) {
        return Ok(Command::new(path));
    }
    // 首条用户请求和多行 system prompt 不能经 cmd /c：npm 包装器既会破坏
    // 参数，也会把用户文本中的 & 等当成 shell 操作。只认对应安装的 Node 入口。
    let source = std::fs::read_to_string(path)?;
    let prefix = path.parent().context("运行时包装器缺少目录")?;
    let candidates: &[(&str, &str)] = match runtime {
        crate::multi_agent::AgentRuntimeKind::Pi => &[
            ("@earendil-works", "node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"),
            ("@mariozechner", "node_modules/@mariozechner/pi-coding-agent/dist/cli.js"),
        ],
        crate::multi_agent::AgentRuntimeKind::ClaudeCode => &[("@anthropic-ai", "node_modules/@anthropic-ai/claude-code/cli.js")],
        _ => return Err(anyhow!("此运行时不支持安全的 Windows 包装器")),
    };
    let entry = candidates.iter().filter(|(package, _)| source.contains(package)).map(|(_, entry)| prefix.join(entry)).find(|entry| entry.is_file()).context("未找到运行时的 Node 入口，未通过 shell 传递用户内容")?;
    let local_node = prefix.join("node.exe");
    let node = if local_node.is_file() { local_node } else { which::which("node").context("运行时需要 Node.js")? };
    let mut command = Command::new(node);
    command.arg(entry);
    Ok(command)
}

fn summarize_kimi(prompt: &str, model: Option<&str>) -> Result<String> {
    crate::kimi_title_summary::summarize(prompt, model)
}

/// Resolve npm Windows wrappers without handing user content to cmd.exe.
pub(crate) fn claude_command(path: &std::path::Path) -> Result<Command> {
    summary_command(path, crate::multi_agent::AgentRuntimeKind::ClaudeCode)
}

struct CapturedPipe {
    bytes: Vec<u8>,
    overflow: bool,
}

fn capture_pipe(mut pipe: impl std::io::Read, limit: usize, keep_tail: bool) -> std::io::Result<CapturedPipe> {
    let mut bytes = Vec::new();
    let mut overflow = false;
    let mut buffer = [0u8; 8192];
    loop {
        let count = match pipe.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 { break; }
        if keep_tail {
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() > limit {
                bytes.drain(..bytes.len() - limit);
                overflow = true;
            }
        } else {
            let kept = count.min(limit.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&buffer[..kept]);
            overflow |= kept < count;
        }
        // Keep draining after the retention limit: closing a pipe early can block
        // or break an otherwise successful CLI which is still writing diagnostics.
    }
    Ok(CapturedPipe { bytes, overflow })
}

/// Collect both output pipes while writing stdin, with one deadline for all I/O.
/// The caller receives nonzero exits unchanged and decides how to explain them.
pub(crate) fn run_cli_with_timeout(
    mut command: Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
    stdout_limit: usize,
    operation: &str,
) -> Result<std::process::Output> {
    use std::io::Write;
    use std::sync::mpsc;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    let (mut child, identity) = crate::acp::spawn_owned_command(&mut command).with_context(|| format!("无法启动{operation}"))?;
    let deadline = Instant::now() + timeout;
    let result = (|| -> Result<std::process::Output> {
        let stdout = child.stdout.take().context("子进程 stdout 不可用")?;
        let stderr = child.stderr.take().context("子进程 stderr 不可用")?;
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();
        std::thread::spawn(move || { let _ = stdout_tx.send(capture_pipe(stdout, stdout_limit, false)); });
        std::thread::spawn(move || { let _ = stderr_tx.send(capture_pipe(stderr, 256 * 1024, true)); });
        let input_rx = if let Some(input) = input {
            let mut stdin = child.stdin.take().context("子进程 stdin 不可用")?;
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = stdin.write_all(&input).and_then(|_| stdin.flush());
                drop(stdin); // EOF is part of the prompt transport contract.
                let _ = tx.send(result);
            });
            Some(rx)
        } else { None };
        let status = loop {
            if let Some(status) = crate::acp::try_wait_child_preserving_identity(&mut child).context("读取子进程状态失败")? { break status; }
            if Instant::now() >= deadline { return Err(anyhow!("{operation}超时")); }
            std::thread::sleep(Duration::from_millis(20));
        };
        // A descendant may have inherited a pipe. Never let joining reader threads
        // turn a bounded command into an unbounded wait after its parent exits.
        let stdout = stdout_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .with_context(|| format!("{operation} stdout 读取超时或中断"))??;
        let stderr = stderr_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .with_context(|| format!("{operation} stderr 读取超时或中断"))??;
        if let Some(rx) = input_rx {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .with_context(|| format!("{operation} stdin 写入超时或中断"))?
                .with_context(|| format!("写入{operation} Prompt 失败"))?;
        }
        if stdout.overflow { return Err(anyhow!("{operation}响应超出上限（{stdout_limit} 字节）")); }
        let mut diagnostic = stderr.bytes;
        if stderr.overflow {
            let mut marked = b"[diagnostics truncated; showing last 256 KiB]\n".to_vec();
            marked.append(&mut diagnostic);
            diagnostic = marked;
        }
        Ok(std::process::Output { status, stdout: stdout.bytes, stderr: diagnostic })
    })();
    // Every outcome, including an exited root with detached descendants, must
    // finish the spawn-owned Job/group before the caller can retry or succeed.
    if !identity.is_verified() { crate::acp::stop_child_checked(&mut child)?; }
    child.wait().map_err(|error| anyhow!(crate::acp::UnverifiedStop(format!("等待{operation}退出失败：{error}"))))?;
    result
}

fn run_runtime_summary(command: Command, pi: bool) -> Result<String> {
    let output = run_cli_with_timeout(command, None, SUMMARY_TIMEOUT, 64 * 1024, "独立标题摘要")?;
    if !output.status.success() {
        let detail: String = String::from_utf8_lossy(&output.stderr).chars().take(400).collect();
        return Err(anyhow!("独立标题摘要失败：{}（{detail}）", output.status));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    if pi { sanitize_title(&raw) } else { parse_claude_title_response(&raw) }
        .ok_or_else(|| anyhow!("独立标题摘要未返回有效标题"))
}

/// 从模型输出提取单行标题：取首个非空行，去引号/书名号/结尾标点，限长。
pub fn sanitize_title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    let line = ["会话标题：", "会话标题:", "标题：", "标题:", "Title:"].iter().find_map(|prefix| line.strip_prefix(prefix)).unwrap_or(line).trim();
    let stripped = line.trim_matches(|c| {
        matches!(
            c,
            '"' | '\'' | '“' | '”' | '‘' | '’' | '「' | '」' | '『' | '』' | '《' | '》' | '`' | '*' | '_'
        )
    });
    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title = collapsed.trim().to_string();
    while title.ends_with(|c| {
        matches!(
            c,
            '。' | '，' | '、' | '！' | '？' | '：' | '；' | '.' | ',' | '!' | '?' | ':' | ';'
        )
    }) {
        title.pop();
    }
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let capped: String = title.chars().take(MAX_SUMMARY_TITLE_CHARS).collect();
    if capped.is_empty() {
        None
    } else {
        Some(capped)
    }
}

/// 解析 `claude -p --output-format json` 的输出，提取标题。
///
/// 仅当 `num_turns >= 1`（AI 真的被调用）才返回 `Some`；slash-command 解析失败、
/// 命令行错误、空响应等场景 claude 可能仍输出合法 JSON 但 `num_turns=0`，
/// 此时 `result` 字段塞的是 "Unknown command: /xxx" 之类的错误文本，必须丢弃。
fn parse_claude_title_response(raw: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    let num_turns = parsed
        .get("num_turns")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    if num_turns == 0 || parsed.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        return None;
    }
    let result = parsed.get("result").and_then(|value| value.as_str())?;
    sanitize_title(result)
}

#[cfg(all(test, windows))]
pub(crate) fn assert_runner_stops_inherited_child(
    hold_pipe: bool,
    run: impl FnOnce(Command) -> Result<()> + Send,
) {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    let root = std::env::temp_dir().join(format!("cli-owned-descendant-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let script = root.join("fixture.cjs");
    let pid_file = root.join("grandchild.pid");
    let release = root.join("release");
    std::fs::write(&script, r#"
      const fs=require('node:fs'),cp=require('node:child_process');
      const [mode,pidFile,release,hold]=process.argv.slice(2);
      if(mode==='grand'){fs.writeFileSync(pidFile+'.ready',String(process.pid));fs.renameSync(pidFile+'.ready',pidFile);setInterval(()=>{},1000);}
      else if(mode==='tool'){
        const child=cp.spawn(process.execPath,[__filename,'grand',pidFile,release,hold],{stdio:hold==='yes'?'inherit':'ignore',detached:true,windowsHide:true});
        const wait=setInterval(()=>{if(fs.existsSync(pidFile)){clearInterval(wait);child.unref();process.exit(0);}},10);
      }else{
        const tool=cp.spawn(process.execPath,[__filename,'tool',pidFile,release,hold],{stdio:'inherit',windowsHide:true});
        tool.on('exit',()=>{setInterval(()=>{if(fs.existsSync(release)){console.log(JSON.stringify({type:'result',result:'',structured_output:{reply:'done'}}));process.exit(0);}},10);});
      }
    "#).unwrap();
    let mut command = Command::new(which::which("node").unwrap());
    command.arg(&script).arg("root").arg(&pid_file).arg(&release).arg(if hold_pipe { "yes" } else { "no" });
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || run(command));
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Ok(raw) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = raw.parse::<u32>() { break pid; }
            }
            assert!(Instant::now() < deadline, "fixture grandchild did not start");
            std::thread::sleep(Duration::from_millis(10));
        };
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        assert!(!raw.is_null(), "capture the actual live descendant handle before root exits: {} (pid={pid})", std::io::Error::last_os_error());
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        assert_eq!(unsafe { WaitForSingleObject(process.as_raw_handle(), 0) }, windows_sys::Win32::Foundation::WAIT_TIMEOUT);
        std::fs::write(&release, b"release owned root").unwrap();
        worker.join().unwrap().unwrap();
        assert_eq!(unsafe { WaitForSingleObject(process.as_raw_handle(), 3000) }, windows_sys::Win32::Foundation::WAIT_OBJECT_0, "runner returned while its inherited descendant still executes");
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[cfg(windows)]
    #[test]
    fn cli_timeout_stops_grandchild_after_root_and_tool_exit_with_inherited_pipe() {
        super::assert_runner_stops_inherited_child(true, |command| {
            let result = super::run_cli_with_timeout(command, None, Duration::from_secs(5), 1024 * 1024, "owned fixture");
            assert!(result.is_err(), "an inherited open stdout must reach the effective deadline");
            Ok(())
        });
    }

    #[test]
    fn title_platform_npm_wrapper_preserves_literal_user_arguments_without_shell() {
        let root = std::env::temp_dir().join(format!("cc title argv {}", uuid::Uuid::new_v4()));
        let entry = root.join("node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "console.log(JSON.stringify(process.argv.slice(2)));").unwrap();
        #[cfg(windows)]
        let wrapper = root.join("pi.cmd");
        #[cfg(windows)]
        std::fs::write(&wrapper, "@echo off\nnode \"%~dp0%node_modules\\@earendil-works\\pi-coding-agent\\dist\\bundle\\cli.js\" %*\n").unwrap();
        #[cfg(unix)]
        let wrapper = {
            use std::os::unix::fs::PermissionsExt;
            let wrapper = root.join("pi");
            std::fs::write(&wrapper, "#!/bin/sh\nexec node \"$(dirname \"$0\")/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js\" \"$@\"\n").unwrap();
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
            wrapper
        };
        let literal = "中文请求 & echo forbidden | more\n包含 \"引号\" 和 %变量%";
        let output = summary_command(&wrapper, crate::multi_agent::AgentRuntimeKind::Pi).unwrap().args(["--system-prompt", SYSTEM_PROMPT, "--", literal]).output().unwrap();
        assert!(output.status.success());
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(), serde_json::json!(["--system-prompt", SYSTEM_PROMPT, "--", literal]));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn node_command(script: &str) -> Command {
        let mut command = Command::new(which::which("node").expect("Node is required by the CLI bridge tests"));
        command.args(["-e", script]);
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }

    #[test]
    fn cli_collects_large_stdout_stderr_and_stdin_concurrently() {
        let input = "中文 & | <> ^ % \"literal\"\n".repeat(32_768);
        let command = node_command(r#"
            const fs = require('node:fs');
            fs.writeSync(1, Buffer.alloc(512 * 1024, 65));
            fs.writeSync(2, Buffer.alloc(512 * 1024, 66));
            let input = '';
            process.stdin.setEncoding('utf8');
            process.stdin.on('data', part => { input += part; });
            process.stdin.on('end', () => console.log(JSON.stringify({ input })));
        "#);
        let output = run_cli_with_timeout(command, Some(input.clone().into_bytes()), Duration::from_secs(10), 4 * 1024 * 1024, "并发管道测试").unwrap();
        assert!(output.status.success());
        assert!(output.stderr.starts_with(b"[diagnostics truncated; showing last 256 KiB]\n"));
        assert!(output.stderr.ends_with(&vec![b'B'; 256 * 1024]));
        assert!(output.stdout[..512 * 1024].iter().all(|byte| *byte == b'A'));
        let echoed: serde_json::Value = serde_json::from_slice(&output.stdout[512 * 1024..]).unwrap();
        assert_eq!(echoed["input"].as_str(), Some(input.as_str()));
    }

    #[test]
    fn cli_output_limit_drains_both_pipes_and_reports_overflow() {
        let command = node_command("const fs=require('node:fs'); fs.writeSync(1,Buffer.alloc(1024*1024)); fs.writeSync(2,Buffer.alloc(1024*1024));");
        let started = Instant::now();
        let error = run_cli_with_timeout(command, None, Duration::from_secs(5), 1024, "超限测试").unwrap_err();
        assert!(error.to_string().contains("响应超出上限"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn cli_timeout_terminates_its_owned_process() {
        let marker = std::env::temp_dir().join(format!("cc-cli-timeout-{}.pid", uuid::Uuid::new_v4()));
        let mut command = node_command("require('node:fs').writeFileSync(process.argv[1],String(process.pid)); setInterval(()=>{},1000);");
        command.arg(&marker);
        let started = Instant::now();
        let error = run_cli_with_timeout(command, None, Duration::from_millis(700), 1024, "超时测试").unwrap_err();
        assert!(error.to_string().contains("超时"));
        assert!(started.elapsed() < Duration::from_secs(8));
        let pid = std::fs::read_to_string(&marker).unwrap();
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt;
            let output = Command::new("tasklist.exe").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .creation_flags(0x08000000).output().unwrap();
            assert!(output.status.success());
            assert!(!String::from_utf8_lossy(&output.stdout).lines().any(|line| line.split("\",\"").nth(1).is_some_and(|field| field.trim_matches('"') == pid)), "timed-out process is still running");
        }
        #[cfg(not(windows))] {
            assert!(!Command::new("kill").args(["-0", &pid]).status().unwrap().success());
        }
        std::fs::remove_file(marker).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn claude_npm_wrapper_preserves_literal_arguments_and_large_stdin_without_shell() {
        let root = std::env::temp_dir().join(format!("cc claude argv {}", uuid::Uuid::new_v4()));
        let entry = root.join("node_modules/@anthropic-ai/claude-code/cli.js");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "let input='';process.stdin.setEncoding('utf8');process.stdin.on('data',s=>input+=s);process.stdin.on('end',()=>console.log(JSON.stringify({args:process.argv.slice(2),input}))); ").unwrap();
        let wrapper = root.join("claude.cmd");
        std::fs::write(&wrapper, "@echo off\nnode \"%~dp0%node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\n").unwrap();
        let literal = "中文 & | <> ^ %变量% !变量! (括号) \"引号\"\n第二行";
        let input = literal.repeat(4096);
        let mut command = claude_command(&wrapper).unwrap();
        command.args(["--system-prompt", literal, "--settings", "{\"env\":{\"key\":\"literal&value\"}}"]);
        let output = run_cli_with_timeout(command, Some(input.clone().into_bytes()), Duration::from_secs(5), 4 * 1024 * 1024, "Claude包装器测试").unwrap();
        assert!(output.status.success());
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["args"], serde_json::json!(["--system-prompt", literal, "--settings", "{\"env\":{\"key\":\"literal&value\"}}"]));
        assert_eq!(response["input"].as_str(), Some(input.as_str()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn title_rejects_error_envelope_even_with_turns_and_result() {
        assert_eq!(parse_claude_title_response(r#"{"num_turns":1,"is_error":true,"result":"API Error"}"#), None);
    }

    #[test]
    fn title_output_removes_model_prefix_and_markdown() {
        assert_eq!(sanitize_title("会话标题：**登录超时修复**"), Some("登录超时修复".into()));
    }

    #[test]
    #[ignore = "requires a logged-in Kimi managed account; one title-only model request without session history"]
    fn kimi_title_installed_managed_account_generates_without_history() {
        let title = summarize_kimi("修复登录接口超时并添加重试回归测试", None).unwrap();
        assert!(!title.is_empty());
        assert!(title.chars().count() <= MAX_SUMMARY_TITLE_CHARS);
    }

    #[test]
    #[ignore = "requires configured Pi; one no-tools, no-session title-only model request"]
    fn pi_title_installed_account_generates_without_history() {
        let title = summarize_runtime("pi_chat", "修复登录接口超时并补充重试回归测试", None).unwrap();
        assert!(!title.is_empty());
        assert!(title.chars().count() <= MAX_SUMMARY_TITLE_CHARS);
    }

    #[test]
    fn parse_title_returns_sanitized_result_when_ai_was_invoked() {
        let raw = serde_json::json!({
            "num_turns": 1,
            "result": "修复登录接口\n\n第二行忽略"
        })
        .to_string();
        assert_eq!(
            parse_claude_title_response(&raw),
            Some("修复登录接口".to_string())
        );
    }

    #[test]
    fn parse_title_rejects_slash_command_failure_with_zero_turns() {
        let raw = serde_json::json!({
            "num_turns": 0,
            "result": "Unknown command: /unknown_xyz"
        })
        .to_string();
        assert_eq!(parse_claude_title_response(&raw), None);
    }

    #[test]
    fn parse_title_rejects_non_json_stdout() {
        assert_eq!(parse_claude_title_response("Unknown command: /xyz"), None);
        assert_eq!(parse_claude_title_response(""), None);
        assert_eq!(parse_claude_title_response("一些自由文本输出"), None);
    }

    #[test]
    fn parse_title_rejects_missing_or_empty_result() {
        let no_result = serde_json::json!({"num_turns": 1}).to_string();
        assert_eq!(parse_claude_title_response(&no_result), None);

        let empty_result = serde_json::json!({"num_turns": 1, "result": ""}).to_string();
        assert_eq!(parse_claude_title_response(&empty_result), None);
    }

    #[test]
    fn sanitize_takes_first_non_empty_line() {
        assert_eq!(
            sanitize_title("\n\n  修复登录接口超时  \n第二行忽略"),
            Some("修复登录接口超时".to_string())
        );
    }

    #[test]
    fn sanitize_strips_quotes_and_trailing_punctuation() {
        assert_eq!(
            sanitize_title("「重构会话列表。」"),
            Some("重构会话列表".to_string())
        );
        assert_eq!(
            sanitize_title("\"Add login page.\""),
            Some("Add login page".to_string())
        );
    }

    #[test]
    fn sanitize_caps_length() {
        let long = "这是一个非常非常长的标题它一定会超过二十五个字符所以会被截断处理";
        let result = sanitize_title(long).unwrap();
        assert_eq!(result.chars().count(), MAX_SUMMARY_TITLE_CHARS);
    }

    #[test]
    fn sanitize_rejects_empty() {
        assert_eq!(sanitize_title("\n \n"), None);
        assert_eq!(sanitize_title("「」。"), None);
    }
}

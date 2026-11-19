use super::*;
use std::path::Path;

pub(in crate::multi_agent) fn file_operation(
    store: &MultiAgentStore,
    run_id: &str,
    agent: &str,
    cwd: &Path,
    name: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    use sha2::{Digest, Sha256};
    managed_edits::with_write_scope(store, run_id, agent, AgentRuntimeKind::Pi, cwd, |scopes| {
        let relative = args["path"].as_str().context("缺少文件相对路径")?;
        MultiAgentStore::verify_write_paths(scopes, &[relative.to_owned()])?;
        let read = || -> Result<String> {
            let boundary = super::super::conversation::research::workspace_policy().boundary;
            let path =
                super::super::conversation::research::readable_path(cwd, &boundary, relative)?;
            let file = std::fs::File::open(path)?;
            anyhow::ensure!(file.metadata()?.is_file(), "仅支持普通文本文件");
            let mut bytes = Vec::new();
            file.take(1_048_577).read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() <= 1_048_576 && !bytes.contains(&0),
                "Pi 精确编辑只支持 1 MiB 内的 UTF-8 文本"
            );
            Ok(String::from_utf8(bytes)?)
        };
        match name {
            "cc_pi_edit_read" => {
                let text = read()?;
                Ok(
                    serde_json::json!({"revision":format!("{:x}",Sha256::digest(text.as_bytes())),"text":text}),
                )
            }
            "cc_pi_write_file" => {
                if let Some(expected) = args.get("revision") {
                    anyhow::ensure!(
                        expected.as_str()
                            == Some(format!("{:x}", Sha256::digest(read()?.as_bytes())).as_str()),
                        "文件在 Pi 读取后发生变化，未覆盖原文件"
                    );
                }
                managed_edits::write_text(
                    cwd,
                    scopes,
                    relative,
                    args["text"].as_str().context("缺少文件内容")?,
                )?;
                Ok(serde_json::json!({"path":relative}))
            }
            _ => bail!("该托管操作未授权"),
        }
    })
}

fn sessions_directory() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("multi-agent-sessions"))
}

/// The readiness hook reports Pi's actual session file. Its own header carries
/// the native ID and working directory, independent of the application tab ID.
fn session_identity(path: &Path, cwd: &Path) -> Result<String> {
    let path = path.canonicalize().context("Pi 原生会话文件已不可用")?;
    anyhow::ensure!(
        path.parent() == Some(sessions_directory()?.canonicalize()?.as_path()),
        "Pi 原生会话文件不属于本次托管存储"
    );
    let mut header = String::new();
    BufReader::new(std::fs::File::open(&path)?)
        .take(16 * 1024)
        .read_line(&mut header)?;
    let header: serde_json::Value =
        serde_json::from_str(&header).context("Pi 原生会话文件头无效")?;
    let id = header["id"].as_str().context("Pi 原生会话缺少 ID")?;
    anyhow::ensure!(
        header["type"] == "session"
            && uuid::Uuid::parse_str(id).is_ok()
            && Path::new(header["cwd"].as_str().context("Pi 原生会话缺少工作目录")?)
                .canonicalize()?
                == cwd.canonicalize()?,
        "Pi 原生会话与任务目录或身份不符"
    );
    Ok(id.to_string())
}

pub(super) fn confirmed(
    request: &RuntimeRequest,
    sessions: &SessionManager,
    record: &SessionRecord,
    command: &Command,
    expected: Option<&str>,
) -> Result<serde_json::Value> {
    let reported = PathBuf::from(
        sessions
            .runtime_history_locator(record)?
            .context("Pi 尚未回报原生会话文件")?,
    );
    let configured = command
        .get_args()
        .skip_while(|arg| *arg != "--session")
        .nth(1)
        .context("Pi 托管命令缺少原生会话文件")?;
    anyhow::ensure!(
        reported.canonicalize()? == Path::new(configured).canonicalize()?,
        "Pi 接受任务的会话文件与授权不符"
    );
    let id = session_identity(&reported, &request.cwd)?;
    anyhow::ensure!(
        expected.is_none_or(|value| value == id),
        "Pi 返回了不同的原生会话 ID"
    );
    Ok(serde_json::json!({"engine":"pi","sessionId":id,"sessionPath":reported}))
}

pub(super) fn execute(request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
    let mut version = runtime_command(AgentRuntimeKind::Pi)?;
    version.arg("--version");
    let output = crate::title_summary::run_cli_with_timeout(
        version,
        None,
        Duration::from_secs(10),
        16 * 1024,
        "Pi 托管工具检查",
    )?;
    anyhow::ensure!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "1.5.0",
        "Pi 托管需要已验证扩展隔离与原生回执的 1.5.0，未启动任务"
    );
    let directory = sessions_directory()?;
    std::fs::create_dir_all(&directory)?;
    let old = request.resume_locator.as_ref();
    let expected = locator_string(old, &["sessionId"]);
    let session_path = if let Some(id) = expected {
        let path = PathBuf::from(
            locator_string(old, &["sessionPath"]).context("Pi 原托管会话缺少文件位置")?,
        );
        anyhow::ensure!(
            session_identity(&path, &request.cwd)? == id
                && old.is_some_and(|locator| locator["engine"] == "pi"),
            "Pi 原会话 ID 或引擎与任务记录不符"
        );
        path
    } else {
        let path = directory.join(format!("{}.jsonl", request.run_id));
        // Pi writes its authoritative header on open when the explicitly named file exists.
        std::fs::File::create_new(&path)?;
        path
    };
    let research = RuntimePromptFile::create_private(
        &runtime_prompt_directory()?,
        &format!("{}.pi-research.mjs", request.run_id),
        include_bytes!("../../pi-conversation-research.mjs"),
    )?;
    let writes = request.task.workspace_mode == WorkspaceMode::Worktree;
    let editing = writes
        .then(|| {
            RuntimePromptFile::create_private(
                &runtime_prompt_directory()?,
                &format!("{}.pi-files.mjs", request.run_id),
                include_bytes!("managed-pi.mjs"),
            )
        })
        .transpose()?;
    let tools = if writes {
        "cc_research_list,cc_research_read,cc_research_search,crewvela_write,crewvela_edit"
    } else {
        "cc_research_list,cc_research_read,cc_research_search"
    };
    let mut command = runtime_command(AgentRuntimeKind::Pi)?;
    for key in ["PI_CODING_AGENT_DIR", "PI_OFFLINE"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .args([
            "--no-approve",
            "--no-tools",
            "--no-extensions",
            "--no-mcp",
            "--no-skills",
            "--no-prompt-templates",
            "--no-context-files",
            "--no-themes",
            "-e",
        ])
        .arg(&editing.as_ref().unwrap_or(&research).path)
        .args(["--tools", tools, "--session"])
        .arg(&session_path)
        .env("CREWVELA_PI_RESEARCH_EXTENSION", &research.path)
        .env_remove("CREWVELA_PI_MCP_URL")
        .env_remove("CREWVELA_AGENT_TOKEN")
        .env("CREWVELA_PI_MANAGED", "1")
        .env(
            "CREWVELA_RESEARCH_URL",
            format!("{}/mcp", request.collaboration.endpoint),
        )
        .env("CREWVELA_RESEARCH_TOKEN", &request.collaboration.token)
        .current_dir(external_command_path(&request.cwd));
    let run = |prompt: &str| {
        request.terminal_host.execute_native(
            AgentRuntimeKind::Pi,
            &command,
            prompt,
            request,
            cancelled,
            false,
            expected,
        )
    };
    if request.task.workspace_mode == WorkspaceMode::ReadOnly {
        request.log_launch("工作树只读工具", "Pi 原生终端")?;
        return managed_static_review(request, cancelled, run);
    }
    request.log_launch("按计划范围执行 Pi 原生 Write/Edit", "Pi 原生终端")?;
    run(&format!("{}\n请用 crewvela_write 或 crewvela_edit 实际修改获批文件，cc_research_* 读取工作树。CrewVela 在工作树运行计划中的测试命令。",
        render_task_prompt(request)))
}

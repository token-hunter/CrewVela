use crate::{
    multi_agent::AgentRuntimeKind,
    sessions::{SessionKind, SessionManager, SessionRecord},
    transcript::TranscriptMessage,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::{BufRead, Read},
    path::{Path, PathBuf},
};

pub(crate) fn read(
    record: &SessionRecord,
    sessions: &SessionManager,
) -> Result<Vec<TranscriptMessage>> {
    let kind = crate::session_switch::source_runtime(record);
    if kind == "claude_code" {
        let mut source = record.clone();
        source.kind = SessionKind::Claude;
        return crate::transcript::read_transcript(&source, 200);
    }
    let locator = sessions.runtime_history_locator(record)?;
    if kind == "kimi_code" {
        return kimi(record, locator.as_deref()).map(|(_, messages)| messages);
    }
    if kind == "opencode" {
        return sqlite_agent(record, locator.as_deref()).map(|(_, messages)| messages);
    }
    let locator = locator.context("此 CLI 未记录精确原生历史定位；不会按目录猜测其它会话")?;
    if kind == "pi" {
        return jsonl(Path::new(&locator));
    }
    if kind == "codex" {
        let home = std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"));
        return codex(&record.cwd, &locator, &home);
    }
    bail!("{kind} 原生历史读取尚无精确可验证适配；宿主终端记录作为补充资料")
}
pub(crate) enum ConversationRunHistory {
    Chat(Value),
    Managed(ManagedRunHistory),
}

pub(crate) struct ManagedRunHistory {
    pub locator: Value,
    pub cwd: PathBuf,
    pub run_id: String,
    pub kind: AgentRuntimeKind,
    pub planner: bool,
    pub profile: PathBuf,
    pub pi_file: Option<PathBuf>,
    pub identity: Option<Value>,
}

pub(crate) fn read_conversation_run(source: &ConversationRunHistory) -> Result<Vec<TranscriptMessage>> {
    match source {
        ConversationRunHistory::Chat(locator) => read_chat_run(locator),
        ConversationRunHistory::Managed(run) => run.read(),
    }
}

fn read_chat_run(locator: &Value) -> Result<Vec<TranscriptMessage>> {
    let cwd = locator["cwd"].as_str().filter(|cwd| !cwd.is_empty()).context("本次运行尚未记录工作目录")?;
    let runtime = locator["runtimeKind"].as_str().or_else(|| match locator["adapter"].as_str() {
        Some("acp") => match locator["agentName"].as_str() {
            Some("Claude Agent" | "Claude Code" | "Claude Code Agent") => Some("claude_code"),
            Some("Codex" | "Codex Agent") => Some("codex"),
            _ => None,
        },
        adapter => adapter,
    });
    match runtime {
        Some("claude_code") => {
            let id = locator["sessionId"].as_str().context("本次运行尚未记录 Claude 会话 ID")?;
            let path = crate::usage::claude_transcript_path(&home("CLAUDE_CONFIG_DIR", ".claude")?, cwd, id)
                .context("Claude 会话 ID 无效")?;
            crate::transcript::read_transcript_file(&path, 200)
        }
        Some("codex") => {
            let id = locator["threadId"].as_str().or_else(|| locator["sessionId"].as_str())
                .filter(|id| !id.is_empty()).context("本次运行尚未记录 Codex 会话 ID")?;
            codex(cwd, id, &home("CODEX_HOME", ".codex")?)
        }
        _ => bail!("此运行未提供可回看的原生会话记录"),
    }
}

fn identifier<'a>(record: &'a Value, field: &str) -> Result<&'a str> {
    record[field].as_str().filter(|id| !id.is_empty() && id.len() <= 256
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
        .with_context(|| format!("本轮未记录有效的原生 {field}"))
}

fn opaque_id<'a>(record: &'a Value, field: &str) -> Result<&'a str> {
    record[field].as_str().filter(|id| !id.is_empty() && id.len() <= 256 && *id == id.trim() && !id.chars().any(char::is_control))
        .with_context(|| format!("本轮未记录有效的 v4 {field}"))
}

fn check_directory(actual: Option<&str>, expected: &Path) -> Result<()> {
    let reported = Path::new(actual.context("原生记录缺少工作目录")?);
    check_recorded_path(reported, expected)
}

fn check_recorded_path(reported: &Path, expected: &Path) -> Result<()> {
    let canonical = expected.canonicalize()?;
    let spelling = crate::multi_agent::external_command_path(reported);
    ensure!(reported.is_absolute() && expected.is_absolute() && (spelling == crate::multi_agent::external_command_path(expected)
        || spelling == crate::multi_agent::external_command_path(&canonical)) && reported.canonicalize()? == canonical,
        "原生记录位置与本轮授权不符");
    Ok(())
}

/// Resolve from an application-owned anchor. Redirecting any descendant to a sibling
/// session or a user's private history must never turn it into a valid native source.
fn owned_path(anchor: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(relative.components().all(|c| matches!(c, std::path::Component::Normal(_))),
        "原生历史路径包含不可信的目录分量");
    let expected = anchor.canonicalize()?.join(relative);
    ensure!(expected.canonicalize()? == expected, "原生历史文件已重定向到不可信位置");
    Ok(expected)
}

fn profile_path(profile: &Path, child: &Path) -> Result<PathBuf> {
    let anchor = profile.parent().and_then(Path::parent).context("缺少托管存储根目录")?;
    owned_path(anchor, &profile.strip_prefix(anchor)?.join(child))
}

fn read_lines(file: std::fs::File, mut visit: impl FnMut(Value) -> Result<()>) -> Result<()> {
    let mut reader = std::io::BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let size = reader.by_ref().take(8 * 1024 * 1024 + 1).read_until(b'\n', &mut line)?;
        if size == 0 { return Ok(()); }
        ensure!(size <= 8 * 1024 * 1024, "托管历史单条记录超过读取上限");
        visit(serde_json::from_slice(&line).context("已定位的托管原生记录损坏")?)?;
    }
}

fn visible_text(value: &Value) -> String {
    if let Some(string) = value.as_str() { return string.to_owned(); }
    value.as_array().into_iter().flatten()
        .filter(|item| matches!(item["type"].as_str(), Some("text" | "input_text" | "output_text")))
        .filter_map(|item| item["text"].as_str()).collect::<Vec<_>>().join("\n")
}

fn add_turn_reply(messages: &mut Vec<TranscriptMessage>, body: String) {
    if messages.len() == 200 { messages.remove(1); }
    messages.push(limited("assistant", body));
}

impl ManagedRunHistory {
    fn read(&self) -> Result<Vec<TranscriptMessage>> {
        check_directory(self.locator["cwd"].as_str(), &self.cwd)?;
        match self.kind {
            AgentRuntimeKind::Pi => self.pi(),
            AgentRuntimeKind::KimiCode | AgentRuntimeKind::OpenCode => {
                let kimi = self.kind == AgentRuntimeKind::KimiCode;
                ensure!(self.locator["engine"] == if kimi { "kimi-code-acp" } else { "opencode-acp" }
                    && self.locator["commandId"] == self.run_id
                    && matches!(self.locator["toolPolicy"].as_str(), Some("text-only-v1" | "scoped-files-v1")),
                    "托管原生引擎、工具策略或执行轮次不匹配");
                profile_path(&self.profile, Path::new("input"))?;
                let work = self.profile.join("input");
                check_directory(self.locator["sessionPath"].as_str(), &work)?;
                if kimi { self.kimi(&work) } else { self.opencode(&work) }
            }
            AgentRuntimeKind::ZCodeCli => self.zcode(),
            AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex => {
                let key = if self.kind == AgentRuntimeKind::Codex && self.locator["threadId"].is_string() { "threadId" } else { "sessionId" };
                let id = identifier(&self.locator, key)?;
                ensure!(self.identity.as_ref().and_then(|proof| proof[key].as_str()) == Some(id),
                    "此历史托管运行缺少独立的原生会话 ID 回执");
                if self.kind == AgentRuntimeKind::Codex {
                    let root = home("CODEX_HOME", ".codex")?;
                    let sessions = owned_path(&root, Path::new("sessions"))?;
                    let suffix = format!("-{id}.jsonl");
                    let mut result = None;
                    let mut pending = vec![sessions];
                    let mut visited = 0;
                    while let Some(directory) = pending.pop() {
                        for entry in std::fs::read_dir(directory)? {
                            let entry = entry?;
                            visited += 1;
                            ensure!(visited <= 100_000, "Codex 历史目录超出读取上限");
                            let kind = entry.file_type()?;
                            if kind.is_dir() {
                                let path = entry.path();
                                if path.canonicalize()? == path { pending.push(path); }
                            } else if kind.is_file() && entry.file_name().to_string_lossy().ends_with(&suffix) {
                                let path = entry.path();
                                ensure!(path.canonicalize()? == path && result.is_none(), "Codex 原生会话文件身份不唯一");
                                result = Some(path);
                            }
                        }
                    }
                    let path = result.context("未找到宿主回执指向的 Codex 会话文件")?;
                    let reader = std::io::BufReader::new(std::fs::File::open(&path)?);
                    let header: Value = serde_json::from_str(&reader.lines().next().context("Codex 原生历史为空")??)?;
                    ensure!(header["type"] == "session_meta" && header["payload"]["id"] == id, "Codex 原生会话 ID 不匹配");
                    check_directory(header["payload"]["cwd"].as_str(), &self.cwd)?;
                    jsonl(&path)
                } else {
                    let root = home("CLAUDE_CONFIG_DIR", ".claude")?;
                    let path = crate::usage::claude_transcript_path(&root, self.cwd.to_str().context("工作目录无法编码")?, id)
                        .context("Claude 原生会话 ID 无效")?;
                    let path = owned_path(&root, path.strip_prefix(&root)?)?;
                    let mut matched = false;
                    read_lines(std::fs::File::open(&path)?, |row| {
                        if matches!(row["type"].as_str(), Some("user" | "assistant")) {
                            ensure!(row["sessionId"] == id, "Claude 记录属于不同的原生会话");
                            check_directory(row["cwd"].as_str(), &self.cwd)?;
                            matched = true;
                        }
                        Ok(())
                    })?;
                    ensure!(matched, "Claude 原生历史缺少本会话的消息");
                    crate::transcript::read_transcript_file(&path, 200)
                }
            }
            _ => bail!("此托管 Runtime 尚未提供经核实的原生历史"),
        }
    }

    fn pi(&self) -> Result<Vec<TranscriptMessage>> {
        let id = identifier(&self.locator, "sessionId")?;
        let expected = self.pi_file.as_ref().context("缺少该任务的 Pi 存储位置")?;
        let anchor = expected.parent().and_then(Path::parent).context("缺少 Pi 存储根目录")?;
        let path = owned_path(anchor, expected.strip_prefix(anchor)?)?;
        if let Some(recorded) = self.locator["sessionPath"].as_str() {
            check_recorded_path(Path::new(recorded), expected).context("Pi 会话文件不属于本轮")?;
        }
        let marker = if self.planner {
            None // Print-mode planning owns exactly one fresh run file.
        } else {
            ensure!(self.locator["engine"] == "pi", "原生引擎与 Pi 托管记录不符");
            Some(format!("[CrewVela message {}]\n", identifier(&self.locator, "messageId")?))
        };
        let mut header = true;
        let mut selected = false;
        let mut active = false;
        let mut settled = false;
        let mut branch_tip = None;
        let mut messages = Vec::new();
        read_lines(std::fs::File::open(&path)?, |entry| {
            if header {
                header = false;
                ensure!(entry["type"] == "session" && entry["id"] == id, "Pi 文件头的原生会话 ID 不匹配");
                return check_directory(entry["cwd"].as_str(), &self.cwd);
            }
            let message = &entry["message"];
            if entry["type"] == "message" && message["role"] == "user" {
                let input = visible_text(&message["content"]);
                active = marker.as_ref().is_none_or(|key| input.starts_with(key));
                if active {
                    ensure!(!selected, "Pi 原生记录中出现多个匹配输入");
                    selected = true;
                    branch_tip = Some(identifier(&entry, "id")?.to_owned());
                    messages.push(limited("user", input));
                }
            } else if active {
                // Pi can branch within a JSONL file. Only the selected input's linear
                // descendants belong to this run; never infer a sibling's reply.
                if entry["parentId"].as_str() != branch_tip.as_deref() { active = false; return Ok(()); }
                branch_tip = Some(identifier(&entry, "id")?.to_owned());
                if entry["type"] == "message" && message["role"] == "assistant" {
                    let body = visible_text(&message["content"]);
                    settled = message["stopReason"] == "stop" && !body.trim().is_empty();
                    if !body.is_empty() { add_turn_reply(&mut messages, body); }
                }
            }
            Ok(())
        })?;
        ensure!(selected && settled, "Pi 没有该消息标识对应的原生输入与完成回复");
        Ok(messages)
    }

    fn kimi(&self, work: &Path) -> Result<Vec<TranscriptMessage>> {
        let id = identifier(&self.locator, "sessionId")?;
        let msg = identifier(&self.locator, "messageId")?;
        let turn = identifier(&self.locator, "turnId")?;
        let number = turn.parse::<u64>().context("Kimi 原生轮次号无效")?;
        let sessions = profile_path(&self.profile, Path::new("kimi/sessions"))?;
        let mut target = None;
        for bucket in std::fs::read_dir(&sessions)? {
            let bucket = bucket?;
            if !bucket.file_type()?.is_dir() { continue; }
            let relative = PathBuf::from(bucket.file_name()).join(id);
            if !sessions.join(&relative).is_dir() { continue; }
            let directory = owned_path(&sessions, &relative)?;
            ensure!(target.replace(directory).is_none(), "Kimi 托管原生会话身份不唯一");
        }
        let target = target.context("未找到本任务私有 Kimi 会话")?;
        let state: Value = serde_json::from_reader(std::fs::File::open(owned_path(&target, Path::new("state.json"))?)?)?;
        ensure!(state["id"] == id, "Kimi 原生会话 ID 不匹配");
        check_directory(state["cwd"].as_str(), work)?;
        let prefix = format!("CrewVela 托管轮次 {}。\n", self.run_id);
        let mut messages = Vec::new();
        let mut closed = false;
        read_lines(std::fs::File::open(owned_path(&target, Path::new("agents/main/wire.jsonl"))?)?, |row| {
            let event = row.get("event").unwrap_or(&row);
            if event["turnId"].as_u64() != Some(number) && event["turnId"].as_str() != Some(turn) { return Ok(()); }
            match event["type"].as_str() {
                Some("turn.prompt") => {
                    let input = visible_text(&event["input"]);
                    ensure!(messages.is_empty() && event["promptId"] == msg && input.starts_with(&prefix),
                        "Kimi 本轮的用户输入和托管回执不匹配");
                    messages.push(limited("user", input));
                }
                Some("content.part") if event["part"]["type"] == "text" => {
                    ensure!(!closed && !messages.is_empty(), "Kimi 原生回复缺少对应的本轮输入");
                    if let Some(body) = event["part"]["text"].as_str().filter(|s| !s.is_empty()) {
                        add_turn_reply(&mut messages, body.to_owned());
                    }
                }
                Some("turn.ended") => {
                    ensure!(!closed && event["reason"] == "completed", "Kimi 原生轮次没有正常完成");
                    closed = true;
                }
                _ => {}
            }
            Ok(())
        })?;
        ensure!(closed && messages.len() >= 2, "未找到 Kimi 本轮完整的输入、回复及终态");
        Ok(messages)
    }

    fn opencode(&self, work: &Path) -> Result<Vec<TranscriptMessage>> {
        let id = identifier(&self.locator, "sessionId")?;
        let user = identifier(&self.locator, "messageId")?;
        let assistant = identifier(&self.locator, "turnId")?;
        let storage = profile_path(&self.profile, Path::new("data/opencode"))?;
        for name in ["opencode.db-wal", "opencode.db-shm", "opencode.db-journal"] {
            match storage.join(name).symlink_metadata() {
                Ok(_) => { owned_path(&storage, Path::new(name))?; }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        ensure!(!storage.join("opencode.db-wal").exists() || storage.join("opencode.db-shm").is_file(),
            "OpenCode 写入中的 WAL 缺少可只读打开的共享索引");
        let mut connection = db(&owned_path(&storage, Path::new("opencode.db"))?)?;
        let snapshot = connection.transaction()?;
        let cwd: String = snapshot.query_row("SELECT directory FROM session WHERE id=?1", [id], |row| row.get(0))
            .context("未找到该任务的 OpenCode 原生会话")?;
        check_directory(Some(&cwd), work)?;
        let mut query = snapshot.prepare(
            "SELECT m.id,json_extract(p.data,'$.text') FROM message u JOIN message a ON a.session_id=u.session_id
             JOIN message m ON m.id IN (u.id,a.id) JOIN part p ON p.message_id=m.id AND p.session_id=m.session_id
             WHERE u.session_id=?1 AND u.id=?2 AND a.id=?3 AND json_extract(u.data,'$.role')='user'
             AND json_extract(a.data,'$.role')='assistant' AND json_extract(a.data,'$.parentID')=u.id
             AND json_extract(a.data,'$.error') IS NULL AND json_extract(a.data,'$.time.completed') IS NOT NULL
             AND json_extract(a.data,'$.finish')='stop' AND json_extract(p.data,'$.type')='text'
             ORDER BY m.id!=u.id,p.time_created,p.id")?;
        let parts = query.query_map(rusqlite::params![id, user, assistant], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?)))?;
        let mut input = Vec::new();
        let mut answer = Vec::new();
        for part in parts {
            let (message, body) = part?;
            if message == user { input.push(body); } else { answer.push(body); }
        }
        let input = input.join("\n");
        let answer = answer.join("\n");
        ensure!(input.starts_with(&format!("CrewVela 托管轮次 {}。\n", self.run_id)) && !answer.trim().is_empty(),
            "OpenCode 本轮的原生输入和最终回复不匹配");
        Ok(vec![limited("user", input), limited("assistant", answer)])
    }

    fn zcode(&self) -> Result<Vec<TranscriptMessage>> {
        ensure!(self.locator["engine"] == "zcode-v4", "ZCode 运行缺少匹配的 v4 原生引擎");
        let name = zcode_record_name(&self.run_id)?;
        let path = profile_path(&self.profile, Path::new(&name))
            .context("此 ZCode 运行没有可核实的私有 v4 轮次副本")?;
        let record: Value = serde_json::from_reader(std::fs::File::open(path)?.take(2 * 1024 * 1024))?;
        ensure!(record["version"] == 1 && record["runId"] == self.run_id && ["sessionId","commandId","turnId"].iter()
            .all(|key| opaque_id(&self.locator, key).is_ok_and(|id| record[*key] == id)), "ZCode 私有记录的会话或轮次身份不匹配");
        check_directory(record["cwd"].as_str(), &self.cwd)?;
        let prompt = record["input"].as_str().filter(|s| !s.is_empty()).context("ZCode 轮次副本缺少已投递输入")?;
        let reply = record["reply"].as_str().filter(|s| !s.is_empty()).context("ZCode 轮次副本缺少最终回复")?;
        Ok(vec![limited("user", prompt.to_owned()), limited("assistant", reply.to_owned())])
    }
}

fn zcode_record_name(run_id: &str) -> Result<String> {
    let id = uuid::Uuid::parse_str(run_id).context("ZCode 托管运行 ID 无效")?;
    Ok(format!("{id}.json"))
}

pub(crate) fn archive_zcode_turn(profile: &Path, run_id: &str, cwd: &Path, locator: &Value, input: &str, reply: &str, token: &str) -> Result<()> {
    let name = zcode_record_name(run_id)?;
    let parent = profile.parent().context("缺少 ZCode 托管存储根目录")?;
    let anchor = parent.parent().context("缺少托管数据目录")?;
    for directory in [parent, profile] {
        let mut options = std::fs::DirBuilder::new();
        #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; options.mode(0o700); }
        match options.create(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        ensure!(owned_path(anchor, directory.strip_prefix(anchor)?)?.is_dir(), "ZCode 私有记录目录无效");
    }
    let directory = profile_path(profile, Path::new(""))?;
    let path = directory.join(name);
    ensure!(path.symlink_metadata().is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound), "ZCode 本轮归档路径已占用");
    let clean = |text: &str| {
        let safe = if token.is_empty() { text.to_owned() } else { text.replace(token, "[REDACTED]") };
        if safe.chars().count() <= 64 * 1024 { safe }
        else { format!("{} [归档截断]", safe.chars().take(64 * 1024).collect::<String>()) }
    };
    let data = serde_json::json!({"version":1,"runId":run_id,"cwd":cwd,"sessionId":opaque_id(locator,"sessionId")?,
        "commandId":opaque_id(locator,"commandId")?,"turnId":opaque_id(locator,"turnId")?,"input":clean(input),"reply":clean(reply)});
    crate::providers::write_atomic(&path, &serde_json::to_vec(&data)?)
}

fn codex(expected_cwd: &str, locator: &str, home: &Path) -> Result<Vec<TranscriptMessage>> {
    let mut pending = vec![home.join("sessions")];
    let mut visited = 0;
    while let Some(path) = pending.pop() {
        visited += 1;
        if visited > 100000 {
            bail!("Codex 历史目录超出读取上限");
        }
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            // Until session_meta proves identity, a broken neighbor is not
            // evidence that this exact session is corrupt.
            let Ok(file) = std::fs::File::open(&path) else { continue; };
            let reader = std::io::BufReader::new(file);
            if let Some(Ok(line)) = reader.lines().next() {
                let Ok(value) = serde_json::from_str::<Value>(&line) else { continue; };
                if value["type"] == "session_meta" && value["payload"]["id"] == locator {
                    let cwd = value["payload"]["cwd"]
                        .as_str()
                        .context("Codex 历史缺少目录")?;
                    if Path::new(cwd).canonicalize()?
                        != Path::new(expected_cwd).canonicalize()?
                    {
                        bail!("Codex 历史目录身份不匹配");
                    }
                    return jsonl(&path);
                }
            }
        }
    }
    bail!("精确 Codex 原生历史不存在");
}

/// A Codex notify command is also inherited by ephemeral title/subagent
/// threads. Only an exact, persisted interactive thread may bind a terminal.
pub(crate) fn codex_interactive_identity(
    record: &SessionRecord,
    locator: &str,
    codex_home: Option<&Path>,
    config_root: Option<&Path>,
) -> Result<bool> {
    if locator.is_empty() || locator.len() > 256
        || !locator.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Ok(false);
    }
    let root = match codex_home {
        Some(root) => root.to_path_buf(),
        None => home("CODEX_HOME", ".codex")?,
    };
    let sessions = root.join("sessions");
    if !sessions.exists() { return Ok(false); }
    let suffix = format!("-{locator}.jsonl");
    let mut pending = vec![sessions];
    let mut visited = 0;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            visited += 1;
            if visited > 100_000 { bail!("Codex 身份目录超出读取上限"); }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.file_name().to_string_lossy().ends_with(&suffix) {
                let reader = std::io::BufReader::new(std::fs::File::open(entry.path())?);
                let Some(line) = reader.lines().next().transpose()? else { continue; };
                let Ok(meta) = serde_json::from_str::<Value>(&line) else { continue; };
                if meta["type"] == "session_meta"
                    && meta["payload"]["id"] == locator
                    && meta["payload"]["source"] == "cli"
                    && meta["payload"]["cwd"].as_str().is_some_and(|cwd| {
                        if same_cwd(cwd, record) { return true; }
                        // Read-only project CLI turns run from this exact
                        // per-member scratch directory while record.cwd keeps
                        // the business/message workspace identity.
                        let (Some(root), Some(project), Some(agent)) = (
                            config_root, record.multi_agent_project_id.as_deref(), record.multi_agent_agent_id.as_deref(),
                        ) else { return false; };
                        let safe = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
                        if !safe(project) || !safe(agent) { return false; }
                        let scratch = root.join("multi-agent-readonly").join(project).join(agent);
                        matches!((Path::new(cwd).canonicalize(), scratch.canonicalize()), (Ok(a), Ok(b)) if a == b)
                    })
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

pub(crate) fn resume_locator(record: &SessionRecord, sessions: &SessionManager) -> Result<String> {
    if matches!(crate::session_switch::source_runtime(record), "minimax_code" | "oh_my_pi") { bail!("该 CLI 已移除，不能恢复历史会话"); }
    if let Some(locator) = sessions.runtime_history_locator(record)? {
        return Ok(locator);
    }
    match crate::session_switch::source_runtime(record) {
        "claude_code" if !record.claude_session_id.is_empty() => {
            Ok(record.claude_session_id.clone())
        }
        "kimi_code" => kimi(record, None).map(|(id, _)| id),
        "opencode" => sqlite_agent(record, None).map(|(id, _)| id),
        _ => {
            bail!("尚未记录接手会话的精确原生 ID；不能新建空会话替代历史，请核对接力详情或撤销接力")
        }
    }
}
fn home(variable: &str, fallback: &str) -> Result<PathBuf> {
    Ok(std::env::var_os(variable)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(
            dirs::home_dir()
                .context("无法定位 Agent 历史目录")?
                .join(fallback),
        ))
}
fn same_cwd(cwd: &str, record: &SessionRecord) -> bool {
    match (
        Path::new(cwd).canonicalize(),
        Path::new(&record.cwd).canonicalize(),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
fn marker_message(content: &str, marker: &str) -> bool {
    // A later Agent can quote the entire handoff. Match only the actual input
    // prefix, never a marker embedded inside a later conversation's background.
    let content = content.trim_start();
    content.starts_with(marker)
        || content
            .strip_prefix("[CrewVela message ")
            .and_then(|s| s.split_once("]\n"))
            .is_some_and(|(_, body)| body.starts_with(marker))
}
fn limited(role: &str, content: String) -> TranscriptMessage {
    let text = if role == "user" || content.chars().count() <= 4000 {
        content
    } else {
        format!(
            "{} [此条截断]",
            content.chars().take(4000).collect::<String>()
        )
    };
    TranscriptMessage {
        role: role.into(),
        text,
        at: None,
    }
}
fn db(path: &Path) -> Result<rusqlite::Connection> {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(2))?;
    Ok(connection)
}
fn unique(ids: Vec<String>) -> Result<String> {
    let mut ids = ids;
    ids.sort();
    ids.dedup();
    if ids.len() != 1 {
        bail!("无法唯一核实原生会话身份；不会按目录或时间猜测会话");
    }
    Ok(ids.remove(0))
}
fn kimi(record: &SessionRecord, locator: Option<&str>) -> Result<(String, Vec<TranscriptMessage>)> {
    let root = home("KIMI_CODE_HOME", ".kimi-code")?;
    let marker = crate::session_switch::incoming_marker(&record.id)?;
    kimi_at(record, locator, &root, marker.as_deref())
}

fn kimi_at(record: &SessionRecord, locator: Option<&str>, root: &Path, marker: Option<&str>) -> Result<(String, Vec<TranscriptMessage>)> {
    let canonical_root = root.canonicalize()?;
    let reader = std::io::BufReader::new(std::fs::File::open(root.join("session_index.jsonl"))?);
    let mut entries = std::collections::HashMap::new();
    for line in reader.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(&line?) {
            if let (Some(id), Some(dir)) =
                (value["sessionId"].as_str(), value["sessionDir"].as_str())
            {
                entries.insert(id.to_owned(), dir.to_owned());
            }
        }
    }
    let mut found = Vec::new();
    for (id, dir) in entries {
        if locator.is_some_and(|expected| expected != id) {
            continue;
        }
        let candidate = (|| -> Result<(PathBuf, Value)> {
            let directory = PathBuf::from(&dir).canonicalize()?;
            anyhow::ensure!(directory.starts_with(&canonical_root), "Kimi 历史位于原生目录之外");
            let metadata = serde_json::from_slice(&std::fs::read(directory.join("state.json"))?)?;
            Ok((directory, metadata))
        })();
        let (directory, metadata) = match candidate {
            Ok(candidate) => candidate,
            Err(error) if locator.is_some() => return Err(error.context("精确 Kimi 历史状态损坏")),
            Err(_) => continue,
        };
        let cwd = metadata["cwd"]
            .as_str()
            .or_else(|| metadata["workDir"].as_str())
            .or_else(|| metadata.pointer("/custom/cwd").and_then(Value::as_str));
        if !cwd.is_some_and(|cwd| same_cwd(cwd, record)) {
            continue;
        }
        if let Some(messages) = kimi_wire(&directory.join("agents/main/wire.jsonl"), marker, locator.is_some())? {
            found.push((id, messages));
        }
    }
    if found.len() != 1 {
        bail!("Kimi 未记录唯一原生历史定位，或版本格式尚未适配");
    }
    Ok(found.remove(0))
}
fn kimi_wire(path: &Path, marker: Option<&str>, mut known_target: bool) -> Result<Option<Vec<TranscriptMessage>>> {
    let mut messages = VecDeque::new();
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if known_target => return Err(error.into()),
        Err(_) => return Ok(None),
    };
    for line in std::io::BufReader::new(file).lines() {
        let parsed = line.map_err(anyhow::Error::from).and_then(|line| Ok(serde_json::from_str::<Value>(&line)?));
        let value = match parsed {
            Ok(value) => value,
            Err(error) if known_target => return Err(error.context("已确认 Kimi 历史损坏")),
            Err(_) => return Ok(None),
        };
        let event = value
            .get("event")
            .filter(|_| value["type"] == "agent")
            .unwrap_or(&value);
        if event["type"] == "context.append_message" {
            let message = &event["message"];
            if let Some(role) = message["role"].as_str() {
                messages.push_back(limited(role, text(&message["content"])));
            }
        } else if event["type"] == "turn_begin" {
            if let Some(input) = event["userInput"].as_str() {
                messages.push_back(limited("user", input.into()));
            }
        }
        if marker.is_some_and(|marker| messages.back().is_some_and(|message| message.role == "user" && marker_message(&message.text, marker))) {
            known_target = true;
        }
        while messages.len() > 200 {
            messages.pop_front();
        }
    }
    Ok(known_target.then(|| messages.into_iter().collect()))
}
fn sqlite_agent(record: &SessionRecord, locator: Option<&str>) -> Result<(String, Vec<TranscriptMessage>)> {
    let root = home("XDG_DATA_HOME", ".local/share")?.join("opencode");
    let path = std::env::var_os("OPENCODE_DB").map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { root.join(p) }).unwrap_or(root.join("opencode.db"));
    sqlite_history(&db(&path)?, record, locator, crate::session_switch::incoming_marker(&record.id)?.as_deref())
}
fn sqlite_history(connection: &rusqlite::Connection, record: &SessionRecord, locator: Option<&str>, marker: Option<&str>) -> Result<(String, Vec<TranscriptMessage>)> {
    let mut query = connection.prepare("SELECT id,directory FROM session")?;
    let rows = query.query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?
        .collect::<std::result::Result<Vec<_>,_>>()?;
    let mut found = Vec::new();
    for (id,cwd) in rows {
        if locator.is_some_and(|expected| expected != id) || !same_cwd(&cwd,record) { continue; }
        let messages = opencode_messages(connection,&id)?;
        if locator.is_some() || marker.is_some_and(|marker| messages.iter().any(|m| m.role=="user" && marker_message(&m.text,marker))) { found.push((id,messages)); }
    }
    unique(found.iter().map(|(id,_)| id.clone()).collect())?;
    Ok(found.remove(0))
}
fn opencode_messages(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Vec<TranscriptMessage>> {
    let mut query = connection.prepare("SELECT m.data,p.data FROM message m JOIN part p ON p.message_id=m.id WHERE m.session_id=?1 ORDER BY m.time_created DESC,m.id DESC,p.time_created DESC LIMIT 400")?;
    let mut messages = query
        .query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|row| -> Option<Result<_>> {
            Some((|| {
                let (info, data) = row?;
                let info: Value = serde_json::from_str(&info)?;
                let data: Value = serde_json::from_str(&data)?;
                if data["type"] != "text" {
                    return Ok(None);
                }
                Ok(Some(limited(
                    info["role"].as_str().unwrap_or("tool"),
                    text(&data),
                )))
            })())
        })
        .collect::<Result<Vec<Option<TranscriptMessage>>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    messages.reverse();
    Ok(messages)
}
fn text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .map(text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    for field in ["text", "content", "output", "arguments"] {
        if let Some(value) = value.get(field) {
            return text(value);
        }
    }
    String::new()
}
fn jsonl(path: &Path) -> Result<Vec<TranscriptMessage>> {
    let reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut messages = VecDeque::new();
    for line in reader.lines() {
        let line = line?;
        if line.len() > 8 * 1024 * 1024 {
            bail!("原生历史单行超出读取上限");
        }
        let value: Value = serde_json::from_str(&line)?;
        let message = value
            .get("message")
            .or_else(|| value.get("payload"))
            .unwrap_or(&value);
        let role = message["role"]
            .as_str()
            .or_else(|| match message["type"].as_str() {
                Some("function_call" | "function_call_output") => Some("tool"),
                _ => None,
            });
        if let Some(role) = role {
            let content = message
                .get("content")
                .or_else(|| message.get("text"))
                .or_else(|| message.get("output"))
                .or_else(|| message.get("arguments"))
                .map(text)
                .unwrap_or_default();
            if !content.is_empty() {
                let text = if role == "user" {
                    content
                } else if content.chars().count() > 4000 {
                    format!(
                        "{} [此条截断]",
                        content.chars().take(4000).collect::<String>()
                    )
                } else {
                    content
                };
                messages.push_back(TranscriptMessage {
                    role: role.into(),
                    text,
                    at: value["timestamp"].as_str().map(str::to_owned),
                });
            }
            while messages.len() > 200 {
                messages.pop_front();
            }
        }
    }
    Ok(messages.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::NewRuntimeSession;
    use serde_json::json;

    fn managed(root: &Path, kind: AgentRuntimeKind, run_id: &str) -> ManagedRunHistory {
        let profile = root.join("store.managed-runtime").join(kind.as_str());
        ManagedRunHistory {
            locator: json!({"cwd":root, "adapter":kind, "managedTurn":"completed"}),
            cwd: root.into(), run_id: run_id.into(), kind, planner: false, profile,
            pi_file: None, identity: None,
        }
    }

    #[test]
    fn managed_pi_limits_readback_to_the_received_message_in_the_owned_file() -> Result<()> {
        let (root, _sessions, _) = isolated_history("pi");
        let directory = root.join("multi-agent-sessions");
        std::fs::create_dir(&directory)?;
        let file = directory.join("this-run.jsonl");
        let mut run = managed(&root, AgentRuntimeKind::Pi, "run");
        run.pi_file = Some(file.clone());
        run.locator = json!({"sessionId":"pi-session","cwd":root,"sessionPath":file,"messageId":"task-two","engine":"pi"});
        let rows = [
            json!({"type":"session","id":"pi-session","cwd":root}),
            json!({"type":"message","id":"a","message":{"role":"user","content":"[CrewVela message task-one]\nfirst"}}),
            json!({"type":"message","id":"b","parentId":"a","message":{"role":"assistant","content":"earlier reply"}}),
            json!({"type":"message","id":"c","parentId":"b","message":{"role":"user","content":[{"type":"text","text":"[CrewVela message task-two]\nsecond"}]}}),
            json!({"type":"message","id":"d","parentId":"c","message":{"role":"assistant","stopReason":"stop","content":[{"type":"thinking","text":"private reasoning"},{"type":"text","text":"selected reply"}]}}),
            json!({"type":"message","id":"e","parentId":"d","message":{"role":"user","content":"neighbor"}}),
            json!({"type":"message","id":"f","parentId":"e","message":{"role":"assistant","content":"later reply"}}),
        ];
        std::fs::write(&file, rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))?;
        let view = run.read()?;
        assert_eq!(view.len(), 2);
        assert_eq!(view[1].text, "selected reply");
        run.locator["messageId"] = "task-missing".into();
        assert!(run.read().is_err());
        run.locator["messageId"] = "task-two".into();
        run.locator["sessionId"] = "other-session".into();
        assert!(run.read().is_err());
        run.locator["sessionId"] = "pi-session".into();
        let other = root.join("private.jsonl");
        std::fs::copy(&file, &other)?;
        run.locator["sessionPath"] = json!(other);
        assert!(run.read().is_err());
        run.locator["sessionPath"] = json!(file);
        run.locator["cwd"] = json!(directory);
        assert!(run.read().is_err());
        drop(_sessions);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn managed_zcode_archives_only_the_matched_command_and_redacts_its_token() -> Result<()> {
        let (root, sessions, _) = isolated_history("zcode_cli");
        let mut run = managed(&root, AgentRuntimeKind::ZCodeCli, &uuid::Uuid::new_v4().to_string());
        run.locator = json!({"cwd":root,"engine":"zcode-v4","sessionId":"v4-session","commandId":"command","turnId":"turn:7"});
        assert!(run.read().is_err());
        archive_zcode_turn(&run.profile, &run.run_id, &run.cwd, &run.locator, "indexed secret", "finished secret", "secret")?;
        let view = run.read()?;
        assert_eq!(view[0].text, "indexed [REDACTED]");
        assert_eq!(view[1].text, "finished [REDACTED]");
        for key in ["sessionId", "commandId", "turnId"] {
            let original = run.locator[key].clone();
            run.locator[key] = "other".into();
            assert!(run.read().is_err());
            run.locator[key] = original;
        }
        let outside = root.join("unrelated-private-directory");
        std::fs::rename(&run.profile, &outside)?;
        #[cfg(windows)] junction::create(&outside, &run.profile)?;
        #[cfg(unix)] std::os::unix::fs::symlink(&outside, &run.profile)?;
        assert!(run.read().is_err());
        assert!(archive_zcode_turn(&run.profile, &uuid::Uuid::new_v4().to_string(), &run.cwd, &run.locator, "a", "b", "").is_err());
        #[cfg(windows)] junction::delete(&run.profile)?;
        #[cfg(unix)] std::fs::remove_file(&run.profile)?;
        drop(sessions);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Kimi Code/OpenCode and Node; uses isolated accounts and a loopback model"]
    fn managed_history_reads_two_real_native_acp_turns_without_mixing_retries() -> Result<()> {
        let (root, sessions, _) = isolated_history("kimi_code");
        std::fs::write(root.join("marker"), b"isolated-managed-acp")?;
        let cli_case = "multi_agent::executor::managed_cli::tests::installed_kimi_and_opencode_enforce_text_only_and_resume_exact_acp_sessions";
        let output = std::process::Command::new(std::env::current_exe()?).args(["--exact", cli_case, "--ignored", "--nocapture"])
            .env("CREWVELA_MANAGED_ACP_PROBE", &root).env("KIMI_CODE_HOME",root.join("original-kimi"))
            .env("XDG_CONFIG_HOME",root.join("original-config")).env("XDG_DATA_HOME",root.join("original-data"))
            .env("OPENCODE_AUTH_CONTENT","{}").env("OPENCODE_DISABLE_MODELS_FETCH","1").env_remove("OPENCODE_CONFIG")
            .env_remove("OPENCODE_CONFIG_DIR").env_remove("OPENCODE_CONFIG_CONTENT").env_remove("KIMI_MODEL_NAME").output()?;
        ensure!(output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "隔离原生 ACP 用例失败：{}", String::from_utf8_lossy(&output.stderr));
        for kind in [AgentRuntimeKind::KimiCode, AgentRuntimeKind::OpenCode] {
            let profile = root.join(kind.as_str());
            let work = profile.join("input");
            for stage in ["first-crewvela-deny-read-write", "resumed-crewvela-deny-read-write"] {
                let prompt = format!("CrewVela 托管轮次 {stage}。\n{stage} {}", kind.as_str());
                let (id, user, turn): (String, String, String) = if kind == AgentRuntimeKind::KimiCode {
                    let index = std::io::BufReader::new(std::fs::File::open(profile.join("kimi/session_index.jsonl"))?);
                    let row: Value = serde_json::from_str(&index.lines().next().context("missing fixture index")??)?;
                    let directory = Path::new(row["sessionDir"].as_str().context("missing fixture session")?);
                    let state: Value = serde_json::from_reader(std::fs::File::open(directory.join("state.json"))?)?;
                    let mut ids = None;
                    read_lines(std::fs::File::open(directory.join("agents/main/wire.jsonl"))?, |row| {
                        let event = row.get("event").unwrap_or(&row);
                        if event["type"] == "turn.prompt" && visible_text(&event["input"]) == prompt {
                            ids = Some((event["promptId"].as_str().unwrap().to_owned(), event["turnId"].to_string()));
                        }
                        Ok(())
                    })?;
                    let (user, turn) = ids.context("fixture did not persist the prompted turn")?;
                    (state["id"].as_str().unwrap().to_owned(), user, turn)
                } else {
                    db(&profile.join("data/opencode/opencode.db"))?.query_row(
                        "SELECT u.session_id,u.id,a.id FROM message u JOIN part p ON p.message_id=u.id
                         JOIN message a ON json_extract(a.data,'$.parentID')=u.id
                         WHERE json_extract(p.data,'$.text')=?1 AND json_extract(a.data,'$.finish')='stop'",
                        [&prompt],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?
                };
                let mut run = managed(&root, kind, stage);
                run.profile = profile.clone();
                run.locator = json!({"engine":if kind==AgentRuntimeKind::KimiCode {"kimi-code-acp"} else {"opencode-acp"},
                    "cwd":root,"toolPolicy":"text-only-v1","sessionPath":work,"commandId":stage,"sessionId":id,"turnId":turn,"messageId":user});
                let view = run.read()?;
                assert_eq!(view.len(), 2);
                assert_eq!(view[0].text, prompt);
                assert_eq!(view[1].text, "CREWVELA_PROTOCOL_PROOF");
                run.locator["messageId"] = "unrelated-input".into();
                assert!(run.read().is_err());
                run.locator["messageId"] = user.into();
                run.locator["sessionPath"] = json!(root);
                assert!(run.read().is_err());
            }
        }
        drop(sessions);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
    fn isolated_history(kind: &str) -> (PathBuf, SessionManager, SessionRecord) {
        let root = std::env::temp_dir().join(format!("history-neighbors-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions.create_runtime(NewRuntimeSession { runtime_kind: kind.into(), cwd: root.display().to_string(), project_id: None }).unwrap();
        (root, sessions, record)
    }

    #[test]
    fn codex_bad_neighbor_does_not_block_exact_identity_but_target_corruption_fails() {
        let (root, sessions, record) = isolated_history("codex");
        let directory = root.join("sessions");
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("a-target.jsonl");
        let meta = serde_json::json!({"type":"session_meta", "payload":{"id":"exact-id", "cwd":record.cwd}}).to_string();
        let user = serde_json::json!({"type":"response_item", "payload":{"role":"user", "content":[{"type":"input_text","text":"exact history"}]}}).to_string();
        std::fs::write(&target, format!("{meta}\n{user}\n")).unwrap();
        std::fs::write(directory.join("z-bad-neighbor.jsonl"), "{broken neighbor\n").unwrap();
        let messages = codex(&record.cwd, "exact-id", &root).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "exact history");
        std::fs::write(&target, format!("{meta}\n{{broken target\n")).unwrap();
        assert!(codex(&record.cwd, "exact-id", &root).is_err());
        std::fs::write(&target, "{broken session_meta\n").unwrap();
        assert!(codex(&record.cwd, "exact-id", &root).is_err());
        let wrong = serde_json::json!({"type":"session_meta", "payload":{"id":"exact-id", "cwd":directory}});
        std::fs::write(&target, format!("{wrong}\n{user}\n")).unwrap();
        assert!(codex(&record.cwd, "exact-id", &root).is_err());
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn kimi_skips_unidentified_broken_neighbors_but_rejects_identified_corruption_and_ambiguity() {
        let (root, sessions, record) = isolated_history("kimi_code");
        let target = root.join("target");
        let corrupt_state = root.join("bad-state");
        let corrupt_wire = root.join("bad-wire");
        for directory in [&target, &corrupt_state, &corrupt_wire] {
            std::fs::create_dir_all(directory.join("agents/main")).unwrap();
            std::fs::write(directory.join("state.json"), serde_json::json!({"cwd":record.cwd}).to_string()).unwrap();
        }
        std::fs::write(corrupt_state.join("state.json"), "{broken state").unwrap();
        std::fs::write(corrupt_wire.join("agents/main/wire.jsonl"), "{broken unrelated wire\n").unwrap();
        let index: String = [("exact", &target), ("bad-state", &corrupt_state), ("bad-wire", &corrupt_wire), ("dangling", &root.join("missing"))]
            .iter().map(|(id,directory)| format!("{}\n", serde_json::json!({"sessionId":id,"sessionDir":directory}))).collect();
        std::fs::write(root.join("session_index.jsonl"), &index).unwrap();
        let marker = "[CrewVela handoff exact-history]";
        let input = serde_json::json!({"type":"turn_begin", "userInput":format!("{marker}\nexact message")}).to_string();
        let wire = target.join("agents/main/wire.jsonl");
        std::fs::write(&wire, format!("{input}\n")).unwrap();
        for locator in [None, Some("exact")] {
            let (id, messages) = kimi_at(&record, locator, &root, Some(marker)).unwrap();
            assert_eq!(id, "exact");
            assert_eq!(messages.len(), 1);
            assert!(messages[0].text.starts_with(marker));
        }
        // Once the actual marker proves the fallback's identity, later damage
        // must remain visible instead of producing partial successful history.
        std::fs::write(&wire, format!("{input}\n{{broken target wire\n")).unwrap();
        assert!(kimi_at(&record, None, &root, Some(marker)).unwrap_err().to_string().contains("已确认 Kimi 历史损坏"));
        assert!(kimi_at(&record, Some("exact"), &root, Some(marker)).is_err());
        std::fs::write(target.join("state.json"), "{broken target state").unwrap();
        assert!(kimi_at(&record, Some("exact"), &root, Some(marker)).unwrap_err().to_string().contains("精确 Kimi 历史状态损坏"));
        assert!(kimi_at(&record, Some("dangling"), &root, Some(marker)).is_err());
        std::fs::write(target.join("state.json"), serde_json::json!({"cwd":record.cwd}).to_string()).unwrap();
        std::fs::write(&wire, format!("{input}\n")).unwrap();
        std::fs::write(corrupt_wire.join("agents/main/wire.jsonl"), format!("{input}\n")).unwrap();
        assert!(kimi_at(&record, None, &root, Some(marker)).is_err(), "two exact input markers cannot pick an arbitrary session");
        let mut wrong = record;
        wrong.cwd = target.display().to_string();
        assert!(kimi_at(&wrong, Some("exact"), &root, Some(marker)).is_err());
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn history_reader_preserves_long_user_tail_and_marks_other_truncation() {
        let file =
            std::env::temp_dir().join(format!("cc-handoff-history-{}.jsonl", uuid::Uuid::new_v4()));
        let instruction = format!("{}不能遗漏结尾约束", "正文".repeat(3500));
        std::fs::write(&file, format!("{}\n{}\n", serde_json::json!({"message":{"role":"user","content":[{"type":"text","text":instruction}]}}), serde_json::json!({"message":{"role":"assistant","content":"长".repeat(6000)}}))).unwrap();
        let messages = jsonl(&file).unwrap();
        assert_eq!(messages[0].text, instruction);
        assert!(messages[1].text.ends_with("[此条截断]"));
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn sqlite_lookup_requires_exact_input_marker_and_matching_workspace() {
        let root = std::env::temp_dir().join(format!("cc-handoff-db-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let sessions =
            SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "opencode".into(),
                cwd: root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE session(id TEXT,directory TEXT); CREATE TABLE message(id TEXT,session_id TEXT,data TEXT,time_created INTEGER); CREATE TABLE part(message_id TEXT,data TEXT,time_created INTEGER);").unwrap();
        let marker = "[CrewVela handoff synthetic-exact-id]";
        for (id, input) in [
            ("exact", format!("{marker}\n继续工作")),
            ("quoted", format!("另一任务的引用背景：{marker}")),
        ] {
            connection
                .execute(
                    "INSERT INTO session VALUES(?1,?2)",
                    rusqlite::params![
                        id,
                        record.cwd.clone()
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO message VALUES(?1,?1,?2,1)",
                    rusqlite::params![id, serde_json::json!({"role":"user"}).to_string()],
                )
                .unwrap();
            connection.execute("INSERT INTO part VALUES(?1,?2,1)", rusqlite::params![id,serde_json::json!({"type":"text","text":input}).to_string()]).unwrap();
        }
        let (id, messages) =
            sqlite_history(&connection, &record, None, Some(marker)).unwrap();
        assert_eq!(id, "exact");
        assert!(messages[0].text.starts_with(marker));
        assert!(sqlite_history(&connection, &record, None, Some("missing-marker")).is_err());
        let mut wrong = record;
        wrong.cwd = root.join("wrong").display().to_string();
        assert!(sqlite_history(&connection, &wrong, Some("exact"), Some(marker)).is_err());
        drop(sessions);
        std::fs::remove_dir_all(root).unwrap();
    }
}

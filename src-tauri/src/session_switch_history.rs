use crate::{
    sessions::{SessionKind, SessionManager, SessionRecord},
    transcript::TranscriptMessage,
};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::BufRead,
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
pub(crate) fn read_conversation_run(locator: &Value) -> Result<Vec<TranscriptMessage>> {
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

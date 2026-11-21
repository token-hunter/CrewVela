//! The interactive Codex CLI accepts an initial prompt as a single literal
//! argument. Its own onboarding, trust and approval dialogs retain the keyboard.
use super::{QueuedMessage, SessionActionQueue};
use crate::sessions::{SessionManager, SessionRecord};
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Destination {
    thread: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Unsent,
    AwaitingReceipt,
    Confirmed,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Attempt {
    target: String,
    digest: String,
    pub(crate) phase: Phase,
    started_ms: i64,
    pub(crate) thread: Option<String>,
    // Only the compact file-reference prompt differs from the switch record.
    pub(crate) reference_prompt: Option<String>,
}

fn prompt(body: &str) -> String {
    body.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end()
        .replace('\0', "")
}
fn digest(body: &str) -> String {
    format!("{:x}", Sha256::digest(body.as_bytes()))
}
fn attempt_path(root: &Path, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).context("Codex 交接消息 ID 无效")?;
    Ok(root
        .join("session-switch-deliveries")
        .join(format!("{id}.json")))
}

pub(crate) fn read_attempt(
    root: &Path,
    id: &str,
    target: &str,
    body: &str,
) -> Result<Option<Attempt>> {
    let bytes = match fs::read(attempt_path(root, id)?) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let record: Attempt = serde_json::from_slice(&bytes)?;
    ensure!(
        record.target == target && record.digest == digest(body),
        "Codex 交接投递记录与当前目标或正文不匹配"
    );
    Ok(Some(record))
}

fn fits(command: &Command, text: &str) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        // Bound CreateProcessW's quoting/escaping, spaces and terminating NUL.
        let units = |arg: &std::ffi::OsStr| -> usize {
            3 + arg
                .encode_wide()
                .map(|ch| if ch == 34 || ch == 92 { 2 } else { 1 })
                .sum::<usize>()
        };
        units(command.get_program())
            + command.get_args().map(units).sum::<usize>()
            + units(std::ffi::OsStr::new("--"))
            + units(std::ffi::OsStr::new(text))
            + 1
            <= 32_767
    }
    #[cfg(not(windows))]
    {
        let _ = command;
        text.len() <= 120_000
    }
}

pub(crate) struct Launch {
    path: PathBuf,
    attempt: Attempt,
}

impl Launch {
    pub(crate) fn prepare(
        root: &Path,
        command: &mut Command,
        id: &str,
        target: &str,
        body: &str,
        latest_user: Option<&str>,
    ) -> Result<Option<Self>> {
        if let Some(previous) = read_attempt(root, id, target, body)? {
            if previous.phase == Phase::Confirmed {
                return Ok(None);
            }
            ensure!(
                previous.phase == Phase::Unsent,
                "原生 Codex 已尝试接收本次交接，请核对原线程；未重复发送"
            );
        }
        #[cfg(windows)]
        {
            let program = Path::new(command.get_program());
            ensure!(
                program
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("codex.exe")),
                "当前 Codex 启动器无法安全传入交接资料；请选择原生 Codex CLI 或恢复来源"
            );
        }
        ensure!(
            body.starts_with(&format!("[CrewVela handoff {id}]\n")),
            "Codex 交接正文缺少本次交接标识"
        );
        let path = attempt_path(root, id)?;
        fs::create_dir_all(path.parent().context("缺少交接回执目录")?)?;
        let original = prompt(body);
        let reference_prompt = if fits(command, &original) {
            None
        } else {
            let full = path.with_extension("txt");
            let excerpt = prompt(latest_user.unwrap_or(""))
                .chars()
                .take(1200)
                .collect::<String>();
            let compact = format!("[CrewVela handoff {id}]\n交接资料较长。请先读取工作交接包全文，再按其中的原需求继续；若读取受限，请说明。\n文件：{}\n最近用户指令摘录：{}", full.display(), excerpt);
            ensure!(
                fits(command, &compact),
                "Codex 启动参数超过系统上限，交接资料尚未提交"
            );
            crate::providers::write_atomic(&full, original.as_bytes())?;
            Some(compact)
        };
        command
            .arg("--")
            .arg(reference_prompt.as_deref().unwrap_or(&original));
        Ok(Some(Self {
            path,
            attempt: Attempt {
                target: target.into(),
                digest: digest(body),
                phase: Phase::AwaitingReceipt,
                started_ms: chrono::Utc::now().timestamp_millis(),
                thread: None,
                reference_prompt,
            },
        }))
    }

    pub(crate) fn arm(&self) -> Result<()> {
        crate::providers::write_atomic(&self.path, &serde_json::to_vec(&self.attempt)?)
    }
    #[cfg(windows)]
    pub(crate) fn not_started(&self) -> Result<()> {
        let mut attempt = self.attempt.clone();
        attempt.phase = Phase::Unsent;
        crate::providers::write_atomic(&self.path, &serde_json::to_vec(&attempt)?)
    }
}

fn rollout_received(
    path: &Path,
    home: &Path,
    thread: &str,
    cwd: &str,
    expected: &str,
    imported: bool,
) -> Result<bool> {
    uuid::Uuid::parse_str(thread)?;
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        path.starts_with(home.join("sessions").canonicalize()?)
            && path
                .file_name()
                .is_some_and(|f| f.to_string_lossy().ends_with(&format!("-{thread}.jsonl"))),
        "Codex 原生历史文件归属不匹配"
    );
    let mut lines = BufReader::new(fs::File::open(path)?).lines();
    let Some(header) = lines.next().transpose()? else {
        return Ok(false);
    };
    let meta: Value = serde_json::from_str(&header)?;
    ensure!(
        meta["type"] == "session_meta"
            && meta["payload"]["id"] == thread
            && (imported || matches!(meta["payload"]["source"].as_str(), Some("cli" | "vscode")))
            && Path::new(
                meta["payload"]["cwd"]
                    .as_str()
                    .context("Codex 原生历史缺少目录")?
            )
            .canonicalize()?
                == Path::new(cwd).canonicalize()?,
        "Codex 原生线程来源或目录不匹配"
    );
    for line in lines {
        let line = line?;
        if !line.contains("[CrewVela handoff ") {
            continue;
        }
        let Ok(row) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if row["type"] == "response_item"
            && row["payload"]["type"] == "message"
            && row["payload"]["role"] == "user"
            && row["payload"]["content"].as_array().is_some_and(|parts| {
                parts.len() == 1
                    && parts[0]["type"] == "input_text"
                    && parts[0]["text"].as_str() == Some(expected)
            })
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn indexed_receipt(
    home: &Path,
    sqlite_home: &Path,
    bound: Option<&str>,
    cwd: &str,
    expected: &str,
    started_ms: i64,
    imported: bool,
) -> Result<Option<String>> {
    // Use the native thread index as a locator only. No recent-thread fallback:
    // a new thread must start with this exact, UUID-bound first user message.
    let path = sqlite_home.join("state_5.sqlite");
    if !path.is_file() {
        return Ok(None);
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(Duration::from_millis(100))?;
    let marker = expected.lines().next().context("交接包缺少标识")?;
    let (sql, identity) = match bound {
        Some(id) => ("SELECT id,rollout_path,cwd FROM threads WHERE id=?1 AND archived=0", id),
        None => ("SELECT id,rollout_path,cwd FROM threads WHERE archived=0 AND source IN ('cli','vscode') AND created_at>=?2 AND substr(first_user_message,1,length(?1))=?1", marker),
    };
    let mut statement = db.prepare(sql)?;
    statement.raw_bind_parameter(1, identity)?;
    if bound.is_none() {
        statement.raw_bind_parameter(2, started_ms / 1000 - 2)?;
    }
    let mut rows = statement.raw_query();
    let mut matched = None;
    let cwd_path = Path::new(cwd).canonicalize()?;
    while let Some(row) = rows.next()? {
        let directory: String = row.get(2)?;
        if !Path::new(&directory)
            .canonicalize()
            .is_ok_and(|dir| dir == cwd_path)
        {
            continue;
        }
        let id: String = row.get(0)?;
        let path: String = row.get(1)?;
        if rollout_received(Path::new(&path), home, &id, cwd, expected, imported)? {
            ensure!(
                matched.is_none(),
                "多个 Codex 原生线程含有本次完整交接包，不能确认唯一目标"
            );
            matched = Some(id);
        }
    }
    Ok(matched)
}

fn received(
    home: &Path,
    sqlite_home: &Path,
    bound: Option<&str>,
    cwd: &str,
    expected: &str,
    started_ms: i64,
    imported: bool,
) -> Result<Option<String>> {
    let indexed = indexed_receipt(
        home,
        sqlite_home,
        bound,
        cwd,
        expected,
        started_ms,
        imported,
    );
    // A delayed or unavailable index cannot acknowledge a message. For a
    // precisely bound native thread, also support its ordinary rollout alone.
    if matches!(indexed, Ok(Some(_))) || bound.is_none() {
        return indexed;
    }
    let thread = bound.unwrap();
    let Ok(id) = uuid::Uuid::parse_str(thread) else {
        return indexed;
    };
    if id.get_version_num() != 7 {
        return indexed;
    }
    let millis = id.as_bytes()[..6]
        .iter()
        .fold(0i64, |v, b| (v << 8) | i64::from(*b));
    let Some(date) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis) else {
        return indexed;
    };
    let mut dates = vec![
        date.format("%Y/%m/%d").to_string(),
        date.with_timezone(&chrono::Local)
            .format("%Y/%m/%d")
            .to_string(),
    ];
    dates.dedup();
    let mut matched = false;
    for day in dates {
        let directory = home.join("sessions").join(day);
        if !directory.is_dir() {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(&format!("-{thread}.jsonl"))
                && rollout_received(&entry.path(), home, thread, cwd, expected, imported)?
            {
                ensure!(!matched, "Codex 原生历史文件不唯一");
                matched = true;
            }
        }
    }
    if matched {
        Ok(Some(thread.into()))
    } else {
        indexed
    }
}

impl SessionActionQueue {
    pub(crate) fn enqueue_codex_handoff(
        &self,
        target: &str,
        mut message: QueuedMessage,
        thread: Option<String>,
    ) -> Result<()> {
        uuid::Uuid::parse_str(&message.id)?;
        ensure!(
            message.from.is_none()
                && message
                    .body
                    .starts_with(&format!("[CrewVela handoff {}]\n", message.id)),
            "Codex 交接正文缺少本次交接标识"
        );
        if let Some(thread) = &thread {
            uuid::Uuid::parse_str(thread)?;
        }
        message.codex_handoff = Some(Destination { thread });
        self.enqueue_handoff(target, message)
    }

    pub(super) fn flush_codex_handoffs(&self, sessions: &SessionManager) -> Vec<String> {
        let _delivery = self.delivery_lock.lock().unwrap();
        let items = self
            .items
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(id, queue)| {
                queue
                    .iter()
                    .filter(|m| m.codex_handoff.is_some())
                    .map(|m| (id.clone(), m.clone()))
            })
            .collect::<Vec<_>>();
        let mut dispatched = Vec::new();
        let mut polls = self.codex_handoffs.lock().unwrap();
        for (id, message) in items {
            let now = Instant::now();
            let poll = polls
                .entry(id.clone())
                .or_insert((now, Duration::from_secs(1)));
            if now < poll.0 {
                continue;
            }
            poll.0 = now + poll.1;
            poll.1 = (poll.1 * 2).min(Duration::from_secs(30));
            let Some(session) = sessions.get(&id) else {
                continue;
            };
            if session.archived_at.is_some()
                || session.agent_runtime_kind.as_deref() != Some("codex")
                || session.runtime_command_host
                || session.multi_agent_agent_id.is_some()
            {
                continue;
            }
            match self.check_codex_receipt(&session, &message, sessions) {
                Ok(true) => {
                    if self.commit_delivered_message(&id, &message.id) {
                        self.record_delivery(&id, &message);
                        self.update_delivery_stage(&id, "native_received", None);
                        polls.remove(&id);
                        dispatched.push(id);
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    self.delivery_errors
                        .lock()
                        .unwrap()
                        .insert(id, error.to_string());
                }
            }
        }
        dispatched
    }

    fn check_codex_receipt(
        &self,
        session: &SessionRecord,
        message: &QueuedMessage,
        sessions: &SessionManager,
    ) -> Result<bool> {
        let root = self
            .mailbox_path
            .as_deref()
            .and_then(Path::parent)
            .context("缺少交接投递目录")?;
        let Some(mut saved) = read_attempt(root, &message.id, &session.id, &message.body)? else {
            return Ok(false);
        };
        if saved.phase != Phase::AwaitingReceipt {
            return Ok(saved.phase == Phase::Confirmed);
        }
        let home = session.codex_storage.home()?;
        let sqlite = std::env::var_os("CODEX_SQLITE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.clone());
        let imported = message
            .codex_handoff
            .as_ref()
            .and_then(|d| d.thread.as_deref());
        let native = sessions.runtime_history_locator(session)?;
        ensure!(
            !matches!((imported, native.as_deref()), (Some(a),Some(b)) if a != b),
            "接手会话与导入的 Codex 原生线程不一致"
        );
        let original = prompt(&message.body);
        let expected = saved.reference_prompt.as_deref().unwrap_or(&original);
        let Some(thread) = received(
            &home,
            &sqlite,
            imported.or(native.as_deref()),
            &session.cwd,
            expected,
            saved.started_ms,
            imported.is_some(),
        )?
        else {
            return Ok(false);
        };
        sessions.save_codex_locator(session, &thread)?;
        saved.phase = Phase::Confirmed;
        saved.thread = Some(thread);
        crate::providers::write_atomic(
            &attempt_path(root, &message.id)?,
            &serde_json::to_vec(&saved)?,
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn native_argument_is_literal_and_attempt_survives_stop_without_another_send() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("crewvela-handoff-launch-{}", uuid::Uuid::new_v4()));
        let mailbox = root.join("mailbox.json");
        let queue = SessionActionQueue::open_at(mailbox.clone(), root.join("outbox"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let body = format!("[CrewVela handoff {id}]\n原文 \"quoted\" & %PATH% --full-auto 😺\n");
        let mut message = QueuedMessage::new(body.clone(), None);
        message.id = id.clone();
        queue.enqueue_codex_handoff("target", message.clone(), None)?;
        assert!(
            queue.peek_deliverable("target").is_none(),
            "a native launch prompt cannot go to the PTY input queue"
        );
        let mut command = Command::new("codex.exe");
        let launch = Launch::prepare(&root, &mut command, &id, "target", &body, None)?.unwrap();
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                std::ffi::OsStr::new("--"),
                std::ffi::OsStr::new(body.trim_end())
            ]
        );
        assert!(
            read_attempt(&root, &id, "target", &body)?.is_none(),
            "preparing the command is retryable"
        );
        launch.arm()?;
        assert!(Launch::prepare(
            &root,
            &mut Command::new("codex.exe"),
            &id,
            "target",
            &body,
            None
        )
        .is_err());
        assert!(read_attempt(&root, &id, "another-target", &body).is_err());
        queue.discard_on_stop("target");
        queue.clear();
        drop(queue);
        let queue = SessionActionQueue::open_at(mailbox, root.join("outbox"))?;
        queue.enqueue_codex_handoff("target", message, None)?;
        assert_eq!(queue.items.lock().unwrap()["target"].len(), 1);
        #[cfg(windows)]
        {
            launch.not_started()?;
            assert!(Launch::prepare(
                &root,
                &mut Command::new("codex.exe"),
                &id,
                "target",
                &body,
                None
            )?
            .is_some());
            assert!(Launch::prepare(
                &root,
                &mut Command::new("cmd.exe"),
                &id,
                "target",
                &body,
                None
            )
            .is_err());
        }
        queue.discard("target");
        assert!(queue.peek_next("target").is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn oversized_command_preserves_full_package_and_exposes_the_reference_receipt() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("crewvela-long-handoff-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        let body = format!("[CrewVela handoff {id}]\n{}", "🐍\\\"中文".repeat(30_000));
        let mut cmd = Command::new("codex.exe");
        cmd.args(["-c", "unchanged=true"]);
        let launch = Launch::prepare(
            &root,
            &mut cmd,
            &id,
            "target",
            &body,
            Some("请保留最后的要求"),
        )?
        .unwrap();
        let native = cmd.get_args().last().unwrap().to_str().unwrap();
        assert!(native.starts_with(&format!("[CrewVela handoff {id}]\n")));
        assert!(native.contains("请保留最后的要求"));
        assert_eq!(launch.attempt.reference_prompt.as_deref(), Some(native));
        assert_eq!(fs::read_to_string(launch.path.with_extension("txt"))?, body);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn receipt_requires_the_full_user_turn_on_the_unique_original_thread() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("crewvela-handoff-receipt-{}", uuid::Uuid::new_v4()));
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions)?;
        let cwd = root.display().to_string();
        let expected = format!("[CrewVela handoff {}]\n完整原文 →🐍", uuid::Uuid::new_v4());
        let db = Connection::open(root.join("state_5.sqlite"))?;
        db.execute_batch("CREATE TABLE threads(id TEXT,rollout_path TEXT,cwd TEXT,created_at INTEGER,first_user_message TEXT,source TEXT,archived INTEGER)")?;
        let make = |thread: &str| -> Result<PathBuf> {
            let path = sessions.join(format!("rollout-test-{thread}.jsonl"));
            fs::write(
                &path,
                format!(
                    "{}\n",
                    json!({"type":"session_meta","payload":{"id":thread,"source":"vscode","cwd":root}})
                ),
            )?;
            db.execute(
                "INSERT INTO threads VALUES(?1,?2,?3,1,?4,'vscode',0)",
                params![thread, path.to_str(), cwd, expected],
            )?;
            Ok(path)
        };
        let row = |role: &str, text: &str| json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":"input_text","text":text}]}});
        let first = uuid::Uuid::new_v4().to_string();
        let path = make(&first)?;
        let observe = |id| received(&root, &root, id, &cwd, &expected, 1000, false);
        let mut file = fs::OpenOptions::new().append(true).open(&path)?;
        writeln!(
            file,
            "{}\n{}",
            row("assistant", &expected),
            row("user", &(expected.clone() + "other"))
        )?;
        assert_eq!(observe(None)?, None);
        writeln!(file, "{}", row("user", &expected))?;
        assert_eq!(observe(None)?, Some(first.clone()));
        assert_eq!(
            received(
                &root,
                &root,
                None,
                sessions.to_str().unwrap(),
                &expected,
                1000,
                false
            )?,
            None
        );
        let second = uuid::Uuid::new_v4().to_string();
        writeln!(
            fs::OpenOptions::new().append(true).open(make(&second)?)?,
            "{}",
            row("user", &expected)
        )?;
        assert!(observe(None).is_err());
        assert_eq!(observe(Some(&first))?, Some(first.clone()));
        assert_eq!(observe(Some("unrelated"))?, None);
        drop(file);
        drop(db);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}

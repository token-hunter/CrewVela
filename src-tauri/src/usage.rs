//! 会话 token 用量：增量解析 claude transcript（~/.claude/projects/{slug}/{session}.jsonl）
//! 里 assistant 消息的 message.usage。文件只增不改，按 offset 续读；同一 message.id
//! 去重（transcript 可能对同一条消息重写多行）。内存态——重启后全量重扫重建。

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::sessions::SessionRecord;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

impl SessionUsage {
    fn add(&mut self, other: &SessionUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
    }

    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Default)]
struct TrackedFile {
    path: Option<PathBuf>,
    offset: u64,
    seen_message_ids: HashSet<String>,
    usage: SessionUsage,
}

#[derive(Clone, Default)]
pub struct UsageTracker {
    files: Arc<Mutex<HashMap<String, Arc<Mutex<TrackedFile>>>>>,
}

/// claude 的项目目录 slug：把 cwd 里非字母数字字符全部转成 '-'
/// （"C:\example\crewvela" → "C--example-crewvela"）。
fn project_slug(cwd: &str) -> String {
    cwd.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

/// 推导会话的 transcript 路径；终端会话（无 claude session id）返回 None。
pub fn transcript_path(session: &SessionRecord) -> Option<PathBuf> {
    if session.claude_session_id.is_empty() {
        return None;
    }
    let home = dirs::home_dir()?;
    Some(
        home.join(".claude")
            .join("projects")
            .join(project_slug(&session.cwd))
            .join(format!("{}.jsonl", session.claude_session_id)),
    )
}

/// 从一行 transcript 提取 usage；非 assistant 消息或无 usage 返回 None。
fn parse_usage_line(line: &str) -> Option<(Option<String>, SessionUsage)> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let message = value.get("message")?;
    let usage = message.get("usage")?;
    let message_id = message
        .get("id")
        .and_then(|id| id.as_str())
        .map(str::to_string);
    let number = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    Some((
        message_id,
        SessionUsage {
            input_tokens: number("input_tokens"),
            output_tokens: number("output_tokens"),
            cache_creation_tokens: number("cache_creation_input_tokens"),
            cache_read_tokens: number("cache_read_input_tokens"),
        },
    ))
}

/// 从 tracked.offset 续读文件，累计新出现的 usage。最后一行没写完（进程正在写）
/// 时不消费，留到下轮；文件被截断/重建（长度 < offset）时重置从头扫。
fn consume_increment(path: &std::path::Path, tracked: &mut TrackedFile) {
    if tracked.path.as_deref() != Some(path) {
        *tracked = TrackedFile { path: Some(path.to_path_buf()), ..TrackedFile::default() };
    }
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let length = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    if length < tracked.offset {
        tracked.offset = 0;
        tracked.seen_message_ids.clear();
        tracked.usage = SessionUsage::default();
    }
    if length == tracked.offset {
        return;
    }
    let mut reader = std::io::BufReader::new(file);
    if reader.seek(SeekFrom::Start(tracked.offset)).is_err() {
        return;
    }
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => {
                if !line.ends_with('\n') {
                    break;
                }
                tracked.offset += n as u64;
                if let Some((message_id, usage)) = parse_usage_line(line.trim_end()) {
                    let fresh = match &message_id {
                        Some(id) => tracked.seen_message_ids.insert(id.clone()),
                        None => true,
                    };
                    if fresh {
                        tracked.usage.add(&usage);
                    }
                }
            }
            Err(_) => break,
        }
    }
}

impl UsageTracker {
    /// 读取某个会话 transcript 的增量并返回累计用量；文件不存在/读失败返回当前累计。
    pub fn usage_for(&self, session: &SessionRecord) -> SessionUsage {
        let Some(path) = transcript_path(session) else {
            self.drop_session(&session.id);
            return SessionUsage::default();
        };
        // Disk I/O for one transcript must not hold the registry lock and stall
        // unrelated sessions, including deletion and telemetry refreshes.
        let tracked = self.files.lock().unwrap().entry(session.id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(TrackedFile::default()))).clone();
        let mut tracked = tracked.lock().unwrap();
        consume_increment(&path, &mut tracked);
        tracked.usage.clone()
    }

    /// 批量刷新一批会话的用量（前端低频轮询用）。
    pub fn usage_batch(&self, sessions: &[SessionRecord]) -> HashMap<String, SessionUsage> {
        let mut result = HashMap::new();
        for session in sessions {
            let usage = self.usage_for(session);
            if usage.total() > 0 {
                result.insert(session.id.clone(), usage);
            }
        }
        result
    }

    /// 会话删除后清理跟踪状态（内存态，不删会泄漏）。
    pub fn drop_session(&self, session_id: &str) {
        self.files.lock().unwrap().remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_usage_switching_transcript_resets_cursor_and_totals() {
        let root = std::env::temp_dir().join(format!("crewvela-usage-switch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let first = root.join("first.jsonl");
        let second = root.join("second.jsonl");
        std::fs::write(&first, "{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":10}}}\n").unwrap();
        std::fs::write(&second, "{\"type\":\"assistant\",\"message\":{\"id\":\"m2\",\"usage\":{\"input_tokens\":90}}}\n").unwrap();
        let mut tracked = TrackedFile::default();
        consume_increment(&first, &mut tracked);
        assert_eq!(tracked.usage.input_tokens, 10);
        consume_increment(&second, &mut tracked);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(tracked.usage.input_tokens, 90, "a runtime/session switch must not reuse another transcript's cursor");
    }

    #[test]
    fn slug_matches_claude_projects_dir_shape() {
        assert_eq!(
            project_slug("C:\\example\\crewvela"),
            "C--example-crewvela"
        );
        assert_eq!(project_slug("/home/user/demo"), "-home-user-demo");
    }

    #[test]
    fn parse_usage_line_extracts_assistant_usage() {
        let line = r#"{"type":"assistant","message":{"id":"msg_1","usage":{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":5,"cache_read_input_tokens":3}}}"#;
        let (id, usage) = parse_usage_line(line).unwrap();
        assert_eq!(id.as_deref(), Some("msg_1"));
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 5);
        assert_eq!(usage.cache_read_tokens, 3);

        assert!(parse_usage_line(r#"{"type":"user","message":{"content":"hi"}}"#).is_none());
        assert!(parse_usage_line("not json").is_none());
    }

    #[test]
    fn increment_reads_new_lines_and_dedupes_message_ids() {
        let root = std::env::temp_dir().join(format!("crewvela-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n",
                "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n"
            ),
        )
        .unwrap();

        let mut tracked = TrackedFile {
            path: None,
            offset: 0,
            seen_message_ids: HashSet::new(),
            usage: SessionUsage::default(),
        };
        consume_increment(&path, &mut tracked);
        assert_eq!(tracked.usage.total(), 15);
        // 无新增时再读不变。
        consume_increment(&path, &mut tracked);
        assert_eq!(tracked.usage.total(), 15);

        // 追加：同 id 重复行去重，新行累计。
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", "{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}").unwrap();
        writeln!(file, "{}", "{\"type\":\"assistant\",\"message\":{\"id\":\"m2\",\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}}").unwrap();
        // 模拟进程写了半行：不完整行不应被消费。
        write!(
            file,
            "{}",
            "{\"type\":\"assistant\",\"message\":{\"id\":\"m3\""
        )
        .unwrap();
        drop(file);

        consume_increment(&path, &mut tracked);
        assert_eq!(tracked.usage.input_tokens, 17);
        assert_eq!(tracked.usage.output_tokens, 8);

        // 半行补齐后能被读上。
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            file,
            "{}",
            ",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}"
        )
        .unwrap();
        drop(file);
        consume_increment(&path, &mut tracked);
        assert_eq!(tracked.usage.input_tokens, 18);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn truncated_file_resets_and_rescans() {
        let root =
            std::env::temp_dir().join(format!("crewvela-usage-reset-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("session.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":100,\"output_tokens\":1}}}\n",
        )
        .unwrap();

        let mut tracked = TrackedFile {
            path: None,
            offset: 0,
            seen_message_ids: HashSet::new(),
            usage: SessionUsage::default(),
        };
        consume_increment(&path, &mut tracked);
        assert_eq!(tracked.usage.input_tokens, 100);

        // 文件被重建成更短的新内容：重置并从头扫。
        std::fs::write(
            &path,
            "{\"type\":\"assistant\",\"message\":{\"id\":\"m9\",\"usage\":{\"input_tokens\":2,\"output_tokens\":2}}}\n",
        )
        .unwrap();
        consume_increment(&path, &mut tracked);
        assert_eq!(tracked.usage.input_tokens, 2);
        assert_eq!(tracked.usage.output_tokens, 2);

        let _ = std::fs::remove_dir_all(&root);
    }
}

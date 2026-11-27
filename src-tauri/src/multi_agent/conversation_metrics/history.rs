//! A resumed native session can contain many unrelated user requests. Require
//! the session identity, an exact host-generated prompt header and its time
//! window before attributing any existing transcript usage to a run.
use super::{TaskRun, Tokens};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

// Native logs are optional. Decline incomplete or oversized records instead
// of returning a partial total or delaying other collaboration work.
const MAX_FILE: u64 = 96 * 1024 * 1024;
const MAX_LINE: u64 = 8 * 1024 * 1024;

fn each_row(path: &Path, mut visit: impl FnMut(&Value) -> Option<()>) -> Option<()> {
    let file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size > MAX_FILE {
        return None;
    }
    let mut reader = BufReader::new(file.take(size));
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader
            .by_ref()
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)
            .ok()?;
        if count == 0 {
            return Some(());
        }
        if count as u64 > MAX_LINE {
            return None;
        }
        visit(&serde_json::from_slice(&line).ok()?)?;
    }
}

fn in_window(time: &Value, run: &TaskRun) -> bool {
    let ms = time.as_i64().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(time.as_str()?)
            .ok()
            .map(|t| t.timestamp_millis())
    });
    ms.zip(run.started.zip(run.finished))
        .is_some_and(|(ms, (start, end))| start <= ms && ms <= end)
}

fn prompt_matches(content: &Value, header: &str) -> bool {
    let text = content.as_str().or_else(|| {
        content
            .as_array()?
            .iter()
            .find(|part| matches!(part["type"].as_str(), Some("input_text" | "text")))?["text"]
            .as_str()
    });
    text.and_then(|text| text.lines().next()) == Some(header)
}

fn owned_file(path: PathBuf, root: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    let relative = path.strip_prefix(root).ok()?;
    (canonical == root.canonicalize().ok()?.join(relative)).then_some(canonical)
}

fn codex_path(locator: &Value, id: &str) -> Option<PathBuf> {
    uuid::Uuid::parse_str(id).ok()?;
    let storage = crate::codex_storage::CodexStorage::from_locator(locator).ok()?;
    let home = storage.home().ok()?;
    let sqlite = if storage.is_personal() {
        std::env::var_os("CODEX_SQLITE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.clone())
    } else {
        home.clone()
    };
    let db = Connection::open_with_flags(
        sqlite.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    db.busy_timeout(std::time::Duration::from_millis(100))
        .ok()?;
    let path = PathBuf::from(
        db.query_row::<String, _, _>("SELECT rollout_path FROM threads WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .ok()?,
    );
    if !path
        .file_name()?
        .to_str()?
        .ends_with(&format!("-{id}.jsonl"))
    {
        return None;
    }
    owned_file(path, &home.join("sessions"))
}

fn codex_usage(path: &Path, id: &str, cwd: &str, run: &TaskRun) -> Option<Tokens> {
    let mut session = false;
    let mut counter = None;
    let mut prior = None;
    let mut turn = None;
    let mut matched = false;
    let mut sampled = false;
    let mut result = None;
    let mut saw_turn = false;
    each_row(path, |row| {
        let body = &row["payload"];
        if !session {
            if row["type"] != "session_meta"
                || body["id"] != id
                || body["cwd"].as_str().map(Path::new)?.canonicalize().ok()?
                    != Path::new(cwd).canonicalize().ok()?
            {
                return None;
            }
            session = true;
            // Parent/fork history can carry consumption before the first event.
            if body["forked_from_id"].is_null() && body["forked_from_thread_id"].is_null() {
                counter = Some(Tokens::ZERO);
            }
            return Some(());
        }
        match (row["type"].as_str(), body["type"].as_str()) {
            (Some("event_msg"), Some("task_started")) => {
                if matched {
                    return None;
                }
                if saw_turn && !sampled {
                    counter = None;
                }
                saw_turn = true;
                turn = body["turn_id"].as_str().map(str::to_owned);
                sampled = false;
            }
            (Some("response_item"), Some("message"))
                if body["role"] == "user"
                    && prompt_matches(&body["content"], &run.header)
                    && in_window(&row["timestamp"], run) =>
            {
                if turn.is_none() || matched || result.is_some() {
                    return None;
                }
                // A resumed adapter may publish the previous quota/usage snapshot
                // while opening the thread. Only samples after this user prompt
                // can demonstrate any consumption by this run.
                prior = counter;
                sampled = false;
                matched = true;
            }
            (Some("event_msg"), Some("token_count"))
                if body["info"]["total_token_usage"].is_object() =>
            {
                let next = Tokens::codex(&body["info"]["total_token_usage"]);
                counter = Some(next);
                sampled = true;
            }
            (Some("event_msg"), Some("task_complete" | "task_completed")) if matched => {
                if !sampled || turn.as_deref()? != body["turn_id"].as_str()? {
                    return None;
                }
                let (before, after) = (prior?, counter?);
                if before == after {
                    return None;
                }
                let diff = |a: Option<u64>, b: Option<u64>| {
                    b.zip(a).and_then(|(end, start)| end.checked_sub(start))
                };
                result = Some(Tokens {
                    input: diff(before.input, after.input),
                    cache_read: diff(before.cache_read, after.cache_read),
                    output: diff(before.output, after.output),
                });
                matched = false;
            }
            _ => {}
        }
        Some(())
    })?;
    result
}

fn message_usage(path: &Path, id: &str, run: &TaskRun, pi: bool) -> Option<Tokens> {
    let mut active = false;
    let mut found = false;
    let mut terminal = false;
    let mut pi_header = false;
    let mut calls = HashMap::new();
    each_row(path, |row| {
        let message = &row["message"];
        if pi && !pi_header {
            if row["type"] != "session" || row["id"] != id {
                return None;
            }
            pi_header = true;
            return Some(());
        }
        if pi && row["type"] != "message"
            || !pi && (row["sessionId"] != id || row["isSidechain"] == true)
        {
            return Some(());
        }
        let role = if pi {
            message["role"].as_str()
        } else {
            row["type"].as_str()
        };
        match role {
            Some("user") => {
                let content = &message["content"];
                let is_prompt = content.is_string()
                    || content.as_array().is_some_and(|parts| {
                        parts.iter().any(|p| p["type"] == "text")
                            && parts.iter().all(|p| p["type"] != "tool_result")
                    });
                if is_prompt && row["isMeta"] != true {
                    active =
                        prompt_matches(content, &run.header) && in_window(&row["timestamp"], run);
                    if active && found {
                        return None;
                    }
                    found |= active;
                }
            }
            Some("assistant") if active => {
                if !in_window(&row["timestamp"], run) {
                    return None;
                }
                let key = if pi { &row["id"] } else { &message["id"] };
                let tokens = if pi {
                    Tokens::pi(&message["usage"])
                } else {
                    Tokens::claude(&message["usage"])
                };
                // Claude can write multiple blocks with the same provider message
                // ID. Its later usage supersedes the earlier block.
                calls.insert(key.as_str()?.to_owned(), tokens);
                terminal = if pi {
                    matches!(message["stopReason"].as_str(), Some("stop" | "length"))
                } else {
                    matches!(
                        message["stop_reason"].as_str(),
                        Some("end_turn" | "max_tokens" | "stop_sequence")
                    )
                };
            }
            _ => {}
        }
        Some(())
    })?;
    (found && terminal && !calls.is_empty())
        .then(|| calls.into_values().fold(Tokens::ZERO, Tokens::add))
}

pub(super) fn usage(run: &TaskRun, round: &str) -> Option<Tokens> {
    if run.started.is_none()
        || run.finished.is_none()
        || !run.header.contains(round)
        || !run.header.contains(&run.agent)
    {
        return None;
    }
    let locator = run.locator.as_ref()?;
    let id = locator["threadId"]
        .as_str()
        .or_else(|| locator["sessionId"].as_str())?;
    let cwd = locator["cwd"].as_str()?;
    match run.runtime.as_str() {
        "codex" => codex_usage(&codex_path(locator, id)?, id, cwd, run),
        "claude_code" => {
            let root = std::env::var_os("CLAUDE_CONFIG_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| dirs::home_dir().map(|p| p.join(".claude")))?;
            let path = crate::usage::claude_transcript_path(&root, cwd, id)?;
            message_usage(&owned_file(path, &root)?, id, run, false)
        }
        "pi" => {
            let root = crate::providers::config_dir()
                .ok()?
                .join("multi-agent-sessions");
            let path = owned_file(PathBuf::from(locator["sessionPath"].as_str()?), &root)?;
            message_usage(&path, id, run, true)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run() -> TaskRun {
        TaskRun {
            task_id: "t".into(),
            title: "".into(),
            agent: "member".into(),
            name: "Member".into(),
            phase: Some("chat".into()),
            reply_to: None,
            header: "crew member round-2".into(),
            runtime: "codex".into(),
            run: Some("r".into()),
            status: Some("succeeded".into()),
            started: Some(10),
            finished: Some(20),
            signal: true,
            usage: None,
            locator: None,
        }
    }
    fn file(name: &str, rows: Vec<Value>) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("crewvela-{name}-{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            rows.into_iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        path
    }

    #[test]
    fn existing_codex_rollout_uses_only_the_selected_complete_turn_delta() {
        let cwd = std::env::temp_dir();
        let event = |kind: &str, fields: Value| {
            let mut payload = fields;
            payload["type"] = json!(kind);
            json!({"type":"event_msg","payload":payload,"timestamp":15})
        };
        let start = |id| event("task_started", json!({"turn_id":id}));
        let end = |id| event("task_complete", json!({"turn_id":id}));
        let usage = |input, cache, output| {
            event(
                "token_count",
                json!({"info":{
            "total_token_usage":{"input_tokens":input,"cached_input_tokens":cache,"output_tokens":output},
            "last_token_usage":{"input_tokens":12,"cached_input_tokens":3,"output_tokens":1}}}),
            )
        };
        let prompt = |header| {
            json!({"type":"response_item","timestamp":15,
            "payload":{"type":"message","role":"user","content":[{"type":"input_text","text":header}]}})
        };
        let rows = vec![
            json!({"type":"session_meta","payload":{"id":"session","cwd":cwd}}),
            start("first"),
            prompt("other header"),
            usage(500, 180, 60),
            end("first"),
            start("mine"),
            prompt("crew member round-2\nrest"),
            usage(560, 201, 68),
            usage(560, 201, 68),
            usage(570, 206, 70),
            end("mine"),
            start("later"),
            prompt("yet another round"),
            usage(600, 220, 80),
            end("later"),
        ];
        let path = file("codex-metrics", rows.clone());
        assert_eq!(
            codex_usage(&path, "session", cwd.to_str().unwrap(), &run()),
            Some(Tokens {
                input: Some(70),
                cache_read: Some(26),
                output: Some(10)
            })
        );
        assert_eq!(
            codex_usage(&path, "wrong-session", cwd.to_str().unwrap(), &run()),
            None
        );
        std::fs::remove_file(path).unwrap();
        let mut stale = rows[..5].to_vec();
        stale.extend([
            start("mine"),
            usage(500, 180, 60),
            prompt("crew member round-2\nrest"),
            usage(500, 180, 60),
            end("mine"),
        ]);
        let path = file("stale-metrics", stale);
        assert_eq!(
            codex_usage(&path, "session", cwd.to_str().unwrap(), &run()),
            None,
            "an unchanged session snapshot does not report a zero-cost turn"
        );
        std::fs::remove_file(path).unwrap();
        let path = file("incomplete-metrics", rows.into_iter().take(10).collect());
        assert_eq!(
            codex_usage(&path, "session", cwd.to_str().unwrap(), &run()),
            None
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn existing_claude_transcript_deduplicates_blocks_and_ignores_adjacent_turns() {
        let user = |at, text| json!({"type":"user","sessionId":"s","timestamp":at,"message":{"content":text}});
        let assistant = |id, at, input, cache, out, stop| {
            json!({"type":"assistant","sessionId":"s","timestamp":at,
            "message":{"id":id,"usage":{"input_tokens":input,"cache_read_input_tokens":cache,"cache_creation_input_tokens":0,"output_tokens":out},"stop_reason":stop}})
        };
        let path = file(
            "claude-metrics",
            vec![
                user(1, "earlier"),
                assistant("old", 2, 900, 299, 30, "end_turn"),
                user(11, "crew member round-2\nexact header"),
                assistant("one", 12, 20, 5, 1, "tool_use"),
                assistant("one", 13, 20, 5, 4, "tool_use"),
                json!({"type":"user","sessionId":"s","timestamp":14,"message":{"content":[{"type":"tool_result","content":"stdout"}]}}),
                assistant("two", 15, 35, 10, 5, "end_turn"),
                user(23, "next round"),
                assistant("three", 24, 500, 99, 80, "end_turn"),
            ],
        );
        assert_eq!(
            message_usage(&path, "s", &run(), false),
            Some(Tokens {
                input: Some(70),
                cache_read: Some(15),
                output: Some(9)
            })
        );
        assert_eq!(message_usage(&path, "other", &run(), false), None);
        std::fs::remove_file(path).unwrap();
    }
}

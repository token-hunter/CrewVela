//! Read only exact runtime session metadata. Prompt-derived fallback names are
//! never reported as generated summaries, and no other conversation is read.
use crate::sessions::SummaryRequest;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    io::BufRead,
    path::{Path, PathBuf},
};

fn home(variable: &str, fallback: &str) -> Result<PathBuf> {
    Ok(std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or(
            dirs::home_dir()
                .context("无法定位运行时目录")?
                .join(fallback),
        ))
}

fn exact_jsonl(path: &Path, id: &str, field: &str) -> Result<Option<Value>> {
    let reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut found = None;
    for line in reader.lines() {
        let line = line?;
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            if value[field].as_str() == Some(id) {
                found = Some(value);
            }
        }
    }
    Ok(found)
}

pub(crate) fn read(request: &SummaryRequest) -> Result<Option<String>> {
    let Some(id) = request
        .upstream_locator
        .as_deref()
        .filter(|id| !id.is_empty())
    else {
        return Ok(None);
    };
    let value = match request.runtime_kind.as_deref() {
        Some("codex" | "codex_chat") => {
            let root = home("CODEX_HOME", ".codex")?;
            exact_jsonl(&root.join("session_index.jsonl"), id, "id")?
                .and_then(|value| value["thread_name"].as_str().map(str::to_string))
        }
        Some("kimi_code" | "kimi_chat") => {
            let root = home("KIMI_CODE_HOME", ".kimi-code")?;
            let Some(index) = exact_jsonl(&root.join("session_index.jsonl"), id, "sessionId")?
            else {
                return Ok(None);
            };
            let directory = PathBuf::from(
                index["sessionDir"]
                    .as_str()
                    .context("Kimi 标题索引缺少目录")?,
            )
            .canonicalize()?;
            if !directory.starts_with(root.canonicalize()?) {
                return Ok(None);
            }
            let state: Value =
                serde_json::from_slice(&std::fs::read(directory.join("state.json"))?)?;
            let cwd = state["cwd"]
                .as_str()
                .or_else(|| state["workDir"].as_str())
                .or_else(|| state.pointer("/custom/cwd").and_then(Value::as_str));
            if !cwd.is_some_and(|cwd| same_directory(cwd, &request.cwd))
                || state["titleKind"] != "generated"
            {
                return Ok(None);
            }
            state["title"].as_str().map(str::to_string)
        }
        Some("opencode" | "opencode_chat") => {
            let root = home("XDG_DATA_HOME", ".local/share")?.join("opencode");
            let path = std::env::var_os("OPENCODE_DB")
                .map(PathBuf::from)
                .map(|path| {
                    if path.is_absolute() {
                        path
                    } else {
                        root.join(path)
                    }
                })
                .unwrap_or(root.join("opencode.db"));
            let db = readonly_db(&path)?;
            let (title, cwd): (String, String) = db.query_row(
                "SELECT title,directory FROM session WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if !same_directory(&cwd, &request.cwd)
                || title.starts_with("New session - ")
                || title.starts_with("Child session - ")
            {
                return Ok(None);
            }
            Some(title)
        }
        _ => None,
    };
    Ok(value.filter(|title| !title.trim().is_empty()))
}

fn readonly_db(path: &Path) -> Result<rusqlite::Connection> {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_millis(500))?;
    Ok(db)
}
fn same_directory(a: &str, b: &str) -> bool {
    matches!((Path::new(a).canonicalize(), Path::new(b).canonicalize()), (Ok(a), Ok(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_uses_last_exact_id_and_ignores_other_threads_and_broken_lines() {
        let path = std::env::temp_dir().join(format!("cc-title-index-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "{\"id\":\"target\",\"thread_name\":\"old\"}\ninvalid\n{\"id\":\"other\",\"thread_name\":\"wrong\"}\n{\"id\":\"target\",\"thread_name\":\"new\"}\n").unwrap();
        assert_eq!(
            exact_jsonl(&path, "target", "id").unwrap().unwrap()["thread_name"],
            "new"
        );
        assert!(exact_jsonl(&path, "missing", "id").unwrap().is_none());
        std::fs::remove_file(path).unwrap();
    }
}

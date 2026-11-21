use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, ensure, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sessions::{SessionManager, SessionRecord, SessionStatus};

#[derive(Deserialize, Serialize)]
struct NativeLaunch {
    runtime: String,
    cwd: PathBuf,
    started_at: u64,
    locator: Option<String>,
}

pub(crate) fn configure(
    command: &mut Command,
    record: &SessionRecord,
    sessions: &SessionManager,
) -> Result<bool> {
    // Legacy directories and formal team worktrees retain their own resume policy.
    if !record
        .worktree
        .as_ref()
        .is_some_and(|tree| tree.binding.is_some())
    {
        return Ok(false);
    }
    let Some(runtime @ ("claude_code" | "codex" | "kimi_code" | "opencode" | "zcode_cli")) =
        record.agent_runtime_kind.as_deref()
    else {
        // Pi already binds a per-process native session file in SessionManager.
        return Ok(false);
    };
    let pristine =
        record.status == SessionStatus::Pending && record.created_at == record.updated_at;
    match runtime {
        "claude_code" => {
            let id = if record.claude_session_id.is_empty() {
                &record.id
            } else {
                &record.claude_session_id
            };
            ensure!(uuid::Uuid::parse_str(id).is_ok(), "Claude 原生会话 ID 无效");
            command.args([if pristine { "--session-id" } else { "--resume" }, id]);
        }
        "codex" => {
            if !pristine {
                let id = sessions
                    .runtime_history_locator(record)?
                    .or_else(|| {
                        (!record.claude_session_id.is_empty())
                            .then(|| record.claude_session_id.clone())
                    })
                    .context("尚未捕获 Codex 原生会话 ID，已保留工作树和历史")?;
                let home = record.codex_storage.home()?;
                ensure!(
                    crate::session_switch_history::codex_interactive_identity(
                        record,
                        &id,
                        Some(&home),
                        None
                    )?,
                    "找不到该工作树的 Codex 原生会话，已保留工作树和历史"
                );
                command.arg("resume").arg(id);
            }
        }
        _ => {
            let root = crate::providers::config_dir()?.join("worktree-cli-identities");
            let cwd = Path::new(&record.cwd).canonicalize()?;
            let history = HistoryStore::for_runtime(runtime, &cwd)?;
            let locate = |since| history.candidates(&cwd, since);
            if let Some(id) =
                configure_from_history(&root, &record.id, runtime, &cwd, pristine, locate)?
            {
                command.args([
                    if runtime == "zcode_cli" {
                        "--resume"
                    } else {
                        "--session"
                    },
                    id.as_str(),
                ]);
            }
        }
    }
    Ok(true)
}

fn configure_from_history(
    root: &Path,
    app_id: &str,
    runtime: &str,
    cwd: &Path,
    pristine: bool,
    locate: impl FnOnce(u64) -> Result<Vec<String>>,
) -> Result<Option<String>> {
    ensure!(uuid::Uuid::parse_str(app_id).is_ok(), "工作树会话标识无效");
    let path = root.join(format!("{app_id}.json"));
    let mut launch: NativeLaunch = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("工作树原生会话登记损坏")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && pristine => {
            let started_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
                .try_into()?;
            let launch = NativeLaunch {
                runtime: runtime.into(),
                cwd: cwd.into(),
                started_at,
                locator: None,
            };
            fs::create_dir_all(root)?;
            crate::providers::write_atomic(&path, &serde_json::to_vec(&launch)?)?;
            return Ok(None);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("缺少本工作树的 CLI 启动登记，已保留原会话")
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(
        launch.runtime == runtime && launch.cwd == cwd,
        "工作树与登记的原生会话不匹配"
    );
    let mut candidates = locate(launch.started_at)?;
    candidates.sort();
    candidates.dedup();
    let id = match (&launch.locator, candidates.as_slice()) {
        (Some(id), ids) if ids.contains(id) => return Ok(Some(id.clone())),
        (Some(_), _) => bail!("已登记的原生会话不存在，已保留工作树和会话记录"),
        (None, [id]) => id.clone(),
        (None, []) => bail!("尚未找到该工作树里的原生会话，已保留工作树和会话记录"),
        (None, _) => bail!("该工作树出现多个原生会话，无法确认原会话，已保留工作树和会话记录"),
    };
    ensure!(
        id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
            && id.len() <= 256
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "原生会话 ID 格式无效"
    );
    launch.locator = Some(id.clone());
    crate::providers::write_atomic(&path, &serde_json::to_vec(&launch)?)?;
    Ok(Some(id))
}

enum HistoryStore {
    Kimi(PathBuf),
    OpenCode(PathBuf),
    ZCode(PathBuf),
}

impl HistoryStore {
    fn for_runtime(runtime: &str, cwd: &Path) -> Result<Self> {
        let home = dirs::home_dir().context("缺少 CLI 原生存储目录")?;
        Ok(match runtime {
            "kimi_code" => {
                Self::Kimi(env_path("KIMI_CODE_HOME").unwrap_or_else(|| home.join(".kimi-code")))
            }
            "opencode" => {
                let root = env_path("XDG_DATA_HOME")
                    .unwrap_or_else(|| home.join(".local").join("share"))
                    .join("opencode");
                Self::OpenCode(
                    root.join(env_path("OPENCODE_DB").unwrap_or_else(|| "opencode.db".into())),
                )
            }
            "zcode_cli" => {
                let db = env_path("ZCODE_SESSION_DB_PATH").or_else(|| env_path("ZCODE_SESSION_DB"));
                Self::ZCode(match db {
                    Some(path) => path
                        .strip_prefix("~")
                        .map(|relative| home.join(relative))
                        .unwrap_or_else(|_| cwd.join(path)),
                    None => home.join(".zcode").join("cli").join("db").join("db.sqlite"),
                })
            }
            _ => bail!("CLI 尚未支持工作树原会话查找"),
        })
    }

    fn candidates(&self, cwd: &Path, since: u64) -> Result<Vec<String>> {
        match self {
            Self::Kimi(root) => kimi_candidates(root, cwd, since),
            Self::OpenCode(db) => sqlite_candidates(db, cwd, since,
                "SELECT id,directory FROM session WHERE time_created>=?1 AND parent_id IS NULL LIMIT 10001"),
            Self::ZCode(db) => sqlite_candidates(db, cwd, since,
                "SELECT id,directory FROM session WHERE time_created>=?1 AND parent_id IS NULL AND time_deleted IS NULL LIMIT 10001"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Result<Self> {
            let path =
                std::env::temp_dir().join(format!("cv-native-worktree-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path)?;
            Ok(Self(path.canonicalize()?))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn worktree_runtime_preserves_bound_native_id_and_rejects_ambiguous_or_missing_history(
    ) -> Result<()> {
        let root = TestDirectory::new()?;
        let index = root.0.join("index");
        let app = uuid::Uuid::new_v4().to_string();
        let start = |kind, cwd, fresh, ids: Vec<String>| {
            configure_from_history(&index, &app, kind, cwd, fresh, |_| Ok(ids))
        };
        assert!(start("kimi_code", &root.0, true, vec![])?.is_none());
        assert!(start(
            "kimi_code",
            &root.0,
            false,
            vec!["one".into(), "two".into()]
        )
        .is_err());
        assert!(start("kimi_code", &root.0, false, vec!["-option".into()]).is_err());
        assert_eq!(
            start("kimi_code", &root.0, false, vec!["native_one".into()])?.as_deref(),
            Some("native_one")
        );
        assert_eq!(
            start(
                "kimi_code",
                &root.0,
                false,
                vec!["native_one".into(), "later".into()]
            )?
            .as_deref(),
            Some("native_one")
        );
        assert!(start("kimi_code", &root.0, false, vec!["later".into()]).is_err());
        assert!(start("opencode", &root.0, false, vec!["native_one".into()]).is_err());
        assert!(start("kimi_code", &index, false, vec!["native_one".into()]).is_err());
        Ok(())
    }

    #[test]
    fn worktree_runtime_filters_sqlite_roots_by_real_directory_and_creation_time() -> Result<()> {
        let root = TestDirectory::new()?;
        let other = TestDirectory::new()?;
        let db = root.0.join("history.sqlite");
        let connection = Connection::open(&db)?;
        connection.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,time_created INT,parent_id TEXT,time_deleted INT)")?;
        for row in [
            ("old", &root.0, 9, None, None),
            ("wrong", &other.0, 15, None, None),
            ("child", &root.0, 15, Some("session"), None),
            ("deleted", &root.0, 15, None, Some(16)),
            ("session", &root.0, 15, None, None),
        ] {
            connection.execute(
                "INSERT INTO session VALUES (?1,?2,?3,?4,?5)",
                rusqlite::params![row.0, row.1.to_str().unwrap(), row.2, row.3, row.4],
            )?;
        }
        drop(connection);
        assert_eq!(
            HistoryStore::ZCode(db.clone()).candidates(&root.0, 10)?,
            ["session"]
        );
        assert_eq!(
            HistoryStore::OpenCode(db).candidates(&other.0, 10)?,
            ["wrong"]
        );
        Ok(())
    }

    #[test]
    fn worktree_runtime_checks_kimi_state_against_the_session_index() -> Result<()> {
        let root = TestDirectory::new()?;
        let repo = TestDirectory::new()?;
        let outside = TestDirectory::new()?;
        let mut entries = Vec::new();
        for (id, directory, created, archived) in [
            ("current", root.0.join("current"), 50, false),
            ("old", root.0.join("old"), 10, false),
            ("archived", root.0.join("archived"), 50, true),
            ("outside", outside.0.clone(), 50, false),
        ] {
            fs::create_dir_all(&directory)?;
            let date = chrono::DateTime::from_timestamp_millis(created)
                .unwrap()
                .to_rfc3339();
            fs::write(
                directory.join("state.json"),
                serde_json::json!({"workDir":repo.0,"createdAt":date,"archived":archived})
                    .to_string(),
            )?;
            entries.push(serde_json::json!({"sessionId":id,"sessionDir":directory}).to_string());
        }
        fs::write(root.0.join("session_index.jsonl"), entries.join("\n"))?;
        assert_eq!(
            HistoryStore::Kimi(root.0.clone()).candidates(&repo.0, 25)?,
            ["current"]
        );
        Ok(())
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn matches_directory(path: &str, cwd: &Path) -> bool {
    let path = Path::new(path);
    path.is_absolute() && path.canonicalize().is_ok_and(|directory| directory == cwd)
}

fn sqlite_candidates(db: &Path, cwd: &Path, since: u64, sql: &str) -> Result<Vec<String>> {
    let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("读取 CLI 原生会话索引失败")?;
    let mut query = connection.prepare(sql)?;
    let rows = query.query_map([i64::try_from(since)?], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut matches = Vec::new();
    for (index, row) in rows.enumerate() {
        ensure!(index < 10000, "CLI 原生会话索引超出核对范围");
        let (id, path) = row?;
        if matches_directory(&path, cwd) {
            matches.push(id);
        }
    }
    Ok(matches)
}

fn kimi_candidates(root: &Path, cwd: &Path, since: u64) -> Result<Vec<String>> {
    let root = root.canonicalize()?;
    let index = root.join("session_index.jsonl");
    ensure!(
        index.canonicalize()? == index && fs::metadata(&index)?.len() <= 16 * 1024 * 1024,
        "Kimi 原生会话索引已重定向或超出核对范围"
    );
    let reader = BufReader::new(fs::File::open(index)?);
    let mut matches = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        ensure!(index < 10000, "Kimi 原生会话索引超出核对范围");
        let entry: Value = serde_json::from_str(&line?)?;
        let (Some(id), Some(path)) = (entry["sessionId"].as_str(), entry["sessionDir"].as_str())
        else {
            continue;
        };
        let Ok(directory) = Path::new(path).canonicalize() else {
            continue;
        };
        if !directory.starts_with(&root) {
            continue;
        }
        let metadata = directory.join("state.json");
        if !metadata.canonicalize().is_ok_and(|path| path == metadata) {
            continue;
        }
        let Ok(bytes) = fs::read(metadata) else {
            continue;
        };
        let state: Value = serde_json::from_slice(&bytes)?;
        let workspace = state["cwd"]
            .as_str()
            .or_else(|| state["workDir"].as_str())
            .or_else(|| state.pointer("/custom/cwd").and_then(Value::as_str))
            .or_else(|| entry["workDir"].as_str());
        let created_at = &state["createdAt"];
        let created_at = created_at.as_u64().or_else(|| {
            chrono::DateTime::parse_from_rfc3339(created_at.as_str()?)
                .ok()?
                .timestamp_millis()
                .try_into()
                .ok()
        });
        if created_at.is_some_and(|created| created >= since)
            && state["archived"].as_bool() != Some(true)
            && state["archivedAt"].is_null()
            && state.pointer("/custom/parent_session_id").is_none()
            && workspace.is_some_and(|path| matches_directory(path, cwd))
        {
            matches.push(id.to_owned());
        }
    }
    Ok(matches)
}

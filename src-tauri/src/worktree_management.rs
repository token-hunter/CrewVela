use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::multi_agent::MultiAgentStore;
use crate::sessions::{NewRuntimeSession, SessionManager, SessionRecord, SessionStatus};
use crate::worktree::{self, SessionWorktree, WorktreeOptions};
use crate::{worktree_lifecycle as lifecycle, PtyManager};

pub(crate) fn process_owner() -> &'static str {
    static INSTANCE: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| uuid::Uuid::new_v4().to_string());
    &INSTANCE
}

pub(crate) fn verify_process_owner(record: &SessionRecord) -> Result<()> {
    ensure!(
        record
            .worktree_runtime_owner
            .as_deref()
            .is_none_or(|id| id == process_owner()),
        "上次运行未留下确认停止的记录，已保护原工作树"
    );
    Ok(())
}

pub(crate) fn release_process_owner(record: &mut SessionRecord) -> bool {
    if record.worktree_runtime_owner.as_deref() == Some(process_owner()) {
        record.worktree_runtime_owner = None;
        true
    } else {
        false
    }
}

pub(crate) fn create_session(
    sessions: &SessionManager,
    source: &str,
    options: Option<&WorktreeOptions>,
    persist: impl FnOnce(String, String, Option<SessionWorktree>) -> Result<SessionRecord>,
    register: impl FnOnce(&SessionRecord) -> Result<()>,
) -> Result<SessionRecord, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let lock = sessions.lifecycle_lock(&id);
    let _guard = lock.lock().unwrap();
    let tree = options
        .map(|options| worktree::create_session_worktree(Path::new(source), &id, options))
        .transpose()
        .map_err(crate::to_err)?;
    let actual = tree
        .as_ref()
        .map(|tree| tree.cwd().display().to_string())
        .unwrap_or_else(|| source.into());
    let record = persist(id.clone(), actual, tree.clone()).map_err(|error| {
        if let Some(tree) = &tree {
            if let Err(rollback) = worktree::remove_checkout(tree, &id) {
                return format!(
                    "登记会话失败：{error}；工作树保留于 {}：{rollback}",
                    tree.path
                );
            }
        }
        error.to_string()
    })?;
    if let Err(error) = register(&record) {
        if let Some(tree) = &tree {
            if let Err(rollback) = worktree::remove_checkout(tree, &id) {
                let message = format!("登记原生会话失败：{error}；工作树已保留：{rollback}");
                sessions
                    .mark_error(&id, message.clone())
                    .map_err(crate::to_err)?;
                return Err(message);
            }
        }
        sessions
            .delete(&id)
            .map_err(|rollback| format!("登记原生会话失败：{error}；撤销会话失败：{rollback}"))?;
        return Err(error.to_string());
    }
    if let Some(tree) = &tree {
        lifecycle::prepare_if_configured(tree, &id).map_err(|error| {
            format!("工作树和会话已保留，可在工作树设置中重试环境准备：{error}")
        })?;
    }
    Ok(record)
}

pub(crate) fn create_chat(
    sessions: &SessionManager,
    runtime: &str,
    cwd: String,
    project_id: Option<String>,
    options: Option<WorktreeOptions>,
    source_id: Option<String>,
    register: impl FnOnce(&SessionRecord) -> Result<()>,
) -> Result<SessionRecord, String> {
    let cwd = crate::acp::canonical_harness_directory(&cwd).map_err(crate::to_err)?;
    let source = source_id
        .as_deref()
        .map(|id| sessions.get(id).ok_or("工作树来源会话不存在"))
        .transpose()?;
    let owner_id = source
        .as_ref()
        .map(|source| {
            source
                .worktree
                .as_ref()
                .map(|_| source.id.clone())
                .or_else(|| source.worktree_source_session_id.clone())
                .ok_or("来源会话没有托管工作树")
        })
        .transpose()?;
    let lock = owner_id.as_ref().map(|id| sessions.lifecycle_lock(id));
    let _guard = lock.as_ref().map(|lock| lock.lock().unwrap());
    let cwd = if let (Some(source), Some(owner_id)) = (&source, &owner_id) {
        if options.is_some() {
            return Err("不能同时继承工作树并创建新工作树".into());
        }
        let current = sessions.get(&source.id).ok_or("工作树来源会话已删除")?;
        let owner = sessions.get(owner_id).ok_or("工作树所有者已删除")?;
        verify_start(&current, sessions).map_err(crate::to_err)?;
        if current.archived_at.is_some()
            || current.project_id != project_id
            || owner.project_id != project_id
            || current.multi_agent_project_id.is_some()
            || current.multi_agent_agent_id.is_some()
            || Path::new(&current.cwd)
                .canonicalize()
                .map_err(crate::to_err)?
                != Path::new(&cwd).canonicalize().map_err(crate::to_err)?
        {
            return Err("工作树来源会话、项目或实际工作目录不匹配".into());
        }
        cwd
    } else {
        crate::runtime_project_cwd(Some(cwd), project_id.as_deref(), |id| {
            crate::projects::get(id).map_err(crate::to_err)
        })?
    };
    create_session(
        sessions,
        &cwd,
        options.as_ref(),
        |id, actual, tree| {
            sessions.create_runtime_in(
                NewRuntimeSession {
                    runtime_kind: runtime.into(),
                    cwd: actual,
                    project_id,
                },
                id,
                None,
                tree,
                owner_id.clone(),
            )
        },
        register,
    )
}

pub(crate) fn verify_start(record: &SessionRecord, sessions: &SessionManager) -> Result<()> {
    verify_process_owner(record)?;
    if let Some(owner_id) = &record.worktree_source_session_id {
        let owner = sessions.get(owner_id).context("此工作树的所有者已删除")?;
        let tree = owner.worktree.as_ref().context("来源工作树已解绑")?;
        worktree::verify_session_cwd(&owner)?;
        ensure!(
            record.project_id == owner.project_id
                && Path::new(&record.cwd).canonicalize()? == tree.cwd().canonicalize()?,
            "借用的工作目录或所属项目与登记不一致"
        );
        lifecycle::ensure_preparation_ready(tree, owner_id)?;
    } else if let Some(tree) = &record.worktree {
        worktree::verify_session_cwd(record)?;
        lifecycle::ensure_preparation_ready(tree, &record.id)?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktreeEntry {
    pub session_id: Option<String>,
    pub title: String,
    pub project: Option<String>,
    pub repo_root: String,
    pub path: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub base_ref: Option<String>,
    pub runtime: Option<String>,
    pub archived: bool,
    pub present: bool,
    pub blocked: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorktreeOverview {
    root: PathBuf,
    entries: Vec<WorktreeEntry>,
}

#[tauri::command(async)]
pub(crate) fn worktree_branches(cwd: String) -> Result<Vec<String>, String> {
    worktree::branches(Path::new(&cwd)).map_err(crate::to_err)
}

#[tauri::command(async)]
pub(crate) fn worktree_set_root(root: Option<String>) -> Result<PathBuf, String> {
    worktree::set_root(root.as_deref().map(Path::new)).map_err(crate::to_err)
}

fn inside(path: &str, checkout: &Path) -> bool {
    let path = Path::new(path);
    path.starts_with(checkout)
        || path
            .canonicalize()
            .ok()
            .zip(checkout.canonicalize().ok())
            .is_some_and(|(path, checkout)| path.starts_with(checkout))
}

pub(crate) fn verify_new_references(records: &[SessionRecord], new: &SessionRecord) -> Result<()> {
    // This check executes under the same registry lock as checkout removal. A caller that
    // selected a directory before cleanup must not publish a session pointing at a removed tree.
    for owner in records.iter().filter(|record| record.worktree.is_some()) {
        let checkout = Path::new(&owner.worktree.as_ref().unwrap().path);
        for reference in std::iter::once(new.cwd.as_str())
            .chain(new.worktree.as_ref().map(|tree| tree.repo_root.as_str()))
        {
            if Path::new(reference).starts_with(checkout) {
                ensure!(Path::new(reference).is_dir(), "工作树已清理，请先恢复目录");
            }
        }
    }
    Ok(())
}

pub(crate) fn unshared(
    records: &[SessionRecord],
    owner: &str,
    tree: &SessionWorktree,
) -> Result<()> {
    let path = Path::new(&tree.path);
    if let Some(other) = records.iter().find(|record| {
        record.id != owner
            && (record.worktree_source_session_id.as_deref() == Some(owner)
                || inside(&record.cwd, path)
                || record.worktree.as_ref().is_some_and(|other| {
                    inside(&other.path, path) || inside(&other.repo_root, path)
                }))
    }) {
        anyhow::bail!("「{}」仍引用此工作树，请先处理该会话及其历史", other.title);
    }
    Ok(())
}

fn project_paths(store: Option<&MultiAgentStore>) -> Result<Vec<String>> {
    let mut paths: Vec<_> = crate::projects::list()?
        .into_iter()
        .map(|project| project.path)
        .collect();
    if let Some(store) = store {
        paths.extend(store.referenced_workspaces()?);
    }
    Ok(paths)
}

fn guard_projects(tree: &SessionWorktree, paths: &[String]) -> Result<()> {
    ensure!(
        !paths
            .iter()
            .any(|location| inside(location, Path::new(&tree.path))),
        "该工作树仍被项目、协作任务或交付历史引用"
    );
    Ok(())
}

fn guard_runtime<R: tauri::Runtime>(app: &AppHandle<R>, record: &SessionRecord) -> Result<()> {
    verify_process_owner(record)?;
    ensure!(
        record.multi_agent_project_id.is_none() && record.multi_agent_agent_id.is_none(),
        "协作任务工作树只读"
    );
    ensure!(
        record.pty_id.is_none()
            && !matches!(
                record.status,
                SessionStatus::Waiting | SessionStatus::Running
            ),
        "请先停止会话"
    );
    let ptys = app.state::<PtyManager>();
    ensure!(!ptys.has_tracking(&record.id), "终端进程尚未确认停止");
    let live = match record.agent_runtime_kind.as_deref() {
        Some("deepseek_harness") => app
            .state::<crate::NativeAcpSessions>()
            .has_worktree_runtime(&record.id)?,
        Some("claude_chat") => app
            .state::<crate::NativeClaudeSessions>()
            .0
            .has_worktree_runtime(&record.id)?,
        Some("kimi_chat") => app
            .state::<crate::NativeKimiSessions>()
            .0
            .has_worktree_runtime(&record.id)?,
        Some("codex_chat") => app
            .state::<crate::NativeCodexChatSessions>()
            .0
            .has_worktree_runtime(&record.id)?,
        Some("opencode_chat") => app
            .state::<crate::NativeOpenCodeChatSessions>()
            .0
            .has_worktree_runtime(&record.id)?,
        Some("pi_chat") => app
            .state::<crate::NativePiSessions>()
            .has_worktree_runtime(&record.id)?,
        Some("zcode") => app
            .state::<crate::NativeZCodeSessions>()
            .has_worktree_runtime(&record.id)?,
        _ => false,
    };
    ensure!(!live, "原生进程尚未确认停止");
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn worktree_overview(
    app: AppHandle,
    session_state: State<'_, SessionManager>,
    multi_agent_store: State<'_, MultiAgentStore>,
) -> Result<WorktreeOverview, String> {
    let mut records = session_state.list();
    records.extend(session_state.list_archived());
    let projects = crate::projects::list().map_err(crate::to_err)?;
    let protected_paths = project_paths(Some(&multi_agent_store)).map_err(crate::to_err)?;
    let mut entries = Vec::new();
    for record in &records {
        let Some(tree) = &record.worktree else {
            continue;
        };
        let present = Path::new(&tree.path).is_dir();
        let blocked = (|| -> Result<()> {
            ensure!(present, "目录未检出");
            guard_runtime(&app, record)?;
            crate::session_switch::protect_delete(&record.id, &session_state)?;
            unshared(&records, &record.id, tree)?;
            guard_projects(tree, &protected_paths)?;
            worktree::ensure_removable(tree, &record.id)
        })()
        .err()
        .map(|error| error.to_string());
        entries.push(WorktreeEntry {
            session_id: Some(record.id.clone()),
            title: record.title.clone(),
            project: record.project_id.as_ref().and_then(|id| {
                projects
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| p.name.clone())
            }),
            repo_root: tree.repo_root.clone(),
            path: tree.path.clone(),
            cwd: record.cwd.clone(),
            branch: Some(tree.branch.clone()),
            base_ref: tree
                .binding
                .as_ref()
                .map(|binding| binding.base_ref.clone()),
            runtime: record
                .agent_runtime_kind
                .clone()
                .or_else(|| Some("claude_code".into())),
            archived: record.archived_at.is_some(),
            present,
            blocked,
        });
    }
    let formal_root = crate::providers::config_dir()
        .map_err(crate::to_err)?
        .join("worktrees");
    for (project, title, repo, project_id, task_id, run_id, cwd) in multi_agent_store
        .task_worktree_locations()
        .map_err(crate::to_err)?
    {
        let path = formal_root.join(&project_id).join(&task_id).join(&run_id);
        if !path.join(".git").is_file() && !cwd.as_ref().is_some_and(|cwd| inside(cwd, &path)) {
            continue;
        }
        let present = path.is_dir();
        entries.push(WorktreeEntry {
            session_id: None,
            title,
            project: Some(project),
            repo_root: repo,
            path: path.display().to_string(),
            cwd: cwd
                .filter(|cwd| inside(cwd, &path))
                .unwrap_or_else(|| path.display().to_string()),
            branch: None,
            base_ref: None,
            runtime: None,
            archived: false,
            present,
            blocked: Some("协作任务保留此工作树及交付历史".into()),
        });
    }
    Ok(WorktreeOverview {
        root: worktree::active_root().map_err(crate::to_err)?,
        entries,
    })
}

/// Caller owns this session's lifecycle lock. Holds the session registry through Git removal,
/// so a concurrent create or handoff cannot borrow the directory between the check and remove.
pub(crate) fn clean_locked(
    sessions: &SessionManager,
    record: &SessionRecord,
    store: Option<&MultiAgentStore>,
) -> Result<()> {
    verify_process_owner(record)?;
    let tree = record.worktree.as_ref().context("该会话没有独立工作树")?;
    crate::session_switch::protect_delete(&record.id, sessions)?;
    guard_projects(tree, &project_paths(store)?)?;
    sessions.with_worktree_records(|records| unshared(records, &record.id, tree))?;
    worktree::ensure_removable(tree, &record.id)?;
    let snapshot = lifecycle::snapshot_for_cleanup(tree, &record.id)?;
    lifecycle::verify_cleanup_snapshot(&snapshot)?;
    guard_projects(tree, &project_paths(store)?)?;
    sessions.with_worktree_records(|records| {
        unshared(records, &record.id, tree)?;
        worktree::remove_at_commit(tree, &record.id, Some(&snapshot.commit))
    })
}

pub(crate) fn after_archive<R: tauri::Runtime>(
    app: &AppHandle<R>,
    sessions: &SessionManager,
    record: &SessionRecord,
) -> Result<(), String> {
    let Some(tree) = record
        .worktree
        .as_ref()
        .filter(|tree| tree.binding.is_some())
    else {
        return Ok(());
    };
    let attempt = || -> Result<()> {
        if !lifecycle::auto_cleanup_enabled(Path::new(&tree.repo_root))?
            || !Path::new(&tree.path).try_exists()?
        {
            return Ok(());
        }
        guard_runtime(app, record)?;
        let store = app.try_state::<MultiAgentStore>();
        clean_locked(sessions, record, store.as_deref())
    };
    attempt().map_err(|error| format!("会话已归档，自动清理保留了工作树：{error}"))
}

#[tauri::command(async)]
pub(crate) fn worktree_clean(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
    multi_agent_store: State<'_, MultiAgentStore>,
) -> Result<(), String> {
    let lock = session_state.lifecycle_lock(&session_id);
    let _guard = lock.lock().unwrap();
    let record = session_state.get(&session_id).ok_or("会话已删除")?;
    guard_runtime(&app, &record).map_err(crate::to_err)?;
    clean_locked(&session_state, &record, Some(&multi_agent_store)).map_err(crate::to_err)
}

#[tauri::command(async)]
pub(crate) fn worktree_lifecycle_prepare(
    app: AppHandle,
    session_id: String,
    session_state: State<'_, SessionManager>,
) -> Result<(), String> {
    let lock = session_state.lifecycle_lock(&session_id);
    let _guard = lock.lock().unwrap();
    let record = session_state.get(&session_id).ok_or("会话已删除")?;
    guard_runtime(&app, &record).map_err(crate::to_err)?;
    crate::session_switch::protect_delete(&session_id, &session_state).map_err(crate::to_err)?;
    let tree = record.worktree.as_ref().ok_or("会话没有独立工作树")?;
    let mut records = session_state.list();
    records.extend(session_state.list_archived());
    unshared(&records, &session_id, tree).map_err(crate::to_err)?;
    worktree::verify_session_cwd(&record).map_err(crate::to_err)?;
    lifecycle::prepare_if_configured(tree, &session_id).map_err(crate::to_err)
}

#[tauri::command(async)]
pub(crate) fn worktree_lifecycle_restore(
    app: AppHandle,
    repo_root: String,
    snapshot_id: String,
    session_state: State<'_, SessionManager>,
) -> Result<(), String> {
    let snapshot = lifecycle::load_recovery_snapshot(Path::new(&repo_root), &snapshot_id)
        .map_err(crate::to_err)?;
    let lock = session_state.lifecycle_lock(&snapshot.session_id);
    let _guard = lock.lock().unwrap();
    let tree = &snapshot.worktree;
    if let Some(record) = session_state.get(&snapshot.session_id) {
        guard_runtime(&app, &record).map_err(crate::to_err)?;
        if record.worktree.as_ref() != Some(tree) {
            return Err("快照与原会话登记不一致".into());
        }
    }
    worktree::validate_location(tree, &snapshot.session_id).map_err(crate::to_err)?;
    lifecycle::restore_snapshot_checkout(&snapshot).map_err(crate::to_err)?;
    if worktree::verify_managed_worktree(tree, &snapshot.session_id).is_ok() {
        return Ok(());
    }
    worktree::bind_restored_checkout(tree, &snapshot.session_id).map_err(crate::to_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_acp_sessions::{NativeAcpSessions, NativeChatFlavor};
    use std::{
        fs,
        process::Command,
        time::{Duration, Instant},
    };

    fn git(path: &Path, args: &[&str]) {
        let result = Command::new("git")
            .arg("-C")
            .arg(path)
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@localhost",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn native_id(file: &Path, record: &SessionRecord, field: &str) -> String {
        let store: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        let locator = &store["sessions"][&record.id];
        assert_eq!(
            Path::new(locator["cwd"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            Path::new(&record.cwd).canonicalize().unwrap()
        );
        locator[field]
            .as_str()
            .filter(|v| !v.is_empty())
            .unwrap()
            .to_owned()
    }

    fn peer(root: &Path, script: &str) -> PathBuf {
        let wrapper = root.join(format!("peer-{script}"));
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures")
            .join(script)
            .canonicalize()
            .unwrap();
        // Each peer reports the OS process cwd before it performs its normal protocol handshake.
        let target =
            serde_json::to_string(&crate::multi_agent::external_command_path(&file)).unwrap();
        let script = r#"
import fs from "node:fs";
import { pathToFileURL } from "node:url";
const receipt = process.env.CREWVELA_WORKTREE_CWD_RECEIPTS;
fs.appendFileSync(receipt, JSON.stringify(process.cwd()) + "\n");
const emit = process.stdin.emit;
let pending = "";
process.stdin.emit = function(event, ...args) {
    if (event === "data") {
        const lines = (pending + args[0].toString()).split("\n");
        pending = lines.pop();
        for (const line of lines) {
            if (!line.trim()) continue;
            const frame = JSON.parse(line);
            if (frame.method === "session/load" || frame.method === "session/resume") {
                fs.appendFileSync(receipt + ".protocol", JSON.stringify(frame.params) + "\n");
            }
        }
    }
    return emit.call(this, event, ...args);
};
await import(pathToFileURL(FIXTURE_PATH).href);
"#;
        fs::write(&wrapper, script.replace("FIXTURE_PATH", &target)).unwrap();
        wrapper
    }

    fn isolated_roundtrip(root: &Path) {
        crate::test_profile::initialize(&root.join("profile")).unwrap();
        let repo = root.join("repo");
        let project_cwd = repo.join("项目 子目录");
        fs::create_dir_all(&project_cwd).unwrap();
        git(&repo, &["init"]);
        fs::write(project_cwd.join("fixture.txt"), "original\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "fixture"]);
        let project =
            crate::projects::create(Some("Test project"), project_cwd.to_str().unwrap()).unwrap();
        let sessions = SessionManager::new().unwrap();
        let app = tauri::test::mock_app();
        let make = |kind: &str, register: &dyn Fn(&SessionRecord) -> Result<()>| {
            let runtime = serde_json::from_value(serde_json::json!(kind)).unwrap();
            crate::runtime_preferences::set_enabled(runtime, true, || Ok(false)).unwrap();
            create_chat(
                &sessions,
                kind,
                project.path.clone(),
                Some(project.id.clone()),
                Some(WorktreeOptions::default()),
                None,
                register,
            )
            .unwrap()
        };
        let receipts = root.join("cwd.jsonl");
        std::env::set_var("CREWVELA_WORKTREE_CWD_RECEIPTS", &receipts);
        let check_restart_cwd = |record: &SessionRecord| {
            let lines = fs::read_to_string(&receipts).unwrap();
            let paths: Vec<PathBuf> = lines
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let expected = Path::new(&record.cwd).canonicalize().unwrap();
            assert_eq!(
                expected,
                record
                    .worktree
                    .as_ref()
                    .unwrap()
                    .cwd()
                    .canonicalize()
                    .unwrap()
            );
            assert_ne!(expected, project_cwd.canonicalize().unwrap());
            assert_eq!(
                paths.len(),
                2,
                "{} must start two local peers",
                record.title
            );
            assert!(
                paths
                    .iter()
                    .all(|path| path.canonicalize().unwrap() == expected),
                "{}: {paths:?} != {expected:?}",
                record.title
            );
            fs::remove_file(&receipts).unwrap();
        };
        for (kind, flavor, fixture) in [
            (
                "deepseek_harness",
                NativeChatFlavor::DeepSeekHarness,
                "native-harness-acp.mjs",
            ),
            (
                "claude_chat",
                NativeChatFlavor::ClaudeChat,
                "native-claude-acp.mjs",
            ),
            (
                "codex_chat",
                NativeChatFlavor::CodexChat,
                "codex-acp-agent.mjs",
            ),
            (
                "kimi_chat",
                NativeChatFlavor::KimiChat,
                "kimi-acp-agent.mjs",
            ),
            (
                "opencode_chat",
                NativeChatFlavor::OpenCodeChat,
                "opencode-acp-agent.mjs",
            ),
        ] {
            let store = root.join(format!("{kind}.json"));
            let make_manager = || {
                let mut manager = NativeAcpSessions::open_at_flavor(store.clone(), flavor).unwrap();
                manager.test_peer = Some(peer(root, fixture));
                manager
            };
            let manager = make_manager();
            let record = make(kind, &|r| manager.register(&r.id, &r.cwd));
            assert_eq!(record.project_id.as_ref(), Some(&project.id));
            manager
                .start(app.handle().clone(), &sessions, &record)
                .unwrap();
            let original = native_id(&store, &record, "acpSessionId");
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            drop(manager);
            let reopened = SessionManager::new().unwrap();
            let record = reopened.get(&record.id).unwrap();
            let manager = make_manager();
            manager
                .start(app.handle().clone(), &reopened, &record)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(
                fs::read_to_string(root.join("cwd.jsonl.protocol"))
                    .unwrap_or_else(|error| panic!("{kind} protocol receipt: {error}"))
                    .trim(),
            )
            .unwrap();
            assert_eq!(
                request["sessionId"], original,
                "{kind} resumed a different session"
            );
            fs::remove_file(root.join("cwd.jsonl.protocol")).unwrap();
            let resumed = native_id(&store, &record, "acpSessionId");
            if flavor == NativeChatFlavor::ClaudeChat {
                // Claude confirms load of the old ID and returns its continuation's new upstream ID.
                assert_eq!(
                    (original.as_str(), resumed.as_str()),
                    ("claude-fake-1", "claude-fake-2")
                );
            } else {
                assert_eq!(resumed, original);
            }
            manager.stop(&record.id, &reopened, app.handle()).unwrap();
            check_restart_cwd(&record);
        }
        {
            let store = root.join("pi.json");
            let make_manager = || {
                let mut manager = crate::NativePiSessions::open_at(store.clone()).unwrap();
                manager.test_peer = Some(peer(root, "pi-chat-agent.mjs"));
                manager
            };
            let manager = make_manager();
            let record = make("pi_chat", &|r| manager.register(&r.id, &r.cwd));
            manager
                .start(app.handle().clone(), &sessions, &record)
                .unwrap();
            manager
                .send_message_with_attachments(
                    app.handle().clone(),
                    &sessions,
                    &record.id,
                    "fixture",
                    &[],
                )
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            while manager.snapshot(&record.id).unwrap().busy && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(30));
            }
            let original = native_id(&store, &record, "piSessionFile");
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            drop(manager);
            let manager = make_manager();
            let reopened = SessionManager::new().unwrap();
            let record = reopened.get(&record.id).unwrap();
            manager
                .start(app.handle().clone(), &reopened, &record)
                .unwrap();
            assert_eq!(native_id(&store, &record, "piSessionFile"), original);
            let restored: serde_json::Value = serde_json::from_slice(
                &fs::read(Path::new(&record.cwd).join("pi-resumed-marker.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(restored["session"], original);
            manager.stop(&record.id, &reopened, app.handle()).unwrap();
            check_restart_cwd(&record);
        }
        {
            let store = root.join("zcode.json");
            let make_manager = || {
                let mut manager = crate::NativeZCodeSessions::open_at(store.clone()).unwrap();
                manager.test_peer = Some((peer(root, "native-zcode-agent.mjs"), "default".into()));
                manager
            };
            let manager = make_manager();
            let record = make("zcode", &|r| manager.register(&r.id, &r.cwd));
            manager
                .start(app.handle().clone(), &sessions, &record)
                .unwrap();
            let original = native_id(&store, &record, "nativeSessionId");
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            drop(manager);
            let manager = make_manager();
            let reopened = SessionManager::new().unwrap();
            let record = reopened.get(&record.id).unwrap();
            manager
                .start(app.handle().clone(), &reopened, &record)
                .unwrap();
            assert_eq!(native_id(&store, &record, "nativeSessionId"), original);
            manager.stop(&record.id, &reopened, app.handle()).unwrap();
            check_restart_cwd(&record);
        }

        let custom = root.join("custom managed 目录");
        fs::create_dir(&custom).unwrap();
        worktree::set_root(Some(&custom)).unwrap();
        let owner = make("codex_chat", &|_| Ok(()));
        let tree = owner.worktree.as_ref().unwrap();
        assert_eq!(
            Path::new(&tree.path)
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap(),
            custom.canonicalize().unwrap()
        );
        worktree::set_root(None).unwrap();
        verify_start(&owner, &sessions).unwrap();
        git(Path::new(&tree.path), &["switch", "-c", "user-branch"]);
        verify_start(&owner, &sessions).unwrap();
        assert!(worktree::ensure_removable(tree, &owner.id).is_err());
        git(Path::new(&tree.path), &["switch", "--detach"]);
        verify_start(&owner, &sessions).unwrap();
        git(Path::new(&tree.path), &["switch", &tree.branch]);
        assert!(create_chat(
            &sessions,
            "kimi_chat",
            repo.display().to_string(),
            Some(project.id.clone()),
            None,
            Some(owner.id.clone()),
            |_| Ok(())
        )
        .is_err());
        assert!(create_chat(
            &sessions,
            "kimi_chat",
            owner.cwd.clone(),
            Some(project.id.clone()),
            None,
            None,
            |_| Ok(())
        )
        .is_err());
        let borrow = |from: &SessionRecord| {
            create_chat(
                &sessions,
                "kimi_chat",
                from.cwd.clone(),
                Some(project.id.clone()),
                None,
                Some(from.id.clone()),
                |_| Ok(()),
            )
            .unwrap()
        };
        let first = borrow(&owner);
        let second = borrow(&first);
        assert_eq!(second.worktree_source_session_id.as_ref(), Some(&owner.id));
        sessions.archive(&second.id).unwrap();
        sessions.delete(&first.id).unwrap();
        assert!(clean_locked(&sessions, &owner, None)
            .unwrap_err()
            .to_string()
            .contains("仍引用"));
        assert!(Path::new(&tree.path).is_dir());
        sessions.delete(&second.id).unwrap();
        let mut unconfirmed = owner.clone();
        unconfirmed.worktree_runtime_owner = Some("interrupted-app-process".into());
        assert!(clean_locked(&sessions, &unconfirmed, None).is_err());
        clean_locked(&sessions, &owner, None).unwrap();
        assert!(!Path::new(&tree.path).exists());
        assert!(sessions.get(&owner.id).unwrap().worktree.is_some());
        assert!(verify_start(&owner, &sessions).is_err());
        // Use the same post-stop entry point as both archive commands.
        app.manage(PtyManager::new());
        let candidate = create_session(
            &sessions,
            &project.path,
            Some(&WorktreeOptions::default()),
            |id, cwd, tree| {
                sessions.create_claude_in(
                    crate::sessions::NewSession {
                        provider_id: "local".into(),
                        provider_name: "fixture".into(),
                        cwd,
                        yolo: false,
                        auto_mode: false,
                        project_id: Some(project.id.clone()),
                    },
                    id,
                    tree,
                )
            },
            |_| Ok(()),
        )
        .unwrap();
        let archived = sessions.archive(&candidate.id).unwrap();
        let checkout = &archived.worktree.as_ref().unwrap().path;
        after_archive(app.handle(), &sessions, &archived).unwrap();
        assert!(
            Path::new(checkout).is_dir(),
            "default archive must keep the checkout"
        );
        tauri::async_runtime::block_on(lifecycle::worktree_lifecycle_save(
            repo.display().to_string(),
            serde_json::from_value(serde_json::json!({"autoCleanup": true})).unwrap(),
        ))
        .unwrap();
        let private = Path::new(checkout).join("uncommitted.txt");
        fs::write(&private, "keep me").unwrap();
        assert!(after_archive(app.handle(), &sessions, &archived).is_err());
        assert_eq!(fs::read_to_string(&private).unwrap(), "keep me");
        fs::remove_file(private).unwrap();
        after_archive(app.handle(), &sessions, &archived).unwrap();
        assert!(!Path::new(checkout).exists());
        assert_eq!(
            fs::read_to_string(project_cwd.join("fixture.txt")).unwrap(),
            "original\n"
        );
    }

    #[test]
    fn managed_project_roundtrip_preserves_all_chat_identities_and_protects_borrowers() {
        const VARIABLE: &str = "CREWVELA_WORKTREE_TEST_PROFILE";
        if let Some(root) = std::env::var_os(VARIABLE) {
            isolated_roundtrip(Path::new(&root));
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "crewvela-worktree-managed-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "worktree_management::tests::managed_project_roundtrip_preserves_all_chat_identities_and_protects_borrowers", "--nocapture"])
            .env(VARIABLE, &root).output().unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr).into_owned();
        fs::remove_dir_all(&root).unwrap();
        assert!(output.status.success(), "{diagnostics}");
    }
}

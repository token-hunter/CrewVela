use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::launcher::{build_env, resolve_claude};
use crate::providers::ProviderConfig;

const GIT_TIMEOUT: Duration = Duration::from_secs(90);
const AI_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_AI_PATCH_CHARS: usize = 60_000;
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

fn default_pull_strategy() -> String {
    "ff_only".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSettings {
    pub branch_prefix: String,
    pub compare_strategy: String,
    pub commit_instruction: String,
    #[serde(default = "default_pull_strategy")]
    pub pull_strategy: String,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            branch_prefix: "crewvela/".into(),
            compare_strategy: "repository_default".into(),
            commit_instruction: "使用 Conventional Commits，准确概括已暂存变更。".into(),
            pull_strategy: default_pull_strategy(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileEntry {
    pub path: String,
    pub original_path: Option<String>,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub conflicted: bool,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommittedEntry {
    pub path: String,
    pub status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusSnapshot {
    pub cwd: String,
    pub repo_root: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub merge_in_progress: bool,
    pub default_base: Option<String>,
    pub compare_base: Option<String>,
    pub entries: Vec<GitFileEntry>,
    pub committed_entries: Vec<GitCommittedEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActionReceipt {
    pub message: String,
    pub commit: Option<String>,
    pub warning: bool,
    pub status: GitStatusSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchOption {
    pub name: String,
    pub remote: bool,
}

pub fn load_settings() -> Result<GitSettings> {
    let _guard = SETTINGS_LOCK.lock().map_err(|_| anyhow!("Git 设置锁不可用"))?;
    load_settings_at(&settings_path()?)
}

fn load_settings_at(path: &Path) -> Result<GitSettings> {
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            let mut settings = serde_json::from_str::<GitSettings>(&raw)
                .with_context(|| format!("解析 Git 设置失败：{}", path.display()))?;
            // Only the exact historical default is application-owned. Custom
            // prefixes and existing branch refs are never renamed here.
            let migrate_default = format!("{:x}", Sha256::digest(settings.branch_prefix.as_bytes()))
                == "8c019bf55744916b2f958059be2afd41c90320b7d11d549f34be760ff017e5a8";
            if migrate_default {
                settings.branch_prefix = GitSettings::default().branch_prefix;
            }
            let settings = validate_settings(settings)?;
            if migrate_default {
                if std::fs::read_to_string(path)? != raw {
                    return Err(anyhow!("Git 设置在迁移期间发生变化，请重新读取"));
                }
                save_settings_at(path, &settings).context("迁移 Git 默认分支前缀失败")?;
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(GitSettings::default()),
        Err(error) => Err(error).with_context(|| format!("读取 Git 设置失败：{}", path.display())),
    }
}

pub fn save_settings(settings: GitSettings) -> Result<GitSettings> {
    let _guard = SETTINGS_LOCK.lock().map_err(|_| anyhow!("Git 设置锁不可用"))?;
    let settings = validate_settings(settings)?;
    save_settings_at(&settings_path()?, &settings)?;
    Ok(settings)
}

fn save_settings_at(path: &Path, settings: &GitSettings) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temporary = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
            .context("创建 Git 设置临时文件失败")?;
        file.write_all(&serde_json::to_vec_pretty(settings)?).context("写入 Git 设置失败")?;
        if let Ok(metadata) = std::fs::metadata(path) { file.set_permissions(metadata.permissions())?; }
        file.sync_all().context("同步 Git 设置失败")?;
        drop(file);
        // std::fs::rename replaces an existing file on Windows and Unix. Never
        // delete the destination first: a failed replacement must keep it intact.
        std::fs::rename(&temporary, path).context("替换 Git 设置失败")?;
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
}

pub fn managed_branch_name(kind: &str, suffix: &str) -> String {
    #[cfg(test)]
    let prefix = GitSettings::default().branch_prefix;
    #[cfg(not(test))]
    let prefix = load_settings()
        .map(|settings| settings.branch_prefix)
        .unwrap_or_else(|_| GitSettings::default().branch_prefix);
    format!("{prefix}{kind}-{suffix}")
}

pub fn status(cwd: &Path, compare_strategy: Option<&str>) -> Result<GitStatusSnapshot> {
    let root = validated_repo_root(cwd)?;
    let raw = run_git_bytes(
        &root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    let mut parsed = parse_porcelain_v2(&raw)?;
    let unstaged_stats = diff_numstat(&root, false, None)?;
    let staged_stats = diff_numstat(&root, true, None)?;
    for entry in &mut parsed.entries {
        let stat = if entry.unstaged || entry.untracked {
            unstaged_stats.get(&entry.path)
        } else {
            staged_stats.get(&entry.path)
        };
        if let Some((additions, deletions)) = stat {
            entry.additions = *additions;
            entry.deletions = *deletions;
        }
    }

    let upstream = parsed.upstream.clone().or_else(|| current_upstream(&root));
    let default_base = default_remote_branch(&root, upstream.as_deref());
    let configured = load_settings().unwrap_or_default().compare_strategy;
    let strategy = compare_strategy.unwrap_or(&configured);
    let compare_base = if strategy == "branch_upstream" {
        upstream.clone().or_else(|| default_base.clone())
    } else {
        default_base.clone().or_else(|| upstream.clone())
    };
    let committed_entries = match compare_base.as_deref() {
        Some(base) if git_ref_exists(&root, base) => committed_entries(&root, base)?,
        _ => Vec::new(),
    };
    Ok(GitStatusSnapshot {
        cwd: cwd.display().to_string(),
        repo_root: root.display().to_string(),
        branch: parsed.branch.clone(),
        detached: parsed.branch.as_deref() == Some("(detached)"),
        head: parsed.head,
        upstream,
        ahead: parsed.ahead,
        merge_in_progress: merge_in_progress(&root),
        behind: parsed.behind,
        default_base,
        compare_base,
        entries: parsed.entries,
        committed_entries,
    })
}

pub fn diff(cwd: &Path, path: Option<&str>, area: &str, base: Option<&str>) -> Result<String> {
    let root = validated_repo_root(cwd)?;
    let mut args = vec!["--literal-pathspecs", "diff", "--no-ext-diff", "--no-color", "--unified=3"];
    let committed_range;
    match area {
        "staged" => args.push("--cached"),
        "committed" => {
            let base = base.ok_or_else(|| anyhow!("缺少比较基准"))?;
            validate_ref(base)?;
            committed_range = format!("{base}...HEAD");
            args.push(&committed_range);
        }
        "unstaged" => {}
        "untracked" => {
            let path = path.ok_or_else(|| anyhow!("未跟踪文件 Diff 缺少路径"))?;
            let relative = validated_relative_path(&root, path, true)?;
            return untracked_diff(&root, &relative);
        }
        _ => return Err(anyhow!("未知 Diff 类型：{area}")),
    }
    let owned;
    if let Some(path) = path {
        owned = validated_relative_path(&root, path, false)?;
        args.push("--");
        args.push(owned.as_str());
    }
    let output = run_git(&root, &args)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// These entry points receive exact file entries, never caller-authored pathspecs.
// `--` ends option parsing but still permits glob/magic matching, so all per-file
// commands also enable Git's global --literal-pathspecs option.
pub fn stage(cwd: &Path, paths: &[String]) -> Result<GitStatusSnapshot> {
    mutate_paths(cwd, paths, |root, paths| {
        let mut args = vec!["--literal-pathspecs", "add", "--"];
        args.extend(paths.iter().map(String::as_str));
        run_git(root, &args).map(|_| ())
    })
}

pub fn unstage(cwd: &Path, paths: &[String]) -> Result<GitStatusSnapshot> {
    mutate_paths(cwd, paths, |root, paths| {
        let mut args = if git_ref_exists(root, "HEAD") {
            vec!["--literal-pathspecs", "restore", "--staged", "--"]
        } else {
            vec!["--literal-pathspecs", "rm", "--cached", "--"]
        };
        args.extend(paths.iter().map(String::as_str));
        run_git(root, &args).map(|_| ())
    })
}

pub fn discard(cwd: &Path, paths: &[String]) -> Result<GitStatusSnapshot> {
    let root = validated_repo_root(cwd)?;
    let snapshot = status(&root, None)?;
    let mut tracked = Vec::new();
    let mut untracked = Vec::new();
    for path in validated_paths(&root, paths, false)? {
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .ok_or_else(|| anyhow!("文件已不在 Git 变更列表中：{path}"))?;
        if entry.untracked {
            untracked.push(path);
        } else if entry.unstaged {
            tracked.push(path);
        } else {
            return Err(anyhow!("该文件没有可丢弃的工作区变更：{path}"));
        }
    }
    if !tracked.is_empty() {
        let mut args = vec!["--literal-pathspecs", "restore", "--worktree", "--"];
        args.extend(tracked.iter().map(String::as_str));
        run_git(&root, &args)?;
    }
    for path in untracked {
        let target = root.join(&path);
        let parent = target
            .parent()
            .context("未跟踪文件缺少父目录")?
            .canonicalize()
            .with_context(|| format!("未跟踪文件父目录不存在：{path}"))?;
        if !parent.starts_with(&root) || parent.starts_with(root.join(".git")) {
            return Err(anyhow!("拒绝删除工作区之外的文件：{path}"));
        }
        let metadata = std::fs::symlink_metadata(&target)?;
        if metadata.is_dir() {
            return Err(anyhow!("P0 仅允许删除未跟踪文件，不递归删除目录：{path}"));
        }
        std::fs::remove_file(&target)?;
    }
    status(&root, None)
}

pub fn commit(cwd: &Path, message: &str, push_after: bool) -> Result<GitActionReceipt> {
    let root = validated_repo_root(cwd)?;
    let message = message.trim();
    if message.is_empty() {
        return Err(anyhow!("提交信息不能为空"));
    }
    let snapshot = status(&root, None)?;
    if !snapshot.entries.iter().any(|entry| entry.staged) {
        return Err(anyhow!("没有已暂存的变更"));
    }
    run_git(&root, &["commit", "-m", message])?;
    let commit = git_text(&root, &["rev-parse", "HEAD"])?;
    let (receipt_message, warning) = if push_after {
        match push(&root, false) {
            Ok(message) => (message, false),
            Err(error) => (format!("提交成功，但推送失败：{error}"), true),
        }
    } else {
        ("提交成功".to_string(), false)
    };
    Ok(GitActionReceipt {
        message: receipt_message,
        commit: Some(commit),
        warning,
        status: status(&root, None)?,
    })
}

fn pull_args(strategy: &str) -> [&'static str; 2] {
    match strategy {
        "rebase" => ["pull", "--rebase"],
        _ => ["pull", "--ff-only"],
    }
}

pub fn remote(cwd: &Path, action: &str) -> Result<GitActionReceipt> {
    let root = validated_repo_root(cwd)?;
    let message = match action {
        "fetch" => {
            run_git(&root, &["fetch", "--prune"])?;
            "Fetch 完成".to_string()
        }
        "pull" => {
            let strategy = load_settings()
                .map(|settings| settings.pull_strategy)
                .unwrap_or_else(|_| default_pull_strategy());
            run_git(&root, &pull_args(&strategy))?;
            "Pull 完成".to_string()
        }
        "push" => push(&root, false)?,
        "publish" => publish(&root)?,
        "force_push" => push(&root, true)?,
        _ => return Err(anyhow!("未知 Git 远程操作：{action}")),
    };
    Ok(GitActionReceipt {
        message,
        commit: None,
        warning: false,
        status: status(&root, None)?,
    })
}

pub fn branches(cwd: &Path) -> Result<Vec<GitBranchOption>> {
    let root = validated_repo_root(cwd)?;
    let current = current_branch(&root)?;
    let raw = git_text(
        &root,
        &[
            "for-each-ref",
            "--format=%(refname:short)\t%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let mut items = raw
        .lines()
        .filter_map(|line| {
            let (name, full_ref) = line.split_once('\t')?;
            let name = name.trim();
            let full_ref = full_ref.trim();
            if name.is_empty() || current.as_deref() == Some(name) || full_ref.ends_with("/HEAD") {
                return None;
            }
            Some(GitBranchOption {
                name: name.to_string(),
                remote: full_ref.starts_with("refs/remotes/"),
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        left.remote
            .cmp(&right.remote)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(items)
}

pub fn merge(cwd: &Path, source: &str, no_ff: bool) -> Result<GitActionReceipt> {
    let root = validated_repo_root(cwd)?;
    validate_ref(source)?;
    if merge_in_progress(&root) {
        return Err(anyhow!("当前已有未完成的 Merge，请先解决冲突或取消合并"));
    }
    let current = current_branch(&root)?.ok_or_else(|| anyhow!("Detached HEAD 无法执行 Merge"))?;
    if current == source {
        return Err(anyhow!("不能将当前分支合并到自身"));
    }
    if !git_ref_exists(&root, source) {
        return Err(anyhow!("来源分支不存在：{source}"));
    }
    let before = status(&root, None)?;
    if !before.entries.is_empty() {
        return Err(anyhow!(
            "工作区存在未提交变更，请先提交、暂存后提交，或清理变更再合并"
        ));
    }

    let mut args = vec!["merge", "--no-edit"];
    if no_ff {
        args.push("--no-ff");
    }
    args.push(source);
    let output = run_git_allow_failure(&root, &args)?;
    if output.status.success() {
        let commit = git_text(&root, &["rev-parse", "HEAD"])?;
        return Ok(GitActionReceipt {
            message: if no_ff {
                format!("已使用 --no-ff 将 {source} 合并到 {current}")
            } else {
                format!("已将 {source} 合并到 {current}")
            },
            commit: Some(commit),
            warning: false,
            status: status(&root, None)?,
        });
    }

    let next = status(&root, None)?;
    let detail = String::from_utf8_lossy(&output.stderr)
        .trim()
        .lines()
        .last()
        .unwrap_or("Git Merge 执行失败")
        .to_string();
    if next.merge_in_progress || next.entries.iter().any(|entry| entry.conflicted) {
        Ok(GitActionReceipt {
            message: format!("合并 {source} 时发生冲突。请解决冲突后提交，或取消本次合并。"),
            commit: None,
            warning: true,
            status: next,
        })
    } else {
        Err(anyhow!("合并失败：{detail}"))
    }
}

pub fn merge_abort(cwd: &Path) -> Result<GitActionReceipt> {
    let root = validated_repo_root(cwd)?;
    if !merge_in_progress(&root) {
        return Err(anyhow!("当前没有进行中的 Merge"));
    }
    run_git(&root, &["merge", "--abort"])?;
    Ok(GitActionReceipt {
        message: "已取消本次合并并恢复合并前状态".into(),
        commit: None,
        warning: false,
        status: status(&root, None)?,
    })
}

pub fn generate_commit_message(
    cwd: &Path,
    provider: &ProviderConfig,
    key: &str,
    instruction: &str,
) -> Result<String> {
    let _runtime_lease =
        crate::runtime_preferences::acquire(crate::multi_agent::AgentRuntimeKind::ClaudeCode)?;
    let root = validated_repo_root(cwd)?;
    let summary = git_text(&root, &["diff", "--cached", "--stat"])?;
    if summary.trim().is_empty() {
        return Err(anyhow!("没有已暂存的变更，无法生成提交信息"));
    }
    let patch = git_text(&root, &["diff", "--cached", "--no-color", "--unified=2"])?;
    let prompt = format!(
        "根据以下已暂存变更生成一条 Git commit message。\n要求：{}\n只输出一行提交信息，不要解释、引号或 Markdown。\n\n变更摘要：\n{}\n\n补丁：\n{}",
        instruction.trim(),
        summary,
        patch.chars().take(MAX_AI_PATCH_CHARS).collect::<String>(),
    );
    let env = build_env(provider, key);
    let settings_json = serde_json::json!({ "env": env }).to_string();
    let claude = resolve_claude()?;
    let mut command = crate::title_summary::claude_command(&claude)?;
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
        .arg("你是 Git 提交信息生成器。只输出一行准确、简洁、可编辑的提交信息。")
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = crate::title_summary::run_cli_with_timeout(command, Some(prompt.into_bytes()), AI_TIMEOUT, 8 * 1024 * 1024, "生成 Git 提交信息")?;
    if !output.status.success() {
        let detail: String = String::from_utf8_lossy(&output.stderr).chars().take(400).collect();
        return Err(anyhow!("Git 提交信息生成失败：{}（{detail}）", output.status));
    }
    commit_message_from_response(&output.stdout)
}

fn commit_message_from_response(raw: &[u8]) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_slice(raw).context("Git 提交信息生成结果不是有效 JSON")?;
    if value.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        let detail: String = value.get("result").and_then(serde_json::Value::as_str).unwrap_or("未知错误").chars().take(400).collect();
        return Err(anyhow!("Agent 返回提交信息生成错误：{detail}"));
    }
    let result = value
        .get("result")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let message = result
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_matches(['`', '"', '\''])
        .chars()
        .take(200)
        .collect::<String>();
    if message.is_empty() {
        Err(anyhow!("Agent 未返回可用的提交信息"))
    } else {
        Ok(message)
    }
}

fn settings_path() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("git-settings.json"))
}

fn validate_settings(mut settings: GitSettings) -> Result<GitSettings> {
    settings.branch_prefix = normalize_branch_prefix(&settings.branch_prefix)?;
    if !matches!(
        settings.compare_strategy.as_str(),
        "repository_default" | "branch_upstream"
    ) {
        return Err(anyhow!("未知 Git 比较策略"));
    }
    if !matches!(settings.pull_strategy.as_str(), "ff_only" | "rebase") {
        return Err(anyhow!("未知 Git Pull 策略"));
    }
    settings.commit_instruction = settings
        .commit_instruction
        .trim()
        .chars()
        .take(2_000)
        .collect();
    Ok(settings)
}

fn normalize_branch_prefix(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let candidate = format!("{trimmed}/probe");
    let output = Command::new("git")
        .args(["check-ref-format", "--branch", &candidate])
        .output()
        .context("无法验证分支前缀")?;
    if !output.status.success() {
        return Err(anyhow!("分支前缀无效：{trimmed}"));
    }
    Ok(format!("{trimmed}/"))
}

fn validated_repo_root(cwd: &Path) -> Result<PathBuf> {
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("工作目录不存在：{}", cwd.display()))?;
    let root = git_text(&cwd, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root).canonicalize()?;
    if !cwd.starts_with(&root) {
        return Err(anyhow!("工作目录不属于解析出的 Git 仓库"));
    }
    Ok(root)
}

fn validated_relative_path(root: &Path, value: &str, must_exist: bool) -> Result<String> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!("Git 文件路径无效：{value}"));
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    if is_git_metadata_path(path) {
        return Err(anyhow!("禁止操作 Git 元数据路径"));
    }
    if must_exist && !root.join(path).exists() {
        return Err(anyhow!("文件不存在：{value}"));
    }
    Ok(normalized)
}

fn is_git_metadata_path(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(name) = component else { return false; };
        let name = name.to_string_lossy();
        if cfg!(windows) {
            name.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(".git")
        } else { name == ".git" }
    })
}

fn validated_paths(root: &Path, paths: &[String], must_exist: bool) -> Result<Vec<String>> {
    if paths.is_empty() {
        return Err(anyhow!("至少选择一个文件"));
    }
    paths
        .iter()
        .map(|path| validated_relative_path(root, path, must_exist))
        .collect()
}

fn mutate_paths(
    cwd: &Path,
    paths: &[String],
    operation: impl FnOnce(&Path, &[String]) -> Result<()>,
) -> Result<GitStatusSnapshot> {
    let root = validated_repo_root(cwd)?;
    let paths = validated_paths(&root, paths, false)?;
    operation(&root, &paths)?;
    status(&root, None)
}

fn push(root: &Path, force: bool) -> Result<String> {
    if current_upstream(root).is_none() {
        return publish(root);
    }
    if force {
        run_git(root, &["push", "--force-with-lease"])?;
        Ok("已使用 force-with-lease 推送".into())
    } else {
        run_git(root, &["push"])?;
        Ok("Push 完成".into())
    }
}

fn publish(root: &Path) -> Result<String> {
    let branch = current_branch(root)?.ok_or_else(|| anyhow!("Detached HEAD 无法 Publish"))?;
    let remotes = git_text(root, &["remote"])?;
    let remote = remotes
        .lines()
        .find(|remote| *remote == "origin")
        .or_else(|| remotes.lines().find(|remote| !remote.trim().is_empty()))
        .ok_or_else(|| anyhow!("仓库没有可用远端"))?;
    run_git(root, &["push", "--set-upstream", remote, &branch])?;
    Ok(format!("已发布到 {remote}/{branch}"))
}

fn current_branch(root: &Path) -> Result<Option<String>> {
    let output = run_git_allow_failure(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    } else {
        Ok(None)
    }
}

fn current_upstream(root: &Path) -> Option<String> {
    run_git_allow_failure(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .ok()
    .filter(|output| output.status.success())
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    .filter(|value| !value.is_empty())
}

fn default_remote_branch(root: &Path, upstream: Option<&str>) -> Option<String> {
    let remote = upstream
        .and_then(|value| value.split_once('/').map(|(remote, _)| remote))
        .unwrap_or("origin");
    let reference = format!("refs/remotes/{remote}/HEAD");
    run_git_allow_failure(root, &["symbolic-ref", "--quiet", "--short", &reference])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn merge_in_progress(root: &Path) -> bool {
    let Ok(output) = run_git_allow_failure(root, &["rev-parse", "--git-path", "MERGE_HEAD"]) else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        return false;
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    path.exists()
}

fn git_ref_exists(root: &Path, reference: &str) -> bool {
    run_git_allow_failure(root, &["rev-parse", "--verify", "--quiet", reference])
        .is_ok_and(|output| output.status.success())
}

fn validate_ref(reference: &str) -> Result<()> {
    if reference.trim().is_empty()
        || reference.starts_with('-')
        || reference.chars().any(char::is_whitespace)
    {
        Err(anyhow!("Git 比较基准无效"))
    } else {
        Ok(())
    }
}

fn committed_entries(root: &Path, base: &str) -> Result<Vec<GitCommittedEntry>> {
    validate_ref(base)?;
    let range = format!("{base}...HEAD");
    let output = run_git_bytes(root, &["diff", "--name-status", "-z", &range])?;
    let stats = diff_numstat(root, false, Some(&range))?;
    let parts = output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        let status = String::from_utf8_lossy(parts[index]).into_owned();
        index += 1;
        let code = status.chars().next().unwrap_or('M');
        let path = if matches!(code, 'R' | 'C') {
            if index + 1 >= parts.len() {
                break;
            }
            index += 1;
            let path = String::from_utf8_lossy(parts[index]).into_owned();
            index += 1;
            path
        } else {
            if index >= parts.len() {
                break;
            }
            let path = String::from_utf8_lossy(parts[index]).into_owned();
            index += 1;
            path
        };
        let stat = stats.get(&path).copied().unwrap_or((None, None));
        entries.push(GitCommittedEntry {
            path,
            status: code.to_string(),
            additions: stat.0,
            deletions: stat.1,
        });
    }
    Ok(entries)
}

fn diff_numstat(
    root: &Path,
    staged: bool,
    range: Option<&str>,
) -> Result<HashMap<String, (Option<u64>, Option<u64>)>> {
    let mut args = vec!["diff", "--numstat", "-z"];
    if staged {
        args.push("--cached");
    }
    if let Some(range) = range {
        args.push(range);
    }
    let raw = run_git_bytes(root, &args)?;
    let mut map = HashMap::new();
    let mut records = raw.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if record.is_empty() { continue; }
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let Some(additions) = fields.next() else { continue; };
        let Some(deletions) = fields.next() else { continue; };
        let Some(path) = fields.next() else { continue; };
        let path = if path.is_empty() {
            // --numstat -z emits renamed/copied paths as: counts\t\tNUL old NUL new NUL.
            if records.next().is_none() { break; }
            let Some(destination) = records.next() else { break; };
            destination
        } else { path };
        map.insert(
            String::from_utf8_lossy(path).into_owned(),
            (String::from_utf8_lossy(additions).parse().ok(), String::from_utf8_lossy(deletions).parse().ok()),
        );
    }
    Ok(map)
}

struct ParsedStatus {
    branch: Option<String>,
    head: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    entries: Vec<GitFileEntry>,
}

fn parse_porcelain_v2(raw: &[u8]) -> Result<ParsedStatus> {
    let records = raw.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut result = ParsedStatus {
        branch: None,
        head: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        entries: Vec::new(),
    };
    let mut index = 0;
    while index < records.len() {
        let record = String::from_utf8_lossy(records[index]).into_owned();
        index += 1;
        if record.is_empty() {
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.oid ") {
            if value != "(initial)" {
                result.head = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.head ") {
            result.branch = Some(value.to_string());
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.upstream ") {
            result.upstream = Some(value.to_string());
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.ab ") {
            for part in value.split_whitespace() {
                if let Some(ahead) = part.strip_prefix('+') {
                    result.ahead = ahead.parse().unwrap_or(0);
                }
                if let Some(behind) = part.strip_prefix('-') {
                    result.behind = behind.parse().unwrap_or(0);
                }
            }
            continue;
        }
        let (xy, path, original_path, untracked, conflicted) =
            if let Some(value) = record.strip_prefix("1 ") {
                let mut fields = value.splitn(8, ' ');
                let xy = fields.next().unwrap_or("..").to_string();
                let path = fields.nth(6).unwrap_or_default().to_string();
                (xy, path, None, false, false)
            } else if let Some(value) = record.strip_prefix("2 ") {
                let mut fields = value.splitn(9, ' ');
                let xy = fields.next().unwrap_or("..").to_string();
                let path = fields.nth(7).unwrap_or_default().to_string();
                let original = records
                    .get(index)
                    .map(|value| String::from_utf8_lossy(value).into_owned());
                index += usize::from(original.is_some());
                (xy, path, original, false, false)
            } else if let Some(value) = record.strip_prefix("u ") {
                let mut fields = value.splitn(10, ' ');
                let xy = fields.next().unwrap_or("UU").to_string();
                let path = fields.nth(8).unwrap_or_default().to_string();
                (xy, path, None, false, true)
            } else if let Some(path) = record.strip_prefix("? ") {
                ("??".to_string(), path.to_string(), None, true, false)
            } else {
                continue;
            };
        if path.is_empty() {
            continue;
        }
        let chars = xy.chars().collect::<Vec<_>>();
        let x = chars.first().copied().unwrap_or('.');
        let y = chars.get(1).copied().unwrap_or('.');
        result.entries.push(GitFileEntry {
            path,
            original_path,
            status: if untracked {
                "?".into()
            } else if conflicted {
                "U".into()
            } else {
                [x, y].iter().collect()
            },
            staged: !untracked && !conflicted && !matches!(x, '.' | ' '),
            unstaged: !untracked && !conflicted && !matches!(y, '.' | ' '),
            untracked,
            conflicted,
            additions: None,
            deletions: None,
        });
    }
    Ok(result)
}

fn untracked_diff(root: &Path, path: &str) -> Result<String> {
    // Reading a symlink follows it, unlike discard which unlinks the entry itself.
    // Resolve only this read path and reject external targets or metadata aliases.
    let target = root.join(path).canonicalize().context("解析未跟踪文件失败")?;
    let relative = target.strip_prefix(root).map_err(|_| anyhow!("拒绝读取仓库之外的文件"))?;
    if is_git_metadata_path(relative) { return Err(anyhow!("禁止读取 Git 元数据路径")); }
    let bytes = std::fs::read(&target)?;
    if bytes.contains(&0) {
        return Ok(format!("Binary file: {path}"));
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut output = format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n"
    );
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            output.push_str(&format!("@@ -0,0 +1,{} @@\n", text.lines().count()));
        }
        output.push('+');
        output.push_str(line);
        output.push('\n');
    }
    Ok(output)
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn run_git(root: &Path, args: &[&str]) -> Result<Output> {
    let output = run_git_allow_failure(root, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(anyhow!(
            "Git 操作失败：{}",
            if stderr.is_empty() { stdout } else { stderr }
        ))
    }
}

fn run_git_allow_failure(root: &Path, args: &[&str]) -> Result<Output> {
    let mut command = git_command(root);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_with_timeout(command, GIT_TIMEOUT, &format!("git {}", args.join(" ")))
}

fn run_git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    Ok(run_git(root, args)?.stdout)
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&run_git(root, args)?.stdout)
        .trim()
        .to_string())
}

fn run_with_timeout(command: Command, timeout: Duration, operation: &str) -> Result<Output> {
    // Git output can legitimately contain large patches; keep its existing stdout
    // contract while sharing concurrent pipe draining and bounded timeout cleanup.
    crate::title_summary::run_cli_with_timeout(command, None, timeout, usize::MAX, operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TestRepo {
        root: PathBuf,
    }

    impl TestRepo {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("crewvela-git-p0-{name}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init"]);
            git(&root, &["config", "user.name", "CrewVela Test"]);
            git(&root, &["config", "user.email", "crewvela@test.local"]);
            fs::write(root.join("tracked.txt"), "one\n").unwrap();
            git(&root, &["add", "tracked.txt"]);
            git(&root, &["commit", "-m", "init"]);
            Self { root }
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = run_git(root, args).unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
    #[test]
    fn committed_diff_uses_merge_base_and_preserves_path_filter() {
        let repo = TestRepo::new("committed-diff");
        git(&repo.root, &["branch", "comparison-base"]);
        git(&repo.root, &["checkout", "-b", "comparison-feature"]);
        fs::write(repo.root.join("tracked.txt"), "one\nfeature\n").unwrap();
        git(&repo.root, &["commit", "-am", "feature change"]);
        git(&repo.root, &["checkout", "comparison-base"]);
        fs::write(repo.root.join("base-only.txt"), "base change\n").unwrap();
        git(&repo.root, &["add", "base-only.txt"]);
        git(&repo.root, &["commit", "-m", "base change"]);
        git(&repo.root, &["checkout", "comparison-feature"]);
        let patch = diff(&repo.root, None, "committed", Some("comparison-base")).unwrap();
        assert!(patch.contains("+feature"));
        assert!(!patch.contains("base-only.txt"));
        assert_eq!(diff(&repo.root, Some("tracked.txt"), "committed", Some("comparison-base")).unwrap(), patch);
        assert!(diff(&repo.root, Some("base-only.txt"), "committed", Some("comparison-base")).unwrap().is_empty());
    }

    #[test]
    fn commit_message_rejects_error_envelopes_and_sanitizes_success() {
        assert_eq!(commit_message_from_response(br#"{"result":"  \"fix: literal commit\"\nextra"}"#).unwrap(), "fix: literal commit");
        let error = commit_message_from_response(br#"{"is_error":true,"result":"API Error: quota exhausted"}"#).unwrap_err();
        assert!(error.to_string().contains("quota exhausted"));
        assert!(commit_message_from_response(br#"{"result":"  "}"#).is_err());
    }

    #[test]
    fn git_settings_replace_existing_config_without_sidecars() {
        let root = std::env::temp_dir().join(format!("cc-git-settings-{}", uuid::Uuid::new_v4()));
        let path = root.join("settings.json");
        save_settings_at(&path, &GitSettings::default()).unwrap();
        let updated = GitSettings { branch_prefix: "task/".into(), ..GitSettings::default() };
        save_settings_at(&path, &updated).unwrap();
        let loaded: GitSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(loaded.branch_prefix, "task/");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn brand_migration_changes_only_the_exact_historical_default_prefix() {
        let root = std::env::temp_dir().join(format!("crewvela-git-brand-{}", uuid::Uuid::new_v4()));
        let path = root.join("settings.json");
        // Historical persisted fixture bytes, kept separate from current names.
        let historical = String::from_utf8(vec![99, 99, 45, 100, 111, 99, 107, 47]).unwrap();
        let old = GitSettings { branch_prefix: historical.clone(), ..GitSettings::default() };
        save_settings_at(&path, &old).unwrap();
        assert_eq!(load_settings_at(&path).unwrap().branch_prefix, "crewvela/");
        let persisted: GitSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted.branch_prefix, "crewvela/");
        assert_eq!(load_settings_at(&path).unwrap().branch_prefix, "crewvela/");
        for custom in ["team/".to_string(), format!("{historical}custom/")] {
            let settings = GitSettings { branch_prefix: custom.clone(), ..GitSettings::default() };
            save_settings_at(&path, &settings).unwrap();
            let before = fs::read(&path).unwrap();
            assert_eq!(load_settings_at(&path).unwrap().branch_prefix, custom);
            assert_eq!(fs::read(&path).unwrap(), before);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn brand_migration_reports_failed_prefix_persistence_without_losing_old_settings() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::temp_dir().join(format!("crewvela-git-brand-locked-{}", uuid::Uuid::new_v4()));
        let path = root.join("settings.json");
        let settings = GitSettings {
            branch_prefix: String::from_utf8(vec![99, 99, 45, 100, 111, 99, 107, 47]).unwrap(),
            ..GitSettings::default()
        };
        save_settings_at(&path, &settings).unwrap();
        let before = fs::read(&path).unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        assert!(load_settings_at(&path).unwrap_err().to_string().contains("迁移"));
        assert_eq!(fs::read(&path).unwrap(), before);
        drop(held);
        assert_eq!(load_settings_at(&path).unwrap().branch_prefix, "crewvela/");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn git_settings_failed_replacement_preserves_existing_config() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::temp_dir().join(format!("cc-git-settings-locked-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        fs::write(&path, b"original settings").unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
        assert!(save_settings_at(&path, &GitSettings::default()).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original settings");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        drop(held);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pull_args_follow_the_configured_strategy_and_default_to_ff_only() {
        assert_eq!(pull_args("ff_only"), ["pull", "--ff-only"]);
        assert_eq!(pull_args("rebase"), ["pull", "--rebase"]);
        assert_eq!(pull_args("anything-else"), ["pull", "--ff-only"]);
    }

    #[test]
    fn saved_settings_without_pull_strategy_default_to_ff_only() {
        let settings: GitSettings = serde_json::from_str(
            r#"{"branchPrefix":"crewvela/","compareStrategy":"repository_default","commitInstruction":"x"}"#,
        )
        .unwrap();
        assert_eq!(settings.pull_strategy, "ff_only");
    }

    #[test]
    fn validate_settings_rejects_unknown_pull_strategy() {
        let settings = GitSettings {
            pull_strategy: "merge".into(),
            ..GitSettings::default()
        };
        assert!(validate_settings(settings).is_err());
    }

    #[test]
    fn status_stage_unstage_diff_and_commit_form_a_real_loop() {
        let repo = TestRepo::new("loop");
        fs::write(repo.root.join("tracked.txt"), "one\ntwo\n").unwrap();
        fs::write(repo.root.join("new.txt"), "new\n").unwrap();
        let initial = status(&repo.root, None).unwrap();
        assert!(initial
            .entries
            .iter()
            .any(|entry| entry.path == "tracked.txt" && entry.unstaged));
        assert!(initial
            .entries
            .iter()
            .any(|entry| entry.path == "new.txt" && entry.untracked));
        assert!(diff(&repo.root, Some("tracked.txt"), "unstaged", None)
            .unwrap()
            .contains("+two"));

        stage(&repo.root, &["tracked.txt".into(), "new.txt".into()]).unwrap();
        let staged = status(&repo.root, None).unwrap();
        assert!(staged.entries.iter().all(|entry| entry.staged));
        unstage(&repo.root, &["new.txt".into()]).unwrap();
        let receipt = commit(&repo.root, "feat: update tracked file", false).unwrap();
        assert!(receipt.commit.is_some());
        assert!(repo.root.join("new.txt").exists());
    }

    #[test]
    fn bracket_path_stage_unstage_discard_and_all_diffs_are_literal() {
        let repo = TestRepo::new("literal-bracket-paths");
        let paths = ["app/[id].tsx", "app/i.tsx", "app/d.tsx"];
        fs::create_dir(repo.root.join("app")).unwrap();
        for path in paths { fs::write(repo.root.join(path), format!("baseline {path}\n")).unwrap(); }
        git(&repo.root, &["add", "--all"]);
        git(&repo.root, &["commit", "-m", "add route fixtures"]);
        git(&repo.root, &["branch", "literal-base"]);
        let markers = ["selected bracket change", "innocent i change", "innocent d change"];
        for (path, marker) in paths.into_iter().zip(markers) {
            fs::write(repo.root.join(path), format!("baseline {path}\n{marker}\n")).unwrap();
        }
        let assert_selected_patch = |area, base| {
            let patch = diff(&repo.root, Some(paths[0]), area, base).unwrap();
            assert!(patch.contains("+selected bracket change"), "missing selected route in {area}: {patch}");
            assert!(!patch.contains("innocent i change"), "i.tsx leaked into {area}");
            assert!(!patch.contains("innocent d change"), "d.tsx leaked into {area}");
        };
        assert_selected_patch("unstaged", None);
        let snapshot = stage(&repo.root, &[paths[0].into()]).unwrap();
        assert!(snapshot.entries.iter().find(|entry| entry.path == paths[0]).unwrap().staged);
        for sibling in &paths[1..] {
            let entry = snapshot.entries.iter().find(|entry| entry.path == *sibling).unwrap();
            assert!(!entry.staged && entry.unstaged, "stage touched {sibling}");
        }
        stage(&repo.root, &paths[1..].iter().map(|path| (*path).into()).collect::<Vec<_>>()).unwrap();
        assert_selected_patch("staged", None);
        let snapshot = unstage(&repo.root, &[paths[0].into()]).unwrap();
        let selected = snapshot.entries.iter().find(|entry| entry.path == paths[0]).unwrap();
        assert!(!selected.staged && selected.unstaged);
        for sibling in &paths[1..] {
            let entry = snapshot.entries.iter().find(|entry| entry.path == *sibling).unwrap();
            assert!(entry.staged && !entry.unstaged, "unstage touched {sibling}");
        }
        // Give siblings additional unstaged edits: restoring their staged versions
        // would silently lose these even though the staged markers still match.
        for (&path, &marker) in paths[1..].iter().zip(&markers[1..]) {
            fs::write(repo.root.join(path), format!("baseline {path}\n{marker}\nworking-only {path}\n")).unwrap();
        }
        discard(&repo.root, &[paths[0].into()]).unwrap();
        assert_eq!(fs::read_to_string(repo.root.join(paths[0])).unwrap().replace("\r\n", "\n"), format!("baseline {}\n", paths[0]));
        for (&path, &marker) in paths[1..].iter().zip(&markers[1..]) {
            assert_eq!(fs::read_to_string(repo.root.join(path)).unwrap(), format!("baseline {path}\n{marker}\nworking-only {path}\n"), "discard touched {path}");
            assert_eq!(git(&repo.root, &["show", &format!(":{path}")]), format!("baseline {path}\n{marker}"), "discard changed index for {path}");
        }
        fs::write(repo.root.join(paths[0]), format!("baseline {}\nselected bracket change\n", paths[0])).unwrap();
        stage(&repo.root, &[paths[0].into()]).unwrap();
        git(&repo.root, &["commit", "-m", "change all fixtures"]);
        assert_selected_patch("committed", Some("literal-base"));
    }

    #[test]
    fn unborn_unstage_treats_brackets_as_a_literal_path() {
        let root = std::env::temp_dir().join(format!("cc-git-literal-unborn-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        git(&root, &["init"]);
        for path in ["[id].tsx", "i.tsx", "d.tsx"] { fs::write(root.join(path), "new route\n").unwrap(); }
        stage(&root, &["[id].tsx".into(), "i.tsx".into(), "d.tsx".into()]).unwrap();
        let snapshot = unstage(&root, &["[id].tsx".into()]).unwrap();
        let selected = snapshot.entries.iter().find(|entry| entry.path == "[id].tsx").unwrap();
        assert!(selected.untracked && !selected.staged);
        for sibling in ["i.tsx", "d.tsx"] {
            assert!(snapshot.entries.iter().find(|entry| entry.path == sibling).unwrap().staged);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discard_restores_tracked_and_deletes_only_confirmed_untracked_files() {
        let repo = TestRepo::new("discard");
        fs::write(repo.root.join("tracked.txt"), "changed\n").unwrap();
        fs::write(repo.root.join("scratch.txt"), "scratch\n").unwrap();
        discard(&repo.root, &["tracked.txt".into(), "scratch.txt".into()]).unwrap();
        assert_eq!(
            fs::read_to_string(repo.root.join("tracked.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "one\n"
        );
        assert!(!repo.root.join("scratch.txt").exists());
    }

    #[test]
    fn discard_can_restore_a_deleted_tracked_file() {
        let repo = TestRepo::new("discard-deleted");
        fs::remove_file(repo.root.join("tracked.txt")).unwrap();
        discard(&repo.root, &["tracked.txt".into()]).unwrap();
        assert!(repo.root.join("tracked.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn discard_untracked_symlink_keeps_its_target() {
        let repo = TestRepo::new("discard-symlink");
        let link = repo.root.join("link.txt");
        std::os::unix::fs::symlink(repo.root.join("tracked.txt"), &link).unwrap();
        discard(&repo.root, &["link.txt".into()]).unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(
            fs::read_to_string(repo.root.join("tracked.txt")).unwrap(),
            "one\n"
        );
    }

    #[test]
    fn unstage_supports_an_unborn_repository() {
        let root =
            std::env::temp_dir().join(format!("crewvela-git-unborn-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init"]);
        fs::write(root.join("first.txt"), "first\n").unwrap();
        stage(&root, &["first.txt".into()]).unwrap();
        let snapshot = unstage(&root, &["first.txt".into()]).unwrap();
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.path == "first.txt" && entry.untracked));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn path_validation_rejects_parent_and_git_metadata() {
        let repo = TestRepo::new("paths");
        assert!(stage(&repo.root, &["../outside.txt".into()]).is_err());
        assert!(discard(&repo.root, &[".git/config".into()]).is_err());
    }

    #[test]
    fn untracked_diff_rejects_metadata_even_with_current_directory_components() {
        let repo = TestRepo::new("metadata-path");
        for path in ["./.git/config", "nested/.git/config"] {
            assert!(diff(&repo.root, Some(path), "untracked", None).is_err());
        }
        #[cfg(windows)]
        for path in [".GiT/config", ".git./config"] {
            assert!(diff(&repo.root, Some(path), "untracked", None).is_err());
        }
    }

    #[test]
    fn renamed_files_keep_numstat_counts_for_the_destination() {
        let repo = TestRepo::new("rename-counts");
        let original = (0..20).map(|line| format!("line {line}\n")).collect::<String>();
        fs::write(repo.root.join("old name.txt"), &original).unwrap();
        git(&repo.root, &["add", "old name.txt"]);
        git(&repo.root, &["commit", "-m", "add file"]);
        git(&repo.root, &["mv", "old name.txt", "new name.txt"]);
        fs::write(repo.root.join("new name.txt"), format!("{original}added line\n")).unwrap();
        git(&repo.root, &["add", "new name.txt"]);
        let snapshot = status(&repo.root, None).unwrap();
        let entry = snapshot.entries.iter().find(|entry| entry.path == "new name.txt").unwrap();
        assert_eq!(entry.original_path.as_deref(), Some("old name.txt"));
        assert_eq!((entry.additions, entry.deletions), (Some(1), Some(0)));
    }

    #[cfg(windows)]
    #[test]
    fn untracked_diff_does_not_follow_junctions_outside_the_repo_or_into_metadata() {
        let repo = TestRepo::new("untracked-junctions");
        let outside = repo.root.with_extension("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("external.txt"), "external data").unwrap();
        let external_link = repo.root.join("linked-external");
        let metadata_link = repo.root.join("linked-metadata");
        junction::create(&outside, &external_link).unwrap();
        junction::create(repo.root.join(".git"), &metadata_link).unwrap();
        let external_result = diff(&repo.root, Some("linked-external/external.txt"), "untracked", None);
        let metadata_result = diff(&repo.root, Some("linked-metadata/config"), "untracked", None);
        junction::delete(&external_link).unwrap();
        junction::delete(&metadata_link).unwrap();
        fs::remove_dir_all(outside).unwrap();
        assert!(external_result.is_err());
        assert!(metadata_result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn untracked_diff_does_not_follow_symlinks_outside_the_repo_or_into_metadata() {
        let repo = TestRepo::new("untracked-links");
        let outside = repo.root.with_extension("outside.txt");
        fs::write(&outside, "external data").unwrap();
        std::os::unix::fs::symlink(&outside, repo.root.join("external.txt")).unwrap();
        std::os::unix::fs::symlink(repo.root.join(".git/config"), repo.root.join("metadata.txt")).unwrap();
        assert!(diff(&repo.root, Some("external.txt"), "untracked", None).is_err());
        assert!(diff(&repo.root, Some("metadata.txt"), "untracked", None).is_err());
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn branch_prefix_is_normalized_and_validated() {
        assert_eq!(normalize_branch_prefix(" codex ").unwrap(), "codex/");
        assert_eq!(normalize_branch_prefix("").unwrap(), "");
        assert!(normalize_branch_prefix("bad..prefix").is_err());
    }

    #[test]
    fn porcelain_parser_preserves_spaces_and_rename_origins() {
        let repo = TestRepo::new("rename");
        fs::write(repo.root.join("with space.txt"), "before\n").unwrap();
        git(&repo.root, &["add", "with space.txt"]);
        git(&repo.root, &["commit", "-m", "add spaced file"]);
        git(&repo.root, &["mv", "with space.txt", "renamed space.txt"]);
        let snapshot = status(&repo.root, None).unwrap();
        let renamed = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == "renamed space.txt")
            .unwrap();
        assert_eq!(renamed.original_path.as_deref(), Some("with space.txt"));
        assert!(renamed.staged);
    }

    #[test]
    fn publish_and_push_update_real_upstream_state() {
        let repo = TestRepo::new("remote");
        let remote_repo = repo.root.with_extension("remote.git");
        git(
            remote_repo.parent().unwrap(),
            &["init", "--bare", remote_repo.to_string_lossy().as_ref()],
        );
        git(
            &repo.root,
            &[
                "remote",
                "add",
                "origin",
                remote_repo.to_string_lossy().as_ref(),
            ],
        );

        let published = remote(&repo.root, "publish").unwrap();
        assert!(published.message.contains("已发布"));
        let clean = status(&repo.root, Some("branch_upstream")).unwrap();
        assert!(clean.upstream.is_some());
        assert_eq!(clean.ahead, 0);

        fs::write(repo.root.join("tracked.txt"), "one\ntwo\n").unwrap();
        stage(&repo.root, &["tracked.txt".into()]).unwrap();
        commit(&repo.root, "feat: add second line", false).unwrap();
        let ahead = status(&repo.root, Some("branch_upstream")).unwrap();
        assert_eq!(ahead.ahead, 1);
        assert!(ahead
            .committed_entries
            .iter()
            .any(|entry| entry.path == "tracked.txt"));
        let pushed = remote(&repo.root, "push").unwrap();
        assert_eq!(pushed.status.ahead, 0);
        let _ = fs::remove_dir_all(remote_repo);
    }

    #[test]
    fn commit_and_push_reports_a_partial_success_without_hiding_the_commit() {
        let repo = TestRepo::new("partial-push");
        let remote_repo = repo.root.with_extension("remote.git");
        git(
            remote_repo.parent().unwrap(),
            &["init", "--bare", remote_repo.to_string_lossy().as_ref()],
        );
        git(
            &repo.root,
            &[
                "remote",
                "add",
                "origin",
                remote_repo.to_string_lossy().as_ref(),
            ],
        );
        remote(&repo.root, "publish").unwrap();
        fs::remove_dir_all(&remote_repo).unwrap();
        fs::write(repo.root.join("tracked.txt"), "changed\n").unwrap();
        stage(&repo.root, &["tracked.txt".into()]).unwrap();

        let receipt = commit(&repo.root, "fix: preserve partial commit", true).unwrap();
        assert!(receipt.warning);
        assert!(receipt.message.contains("提交成功，但推送失败"));
        assert!(receipt.commit.is_some());
        assert_eq!(receipt.status.ahead, 1);
    }

    #[test]
    fn branches_and_merge_support_fast_forward_and_no_ff() {
        let repo = TestRepo::new("merge-modes");
        let main = git(&repo.root, &["branch", "--show-current"]);
        git(&repo.root, &["checkout", "-b", "feature/merge-source"]);
        fs::write(repo.root.join("feature.txt"), "feature\n").unwrap();
        git(&repo.root, &["add", "feature.txt"]);
        git(&repo.root, &["commit", "-m", "feat: add merge source"]);
        git(&repo.root, &["checkout", &main]);

        let available = branches(&repo.root).unwrap();
        assert!(available
            .iter()
            .any(|branch| branch.name == "feature/merge-source"));
        let receipt = merge(&repo.root, "feature/merge-source", false).unwrap();
        assert!(!receipt.warning);
        assert!(!receipt.status.merge_in_progress);
        assert!(repo.root.join("feature.txt").exists());

        git(&repo.root, &["checkout", "-b", "feature/no-ff"]);
        fs::write(repo.root.join("no-ff.txt"), "no-ff\n").unwrap();
        git(&repo.root, &["add", "no-ff.txt"]);
        git(&repo.root, &["commit", "-m", "feat: add no-ff source"]);
        git(&repo.root, &["checkout", &main]);
        let receipt = merge(&repo.root, "feature/no-ff", true).unwrap();
        assert!(!receipt.warning);
        let parents = git(&repo.root, &["rev-list", "--parents", "-n", "1", "HEAD"]);
        assert_eq!(parents.split_whitespace().count(), 3);
    }

    #[test]
    fn merge_rejects_a_dirty_worktree() {
        let repo = TestRepo::new("merge-dirty");
        let main = git(&repo.root, &["branch", "--show-current"]);
        git(&repo.root, &["checkout", "-b", "feature/clean-source"]);
        fs::write(repo.root.join("source.txt"), "source\n").unwrap();
        git(&repo.root, &["add", "source.txt"]);
        git(&repo.root, &["commit", "-m", "feat: source"]);
        git(&repo.root, &["checkout", &main]);
        fs::write(repo.root.join("tracked.txt"), "dirty\n").unwrap();

        let error = merge(&repo.root, "feature/clean-source", false).unwrap_err();
        assert!(error.to_string().contains("未提交变更"));
    }

    #[test]
    fn conflicted_merge_can_be_aborted() {
        let repo = TestRepo::new("merge-conflict");
        let main = git(&repo.root, &["branch", "--show-current"]);
        git(&repo.root, &["checkout", "-b", "feature/conflict"]);
        fs::write(repo.root.join("tracked.txt"), "feature\n").unwrap();
        git(&repo.root, &["add", "tracked.txt"]);
        git(&repo.root, &["commit", "-m", "feat: conflicting source"]);

        git(&repo.root, &["checkout", &main]);
        fs::write(repo.root.join("tracked.txt"), "main\n").unwrap();
        git(&repo.root, &["add", "tracked.txt"]);
        git(&repo.root, &["commit", "-m", "fix: conflicting target"]);

        let receipt = merge(&repo.root, "feature/conflict", false).unwrap();
        assert!(receipt.warning);
        assert!(receipt.status.merge_in_progress);
        assert!(receipt.status.entries.iter().any(|entry| entry.conflicted));

        let aborted = merge_abort(&repo.root).unwrap();
        assert!(!aborted.status.merge_in_progress);
        assert!(!aborted.status.entries.iter().any(|entry| entry.conflicted));
        assert_eq!(
            fs::read_to_string(repo.root.join("tracked.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "main\n"
        );
    }
}

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{anyhow, ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// 会话级独立 Git Worktree 元数据，随 SessionRecord 持久化。
///
/// 每个 Agent 任务使用独立的 checkout。多会话并行改同一个
/// 仓库时，每个会话落在自己的 worktree（独立分支）里，互不踩踏。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWorktree {
    /// 原仓库根目录（git rev-parse --show-toplevel）。
    pub repo_root: String,
    /// Worktree 检出路径（配置目录 worktrees/sessions/<session-id>）。
    pub path: String,
    /// 会话临时分支（crewvela/session-<id 前 8 位>）。
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<WorktreeBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeBinding {
    pub managed_root: String,
    pub repository_identity: String,
    pub source_cwd: String,
    pub relative_cwd: PathBuf,
    pub base_ref: String,
    pub base_commit: String,
    pub token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeOptions {
    pub base_ref: String,
    #[serde(default)]
    pub fetch_upstream: bool,
}

impl Default for WorktreeOptions {
    fn default() -> Self {
        Self {
            base_ref: "HEAD".into(),
            fetch_upstream: false,
        }
    }
}

impl SessionWorktree {
    pub fn cwd(&self) -> PathBuf {
        let mut cwd = PathBuf::from(&self.path);
        if let Some(binding) = &self.binding {
            if !binding.relative_cwd.as_os_str().is_empty() {
                cwd.push(&binding.relative_cwd);
            }
        }
        cwd
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RootSettings {
    root: Option<PathBuf>,
    #[serde(default)]
    previous_roots: Vec<PathBuf>,
}

const OWNER_FILE: &str = "crewvela-owner.json";
static ROOT_SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn default_root() -> Result<PathBuf> {
    let config = crate::providers::config_dir()?;
    let config = if config.is_dir() {
        physical(&config)?
    } else {
        config
    };
    Ok(config.join("worktrees").join("sessions"))
}

fn settings_path() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("worktree-roots.json"))
}

fn roots() -> Result<RootSettings> {
    let path = settings_path()?;
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("读取工作树根目录配置失败"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RootSettings::default()),
        Err(error) => Err(error.into()),
    }
}

fn physical(path: &Path) -> Result<PathBuf> {
    Ok(crate::multi_agent::external_command_path(
        &fs::canonicalize(path)
            .with_context(|| format!("目录或文件不可访问：{}", path.display()))?,
    ))
}

pub fn active_root() -> Result<PathBuf> {
    Ok(roots()?.root.unwrap_or(default_root()?))
}

pub fn set_root(value: Option<&Path>) -> Result<PathBuf> {
    let _guard = ROOT_SETTINGS_LOCK
        .lock()
        .map_err(|_| anyhow!("工作树设置锁损坏"))?;
    let default = default_root()?;
    let root = value
        .map(|path| -> Result<PathBuf> {
            ensure!(
                path.is_absolute() && path.is_dir(),
                "请选择已存在的绝对目录"
            );
            let path = physical(path)?;
            ensure!(path.parent().is_some(), "工作树根目录不能是磁盘根目录");
            // Formal collaboration worktrees live directly under config/worktrees/<project-id>.
            let config = physical(&crate::providers::config_dir()?)?;
            let reserved = config.join("worktrees");
            ensure!(
                !reserved.starts_with(&path)
                    && (!path.starts_with(&reserved) || path == reserved.join("sessions")),
                "该目录由协作任务使用"
            );
            ensure!(
                !path.components().any(|part| part
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(".git")),
                "不能使用 Git 元数据目录"
            );
            Ok(path)
        })
        .transpose()?;
    let mut settings = roots()?;
    if let Some(previous) = settings.root.take() {
        if !settings.previous_roots.contains(&previous) {
            settings.previous_roots.push(previous);
        }
    }
    settings.root = root;
    crate::providers::write_atomic(&settings_path()?, &serde_json::to_vec_pretty(&settings)?)?;
    Ok(settings.root.unwrap_or(default))
}

pub(crate) fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
    ]);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_NAMESPACE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(key);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：GUI 进程 spawn 控制台 git 时不闪控制台窗口。
        command.creation_flags(0x08000000);
    }
    command
}

fn git_output(dir: &Path, args: &[&str]) -> Result<Output> {
    git_command(dir)
        .args(args)
        .output()
        .with_context(|| format!("无法启动 Git：git {}", args.join(" ")))
}

fn git_stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// 目录是否位于某个 Git 仓库工作区内。
pub fn is_git_repo(dir: &Path) -> bool {
    git_output(dir, &["rev-parse", "--is-inside-work-tree"])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn repo_root(dir: &Path) -> Result<PathBuf> {
    let output = git_output(dir, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err(anyhow!("{} 不在任何 Git 仓库内", dir.display()));
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        return Err(anyhow!("无法确定 {} 的仓库根目录", dir.display()));
    }
    Ok(PathBuf::from(root))
}

fn git_text(dir: &Path, args: &[&str]) -> Result<String> {
    let output = git_output(dir, args)?;
    ensure!(
        output.status.success(),
        "Git {}：{}",
        args[0],
        git_stderr(&output)
    );
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches(['\r', '\n'])
        .to_string())
}

pub fn branches(dir: &Path) -> Result<Vec<String>> {
    let root = repo_root(dir)?;
    Ok(git_text(
        &root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/",
            "refs/remotes/",
        ],
    )?
    .lines()
    .filter(|s| !s.ends_with("/HEAD"))
    .map(str::to_owned)
    .collect())
}

pub fn create_session_worktree(
    dir: &Path,
    session_id: &str,
    options: &WorktreeOptions,
) -> Result<SessionWorktree> {
    let root = active_root()?;
    create_in(dir, session_id, &root, options)
}

#[cfg(test)]
static FIXTURE_ROOTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
    std::sync::LazyLock::new(Default::default);

#[cfg(test)]
pub(crate) fn create_session_worktree_in(
    dir: &Path,
    session_id: &str,
    managed: &Path,
) -> Result<SessionWorktree> {
    let tree = create_in(dir, session_id, managed, &WorktreeOptions::default())?;
    FIXTURE_ROOTS.lock().unwrap().insert(physical(managed)?);
    Ok(tree)
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn create_in(
    dir: &Path,
    session_id: &str,
    managed: &Path,
    options: &WorktreeOptions,
) -> Result<SessionWorktree> {
    ensure!(safe_id(session_id), "工作树会话标识无效");
    let source = physical(dir)?;
    let repo = physical(&repo_root(&source)?)?;
    let relative = source.strip_prefix(&repo)?.to_path_buf();
    let base = options.base_ref.as_str();
    ensure!(
        base == "HEAD" || branches(&repo)?.iter().any(|branch| branch == base),
        "基准分支不存在，请刷新后重新选择"
    );
    if options.fetch_upstream {
        crate::worktree_lifecycle::fetch_worktree_base(&repo, base)?;
    }
    let commit = git_text(
        &repo,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )?;
    ensure!(managed.is_absolute(), "托管根目录必须是绝对路径");
    let ancestor = managed
        .ancestors()
        .find(|path| path.exists())
        .context("无法确认托管根目录位置")?;
    ensure!(physical(ancestor)? == ancestor, "托管根目录发生重定向");
    ensure!(!managed.starts_with(&repo), "托管根目录必须位于原仓库之外");
    fs::create_dir_all(managed)?;
    ensure!(physical(managed)? == managed, "托管根目录发生重定向");
    let path = managed.join(session_id);
    ensure!(
        !path.try_exists()? && fs::symlink_metadata(&path).is_err(),
        "同名工作树目录已存在"
    );
    let short = session_id.chars().take(8).collect::<String>();
    let branch = crate::git_source_control::managed_branch_name("session", &short);
    let worktree = SessionWorktree {
        repo_root: repo.display().to_string(),
        path: path.display().to_string(),
        branch,
        binding: Some(WorktreeBinding {
            managed_root: managed.display().to_string(),
            repository_identity: crate::worktree_lifecycle::root_identity(&repo)?,
            source_cwd: source.display().to_string(),
            relative_cwd: relative,
            base_ref: base.into(),
            base_commit: commit.clone(),
            token: uuid::Uuid::new_v4().to_string(),
        }),
    };
    let output = git_output(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            &worktree.branch,
            "--",
            &worktree.path,
            &commit,
        ],
    )?;
    ensure!(
        output.status.success(),
        "创建会话工作树失败：{}",
        git_stderr(&output)
    );
    if let Err(error) = (|| -> Result<()> {
        ensure!(
            worktree.cwd().is_dir() && physical(&worktree.cwd())? == worktree.cwd(),
            "所选基准分支缺少原工作子目录，或该目录已被重定向"
        );
        write_owner(&worktree, session_id)
    })() {
        // Only remove the checkout we just created, without forcing away hook-created files.
        let rollback = (|| -> Result<()> {
            ensure!(!worktree_has_changes(&path)?, "新工作树已有文件，已保留");
            git_text(&repo, &["worktree", "remove", "--", &worktree.path])?;
            Ok(())
        })();
        return match rollback {
            Ok(()) => Err(error),
            Err(other) => Err(anyhow!(
                "{error}；回滚失败，保留 {}：{other}",
                worktree.path
            )),
        };
    }
    Ok(worktree)
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
struct Owner {
    session_id: String,
    token: String,
}

fn admin_directory(worktree: &SessionWorktree) -> Result<PathBuf> {
    let tree = Path::new(&worktree.path);
    let repo = Path::new(&worktree.repo_root);
    let common = |cwd: &Path| -> Result<PathBuf> {
        physical(Path::new(&git_text(
            cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?))
    };
    let repo_common = common(repo)?;
    let admin = physical(Path::new(&git_text(
        tree,
        &["rev-parse", "--absolute-git-dir"],
    )?))?;
    ensure!(
        physical(&repo_root(tree)?)? == physical(tree)?
            && common(tree)? == repo_common
            && admin.parent() == Some(repo_common.join("worktrees").as_path()),
        "工作树的实际 Git 仓库或管理目录与登记不一致"
    );
    let backlink = fs::read_to_string(admin.join("gitdir"))?;
    ensure!(
        physical(Path::new(backlink.trim_end_matches(['\r', '\n'])))?
            == physical(&tree.join(".git"))?,
        "Git 工作树反向登记不匹配"
    );
    Ok(admin)
}

fn owner(worktree: &SessionWorktree, session_id: &str) -> Result<Owner> {
    let binding = worktree
        .binding
        .as_ref()
        .context("旧工作树没有独立所有权标记")?;
    Ok(Owner {
        session_id: session_id.into(),
        token: binding.token.clone(),
    })
}

fn write_owner(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    let path = admin_directory(worktree)?.join(OWNER_FILE);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(&serde_json::to_vec(&owner(worktree, session_id)?)?)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn bind_restored_checkout(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    validate_location(worktree, session_id)?;
    let locked = verify_registration(worktree, true, true)?;
    ensure!(!locked, "工作树已锁定");
    if worktree.binding.is_some() {
        write_owner(worktree, session_id)?;
    }
    verify_managed_worktree(worktree, session_id)
}

fn validate_location_at(
    worktree: &SessionWorktree,
    session_id: &str,
    managed: &Path,
) -> Result<()> {
    ensure!(safe_id(session_id), "工作树会话标识无效");
    let path = Path::new(&worktree.path);
    ensure!(
        managed.is_absolute() && path == managed.join(session_id),
        "工作树路径与登记的会话或根目录不一致"
    );
    let real_managed = physical(managed)?;
    ensure!(real_managed == managed, "托管根目录发生重定向");
    ensure!(
        worktree.branch.ends_with(&format!(
            "session-{}",
            session_id.chars().take(8).collect::<String>()
        )),
        "分支不属于此会话"
    );
    if let Some(binding) = &worktree.binding {
        ensure!(
            crate::worktree_lifecycle::root_identity(Path::new(&worktree.repo_root))?
                == binding.repository_identity,
            "原 Git 仓库的物理身份已改变"
        );
        ensure!(
            binding
                .relative_cwd
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "工作树子目录无效"
        );
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink() && physical(path)? == path,
            "工作树目录发生重定向"
        );
    } else if path.try_exists()? {
        anyhow::bail!("无法核对工作树目录");
    }
    Ok(())
}

pub(crate) fn validate_location(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    let default = default_root()?;
    let managed = match &worktree.binding {
        Some(binding) => {
            let path = PathBuf::from(&binding.managed_root);
            #[cfg(test)]
            if FIXTURE_ROOTS.lock().unwrap().contains(&path) {
                return validate_location_at(worktree, session_id, &path);
            }
            let settings = roots()?;
            let trusted = settings.root.as_ref() == Some(&path)
                || settings.previous_roots.contains(&path)
                || default.is_dir() && physical(&default)? == path;
            ensure!(trusted, "工作树不在登记过的托管根目录中");
            path
        }
        None => physical(&default)?,
    };
    validate_location_at(worktree, session_id, &managed)
}

/// Use Git's NUL format so quoted, non-ASCII and whitespace-containing paths retain their identity.
fn verify_registration(
    worktree: &SessionWorktree,
    exists: bool,
    require_original_branch: bool,
) -> Result<bool> {
    let path = Path::new(&worktree.path);
    let repo = Path::new(&worktree.repo_root);
    ensure!(
        physical(&repo_root(repo)?)? == physical(repo)?,
        "原仓库根目录不匹配"
    );
    let output = git_output(repo, &["worktree", "list", "--porcelain", "-z"])?;
    ensure!(
        output.status.success(),
        "读取 Git 工作树列表失败：{}",
        git_stderr(&output)
    );
    let listed = String::from_utf8(output.stdout)?;
    let expected_branch = format!("branch refs/heads/{}", worktree.branch);
    for entry in listed.split("\0\0") {
        let mut fields = entry.split('\0');
        let Some(listed) = fields.next().and_then(|f| f.strip_prefix("worktree ")) else {
            continue;
        };
        let listed = Path::new(listed);
        if listed != path && (!exists || physical(listed).ok().as_deref() != Some(path)) {
            continue;
        }
        let fields: Vec<_> = fields.collect();
        ensure!(
            !require_original_branch || fields.contains(&expected_branch.as_str()),
            "工作树分支已经变化或 HEAD 已分离"
        );
        if exists {
            admin_directory(worktree)?;
        }
        return Ok(fields
            .iter()
            .any(|f| *f == "locked" || f.starts_with("locked ")));
    }
    anyhow::bail!("Git 未登记此工作树")
}

fn check_owner(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    if worktree.binding.is_some() {
        let stored: Owner = serde_json::from_slice(
            &fs::read(admin_directory(worktree)?.join(OWNER_FILE))
                .context("缺少 Git 工作树的 CrewVela 所有权标记")?,
        )?;
        ensure!(
            stored == owner(worktree, session_id)?,
            "工作树已不属于原会话"
        );
    }
    Ok(())
}

pub fn verify_managed_worktree(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    validate_location(worktree, session_id)?;
    verify_registration(worktree, true, true)?;
    check_owner(worktree, session_id)
}

pub fn verify_session_cwd(record: &crate::sessions::SessionRecord) -> Result<()> {
    if let Some(tree) = &record.worktree {
        validate_location(tree, &record.id)?;
        verify_registration(tree, true, false)?;
        check_owner(tree, &record.id)?;
        let expected = tree.cwd();
        ensure!(
            expected.is_dir()
                && physical(&expected)? == expected
                && physical(Path::new(&record.cwd))? == expected,
            "会话工作目录与登记的工作树不一致，请先恢复原工作树"
        );
    }
    Ok(())
}

pub fn worktree_has_changes(path: &Path) -> Result<bool> {
    let output = git_output(
        path,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored=matching",
            "--ignore-submodules=none",
        ],
    )?;
    ensure!(
        output.status.success(),
        "读取工作树状态失败：{}",
        git_stderr(&output)
    );
    Ok(!output.stdout.is_empty())
}

pub fn ensure_removable(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    validate_location(worktree, session_id)?;
    ensure!(
        !verify_registration(worktree, true, true)?,
        "工作树已被 Git 锁定"
    );
    check_owner(worktree, session_id)?;
    ensure!(
        !worktree_has_changes(Path::new(&worktree.path))?,
        "工作树存在未提交改动、未跟踪或被忽略的文件：{}",
        worktree.path
    );
    Ok(())
}

pub fn remove_checkout(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    remove_at_commit(worktree, session_id, None)
}

pub(crate) fn remove_at_commit(
    worktree: &SessionWorktree,
    session_id: &str,
    expected: Option<&str>,
) -> Result<()> {
    ensure_removable(worktree, session_id)?;
    if let Some(expected) = expected {
        ensure!(
            git_text(
                Path::new(&worktree.path),
                &["rev-parse", "--verify", "HEAD^{commit}"]
            )? == expected,
            "工作树提交在快照保存后发生变化，已保留"
        );
    }
    git_text(
        Path::new(&worktree.repo_root),
        &["worktree", "remove", "--", &worktree.path],
    )?;
    Ok(())
}

// Removing a missing checkout never prunes unrelated Git worktrees.
pub fn forget_missing_checkout(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    validate_location(worktree, session_id)?;
    ensure!(
        fs::symlink_metadata(&worktree.path).is_err() && !Path::new(&worktree.path).try_exists()?,
        "工作树目录仍然存在"
    );
    let repo = Path::new(&worktree.repo_root);
    let entries = git_text(repo, &["worktree", "list", "--porcelain", "-z"])?;
    if entries
        .split('\0')
        .filter_map(|field| field.strip_prefix("worktree "))
        .any(|path| Path::new(path) == Path::new(&worktree.path))
    {
        ensure!(!verify_registration(worktree, false, true)?, "工作树已锁定");
        git_text(repo, &["worktree", "remove", "--", &worktree.path])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = git_output(dir, args).expect("git 应可执行");
        assert!(
            output.status.success(),
            "git {} 失败: {}",
            args.join(" "),
            git_stderr(&output)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    struct TestRepo {
        root: PathBuf,
        worktrees_root: PathBuf,
    }

    impl TestRepo {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir()
                .join(format!("crewvela-worktree-{name}-{}", uuid::Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init"]);
            fs::write(root.join("README.md"), "init").unwrap();
            git(&root, &["add", "README.md"]);
            git(
                &root,
                &[
                    "-c",
                    "user.name=CrewVela Test",
                    "-c",
                    "user.email=crewvela@test.local",
                    "commit",
                    "-m",
                    "init",
                ],
            );
            Self {
                worktrees_root: base.join("worktrees"),
                root,
            }
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let base = self.root.parent().unwrap().to_path_buf();
            // worktree remove 后目录已空；残留一律强清，避免 Windows 句柄占用导致失败。
            let _ = fs::remove_dir_all(base);
        }
    }

    #[test]
    fn detects_git_repo() {
        let repo = TestRepo::new("detect");
        assert!(is_git_repo(&repo.root));
        assert!(!is_git_repo(&repo.worktrees_root));
    }

    #[test]
    fn creates_worktree_on_current_head() {
        let repo = TestRepo::new("create");
        let head = git(&repo.root, &["rev-parse", "HEAD"]);

        let worktree =
            create_session_worktree_in(&repo.root, "abcdef12-3456-7890", &repo.worktrees_root)
                .unwrap();

        assert!(Path::new(&worktree.path).join("README.md").exists());
        assert_eq!(worktree.branch, "crewvela/session-abcdef12");
        assert_eq!(
            git(Path::new(&worktree.path), &["rev-parse", "HEAD"]),
            head,
            "worktree 应检出仓库当前 HEAD"
        );
        assert!(is_git_repo(Path::new(&worktree.path)));

        remove_checkout(&worktree, "abcdef12-3456-7890").unwrap();
        assert!(!Path::new(&worktree.path).exists());
    }

    #[test]
    fn cleanup_rejects_worktree_with_uncommitted_changes() {
        let repo = TestRepo::new("dirty");
        let worktree =
            create_session_worktree_in(&repo.root, "session-dirty123456", &repo.worktrees_root)
                .unwrap();
        fs::write(
            Path::new(&worktree.path).join("new-file.rs"),
            "fn main() {}",
        )
        .unwrap();

        let error = remove_checkout(&worktree, "session-dirty123456").unwrap_err();

        assert!(error.to_string().contains("未提交改动"));
        assert!(Path::new(&worktree.path).exists(), "脏 worktree 必须保留");

        fs::remove_file(Path::new(&worktree.path).join("new-file.rs")).unwrap();
        remove_checkout(&worktree, "session-dirty123456").unwrap();
        assert!(!Path::new(&worktree.path).exists());
    }

    #[test]
    fn cleanup_tolerates_manually_deleted_worktree() {
        let repo = TestRepo::new("prune");
        let worktree =
            create_session_worktree_in(&repo.root, "session-prune123456", &repo.worktrees_root)
                .unwrap();
        fs::remove_dir_all(&worktree.path).unwrap();

        forget_missing_checkout(&worktree, "session-prune123456").unwrap();

        let listed = git(&repo.root, &["worktree", "list", "--porcelain"]);
        assert!(
            !listed.contains("session-prune123456"),
            "prune 后不应再登记该 worktree: {listed}"
        );
    }

    #[test]
    fn rejects_non_repo_directory() {
        let dir = std::env::temp_dir().join(format!("crewvela-not-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        let error = create_session_worktree_in(&dir, "session-x", &dir).unwrap_err();

        assert!(error.to_string().contains("Git 仓库"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn selected_branch_preserves_project_subdirectory_and_rejects_unsafe_ref() {
        let repo = TestRepo::new("subdirectory");
        let project = repo.root.join("子 项目");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("data.txt"), "base").unwrap();
        git(&repo.root, &["add", "."]);
        git(
            &repo.root,
            &[
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@localhost",
                "commit",
                "-m",
                "project",
            ],
        );
        git(&repo.root, &["branch", "worktree-base"]);
        fs::write(project.join("data.txt"), "later").unwrap();
        git(
            &repo.root,
            &[
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@localhost",
                "commit",
                "-am",
                "later",
            ],
        );
        let options = WorktreeOptions {
            base_ref: "refs/heads/worktree-base".into(),
            fetch_upstream: false,
        };
        let tree = create_in(&project, "selected", &repo.worktrees_root, &options).unwrap();
        FIXTURE_ROOTS
            .lock()
            .unwrap()
            .insert(physical(&repo.worktrees_root).unwrap());
        assert_eq!(
            fs::read_to_string(tree.cwd().join("data.txt")).unwrap(),
            "base"
        );
        assert_eq!(
            fs::read_to_string(project.join("data.txt")).unwrap(),
            "later"
        );
        assert_eq!(
            tree.binding.as_ref().unwrap().base_commit,
            git(&repo.root, &["rev-parse", "worktree-base"])
        );
        assert!(create_in(&project, "../outside", &repo.worktrees_root, &options).is_err());
        assert!(create_in(
            &project,
            "invalid",
            &repo.worktrees_root,
            &WorktreeOptions {
                base_ref: "--all".into(),
                ..options
            }
        )
        .is_err());
        remove_checkout(&tree, "selected").unwrap();
    }

    #[test]
    fn git_lock_foreign_repo_changed_owner_and_ignored_files_block_removal() {
        let repo = TestRepo::new("ownership");
        let other = TestRepo::new("foreign");
        let tree =
            create_session_worktree_in(&repo.root, "ownership-id", &repo.worktrees_root).unwrap();
        let path = Path::new(&tree.path);
        let mut foreign = tree.clone();
        foreign.repo_root = other.root.display().to_string();
        assert!(remove_checkout(&foreign, "ownership-id").is_err());
        foreign = tree.clone();
        foreign.path = repo.root.display().to_string();
        assert!(remove_checkout(&foreign, "ownership-id").is_err());
        assert!(remove_checkout(&tree, "wrong-session").is_err());
        git(&repo.root, &["worktree", "lock", "--", &tree.path]);
        assert!(remove_checkout(&tree, "ownership-id")
            .unwrap_err()
            .to_string()
            .contains("锁定"));
        git(&repo.root, &["worktree", "unlock", "--", &tree.path]);
        fs::write(repo.root.join(".git/info/exclude"), "ignored.txt\n").unwrap();
        fs::write(path.join("ignored.txt"), "private").unwrap();
        assert!(remove_checkout(&tree, "ownership-id").is_err());
        fs::remove_file(path.join("ignored.txt")).unwrap();
        let marker = admin_directory(&tree).unwrap().join(OWNER_FILE);
        let original = fs::read(&marker).unwrap();
        fs::write(&marker, b"{}").unwrap();
        assert!(remove_checkout(&tree, "ownership-id").is_err());
        fs::write(&marker, original).unwrap();
        git(path, &["switch", "--detach"]);
        assert!(remove_checkout(&tree, "ownership-id").is_err());
        git(path, &["switch", &tree.branch]);
        remove_checkout(&tree, "ownership-id").unwrap();
        assert!(git(&repo.root, &["branch", "--list", &tree.branch]).contains(&tree.branch));
    }
}

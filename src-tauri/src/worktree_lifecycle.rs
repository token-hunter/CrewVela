use crate::providers::{config_dir, write_atomic};
use crate::worktree::{verify_managed_worktree, SessionWorktree};
use anyhow::{anyhow, bail, ensure, Context, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LifecyclePolicy {
    pub command: String,
    pub files: Vec<String>,
    pub auto_cleanup: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrustedPolicy {
    repository: String,
    policy: LifecyclePolicy,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparationLog {
    state: String,
    updated_at_ms: u64,
    lines: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoverySnapshot {
    pub id: String,
    pub session_id: String,
    pub worktree: SessionWorktree,
    pub commit: String,
    pub git_ref: String,
    pub created_at_ms: u64,
    repository: String,
    contents: String,
    checkout_parent: String,
    history: BTreeSet<String>,
    root_mode: u32,
    root_modified: Duration,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LifecycleView {
    policy: LifecyclePolicy,
    run: Option<PreparationLog>,
    snapshots: Vec<SnapshotView>,
    warning: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotView {
    id: String,
    session_id: String,
    branch: String,
    path: String,
    commit: String,
    git_ref: String,
    archive_path: String,
    created_at_ms: u64,
}

fn state_root() -> Result<PathBuf> {
    Ok(config_dir()?.join("worktree-lifecycle"))
}

fn key(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn project_dir(state: &Path, root: &Path) -> Result<PathBuf> {
    let canonical = root.canonicalize()?;
    let path = canonical.to_str().context("仓库路径不能转换为文本")?;
    #[cfg(windows)]
    let path = path.to_lowercase();
    Ok(state.join(key(path.as_ref())))
}

fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => {
            Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
                format!("读取配置失败：{}", path.display())
            })?))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().context("缺少保存目录")?)?;
    write_atomic(path, &serde_json::to_vec_pretty(value)?)
}

fn hidden(program: &str) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x08000000);
        command
    };
    command
}

fn git_command(repo: &Path) -> Command {
    let mut cmd = crate::worktree::git_command(repo);
    cmd.args(["-c", "core.quotePath=false"])
        .env("GCM_INTERACTIVE", "Never")
        .env("GIT_OPTIONAL_LOCKS", "0");
    cmd
}

fn checked_git(repo: &Path, args: &[&str], input: Option<Vec<u8>>) -> Result<Output> {
    let mut cmd = git_command(repo);
    cmd.args(args);
    let operation = format!("Git {}", args.first().unwrap_or(&""));
    let output = crate::title_summary::run_cli_with_timeout(
        cmd,
        input,
        Duration::from_secs(120),
        64 * 1024 * 1024,
        &operation,
    )?;
    ensure!(
        output.status.success(),
        "{operation} 失败：{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output)
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(checked_git(repo, args, None)?.stdout)?
        .trim_end_matches(['\r', '\n'])
        .to_string())
}

pub(crate) fn root_identity(repo: &Path) -> Result<String> {
    let reported = git_text(repo, &["rev-parse", "--show-toplevel"])?;
    ensure!(
        repo.canonicalize()? == Path::new(&reported).canonicalize()?,
        "请选择 Git 仓库根目录"
    );
    directory_identity(Path::new(&git_text(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?))
}

fn directory_identity(directory: &Path) -> Result<String> {
    let common = directory.canonicalize()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::metadata(&common)?;
        Ok(format!(
            "{}|{}|{}",
            common.display(),
            meta.dev(),
            meta.ino()
        ))
    }
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        };
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&common)?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(dir.as_raw_handle() as _, &mut info) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(format!(
            "{}|{}|{}|{}",
            common.to_string_lossy().to_lowercase(),
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow
        ))
    }
}

fn trusted_policy(project: &Path, repo: &Path) -> Result<LifecyclePolicy> {
    let Some(saved) = read_optional::<TrustedPolicy>(&project.join("policy.json"))? else {
        return Ok(LifecyclePolicy::default());
    };
    ensure!(
        saved.repository == root_identity(repo)?,
        "项目目录的 Git 仓库已更换，请重新保存环境配置"
    );
    validate_policy(&saved.policy)?;
    Ok(saved.policy)
}

pub(crate) fn auto_cleanup_enabled(repo: &Path) -> Result<bool> {
    let state = state_root()?;
    if !state.try_exists()? {
        return Ok(false);
    }
    Ok(trusted_policy(&project_dir(&state, repo)?, repo)?.auto_cleanup)
}

/// Fetch remote-tracking refs only. Resolve local branch tracking before falling back to the single/default remote.
pub(crate) fn fetch_worktree_base(repo: &Path, base: &str) -> Result<()> {
    let remotes = git_text(repo, &["remote"])?;
    let remotes: Vec<_> = remotes.lines().filter(|s| !s.is_empty()).collect();
    let base = if base.trim().is_empty() {
        "HEAD"
    } else {
        base.trim()
    };
    let tracking = base.strip_prefix("refs/remotes/").unwrap_or(base);
    let explicit = remotes
        .iter()
        .copied()
        .filter(|r| tracking.starts_with(&format!("{r}/")))
        .max_by_key(|r| r.len());
    let current_branch = git_text(repo, &["branch", "--show-current"])?;
    let branch = if base == "HEAD" {
        current_branch.as_str()
    } else {
        base.strip_prefix("refs/heads/").unwrap_or(base)
    };
    let candidates = git_text(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(upstream:remotename)",
            "refs/heads/",
        ],
    )?;
    let tracked = candidates
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(name, _)| *name == format!("refs/heads/{branch}"))
        .map(|(_, remote)| remote)
        .filter(|r| remotes.contains(r));
    let remote = explicit
        .or(tracked)
        .or_else(|| remotes.iter().copied().find(|r| *r == "origin"))
        .or_else(|| {
            if remotes.len() == 1 {
                Some(remotes[0])
            } else {
                None
            }
        })
        .context("找不到所选基准的远端，请选择远端分支或关闭获取上游")?;
    checked_git(
        repo,
        &[
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            "--no-auto-maintenance",
            "--",
            remote,
        ],
        None,
    )?;
    Ok(())
}

fn relative_file(value: &str) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for part in value.split(['/', '\\']) {
        let lower = part.to_ascii_lowercase();
        let device = lower.split('.').next().unwrap_or_default();
        ensure!(
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || ":*?\"<>|".contains(c))
                && lower != ".git"
                && !lower.starts_with("git~")
                && !matches!(device, "con" | "prn" | "aux" | "nul" | "conin$" | "conout$")
                && !(device.len() == 4
                    && (device.starts_with("com") || device.starts_with("lpt"))
                    && device.as_bytes()[3].is_ascii_digit()),
            "不支持的项目相对路径：{value}"
        );
        result.push(part);
    }
    ensure!(
        result
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "路径必须位于项目内：{value}"
    );
    Ok(result)
}

fn validate_policy(policy: &LifecyclePolicy) -> Result<()> {
    ensure!(
        policy.command.len() <= 16 * 1024 && !policy.command.contains('\0'),
        "准备命令过长或包含空字符"
    );
    ensure!(policy.files.len() <= 128, "所需文件最多可配置 128 项");
    let mut paths = BTreeSet::new();
    for name in &policy.files {
        let path = relative_file(name)?;
        ensure!(paths.insert(path), "文件路径重复：{name}");
    }
    Ok(())
}

fn ordinary(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !meta.file_type().is_symlink() && (meta.is_dir() || meta.is_file())
}

/// Walk each parent without following links/junctions; create only missing destination directories.
fn checked_file(root: &Path, relative: &Path, create_parents: bool) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let parent = checked_directory(
        &root,
        relative.parent().context("所需文件缺少相对父目录")?,
        create_parents,
    )?;
    let path = parent.join(relative.file_name().context("缺少文件名")?);
    match fs::symlink_metadata(&path) {
        Ok(meta) => ensure!(
            ordinary(&meta) && meta.is_file() && path.canonicalize()?.starts_with(&root),
            "拒绝链接或非普通文件：{}",
            path.display()
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(path)
}

fn checked_directory(root: &Path, relative: &Path, create: bool) -> Result<PathBuf> {
    let canonical = root.canonicalize()?;
    let mut path = canonical.clone();
    for part in relative.components() {
        ensure!(matches!(part, Component::Normal(_)), "文件路径越界");
        path.push(part);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&path)?;
                fs::symlink_metadata(&path)?
            }
            Err(e) => return Err(e.into()),
        };
        ensure!(
            ordinary(&meta) && meta.is_dir() && path.canonicalize()?.starts_with(&canonical),
            "拒绝穿过链接或异常目录：{}",
            path.display()
        );
    }
    Ok(path)
}

fn digest(mut input: impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    io::copy(&mut input, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_required(repo: &Path, checkout: &Path, name: &str) -> Result<()> {
    let relative = relative_file(name)?;
    let source = checked_file(repo, &relative, false)?;
    let mut input = File::open(&source).with_context(|| format!("读取项目所需文件 {name} 失败"))?;
    let target = checked_file(checkout, &relative, true)?;
    if let Ok(existing) = File::open(&target) {
        ensure!(
            digest(input)? == digest(existing)?,
            "目标已有内容不同的文件，已保留：{name}"
        );
        return Ok(());
    }
    let temp = target.with_file_name(format!(".crewvela-copy-{}", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let permissions = input.metadata()?.permissions();
    let mut output = options.open(&temp)?;
    let copied = (|| -> Result<()> {
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        drop(output);
        ensure!(
            checked_file(checkout, &relative, false)? == target,
            "目标目录已变化"
        );
        fs::hard_link(&temp, &target).context("发布所需文件失败，目标可能已被其他程序创建")?;
        Ok(())
    })();
    let removed = fs::remove_file(temp);
    copied?;
    removed?;
    fs::set_permissions(target, permissions)?;
    Ok(())
}

fn run_path(project: &Path, session_id: &str) -> PathBuf {
    project
        .join("preparation")
        .join(format!("{}.json", key(session_id)))
}

fn prepare(
    project: &Path,
    worktree: &SessionWorktree,
    session_id: &str,
    policy: &LifecyclePolicy,
) -> Result<()> {
    let path = run_path(project, session_id);
    if let Some(prior) = read_optional::<PreparationLog>(&path)? {
        ensure!(
            prior.state != "unverified" && prior.state != "running",
            "上次准备进程尚未确认停止，已保留工作树"
        );
    }
    if policy.command.trim().is_empty() && policy.files.is_empty() && !path.exists() {
        return Ok(());
    }
    let checkout = Path::new(&worktree.path);
    let mut log = PreparationLog {
        state: "running".into(),
        updated_at_ms: crate::automation::now_millis(),
        lines: Vec::new(),
    };
    save_json(&path, &log)?;
    let result = (|| -> Result<()> {
        for name in &policy.files {
            copy_required(Path::new(&worktree.repo_root), checkout, name)?;
            log.lines.push(format!("已核对文件 {name}"));
        }
        if !policy.command.trim().is_empty() {
            #[cfg(windows)]
            let mut cmd = {
                use std::os::windows::process::CommandExt;
                let mut c = hidden("cmd.exe");
                c.args(["/d", "/s", "/c"])
                    .raw_arg(format!("\"{}\"", policy.command));
                c
            };
            #[cfg(not(windows))]
            let mut cmd = {
                let mut c = hidden("/bin/sh");
                c.arg("-c").arg(&policy.command);
                c
            };
            cmd.current_dir(checkout);
            let output = crate::title_summary::run_cli_with_timeout(
                cmd,
                None,
                Duration::from_secs(600),
                2 * 1024 * 1024,
                "Worktree 准备命令",
            )?;
            for bytes in [&output.stdout, &output.stderr] {
                let tail = &bytes[bytes.len().saturating_sub(8192)..];
                let line = String::from_utf8_lossy(tail).trim().to_string();
                if !line.is_empty() {
                    log.lines.push(line);
                }
            }
            ensure!(output.status.success(), "准备命令退出：{}", output.status);
        }
        Ok(())
    })();
    match &result {
        Ok(()) => {
            log.state = "ready".into();
            log.lines.push("环境准备完成".into());
        }
        Err(e) => {
            log.state = if e.downcast_ref::<crate::acp::UnverifiedStop>().is_some() {
                "unverified"
            } else {
                "failed"
            }
            .into();
            log.lines.push(format!("{e:#}"));
        }
    }
    log.updated_at_ms = crate::automation::now_millis();
    save_json(&path, &log)?;
    result
}

/// The caller serializes creation/retry with the session lifecycle. An unconfigured project is a no-op.
pub(crate) fn prepare_if_configured(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    let state = state_root()?;
    if !state.try_exists()? {
        return Ok(());
    }
    let repo = Path::new(&worktree.repo_root);
    let project = project_dir(&state, repo)?;
    let policy = trusted_policy(&project, repo)?;
    if policy.command.trim().is_empty()
        && policy.files.is_empty()
        && !run_path(&project, session_id).exists()
    {
        return Ok(());
    }
    verify_managed_worktree(worktree, session_id)?;
    let admin = PathBuf::from(git_text(
        Path::new(&worktree.path),
        &["rev-parse", "--absolute-git-dir"],
    )?);
    ensure!(
        admin.join("locked").try_exists()? == false,
        "Worktree 已锁定"
    );
    prepare(&project, worktree, session_id, &policy)
}

fn ready(project: &Path, session_id: &str) -> Result<()> {
    if let Some(log) = read_optional::<PreparationLog>(&run_path(project, session_id))? {
        ensure!(
            log.state == "ready",
            "Worktree 环境准备尚未完成，请查看准备日志后重试"
        );
    }
    Ok(())
}

pub(crate) fn ensure_preparation_ready(worktree: &SessionWorktree, session_id: &str) -> Result<()> {
    let state = state_root()?;
    if !state.try_exists()? {
        return Ok(());
    }
    ready(
        &project_dir(&state, Path::new(&worktree.repo_root))?,
        session_id,
    )
}

fn snapshot_dir(project: &Path, id: &str) -> Result<PathBuf> {
    ensure!(Uuid::parse_str(id)?.to_string() == id, "恢复快照编号无效");
    Ok(project.join("snapshots").join(id))
}

const PORTABLE_ADMIN_FILES: &[&str] = &["ORIG_HEAD", "logs/HEAD", "COMMIT_EDITMSG", "FETCH_HEAD"];

fn valid_oid(oid: &str) -> bool {
    (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Only allow reconstructible administrative files. Merge/rebase, sparse indexes, worktree
/// refs/config, locks and any unknown Git extension preserve the original checkout.
fn clean_head(worktree: &SessionWorktree) -> Result<(String, PathBuf)> {
    let path = Path::new(&worktree.path);
    let status = checked_git(
        path,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--ignored=matching",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
        None,
    )?;
    ensure!(
        status.stdout.is_empty(),
        "Worktree 含有未提交、未跟踪或被忽略的文件，已保留"
    );
    let files = checked_git(path, &["ls-files", "-v", "--stage", "-z"], None)?;
    for item in files.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        ensure!(
            item.starts_with(b"H 100644 ") || item.starts_with(b"H 100755 "),
            "Worktree 含有子模块、链接或特殊索引标记，已保留"
        );
    }
    let admin = PathBuf::from(git_text(path, &["rev-parse", "--absolute-git-dir"])?);
    for entry in fs::read_dir(&admin)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("Git 管理目录包含未知文件名"))?;
        let meta = fs::symlink_metadata(entry.path())?;
        ensure!(ordinary(&meta), "Git 管理目录包含链接，已保留");
        match name.as_str() {
            "HEAD"
            | "index"
            | "gitdir"
            | "commondir"
            | "crewvela-owner.json"
            | "crewvela-restoring.json"
            | "ORIG_HEAD"
            | "COMMIT_EDITMSG"
            | "FETCH_HEAD"
                if meta.is_file() =>
            {
                ()
            }
            "refs" if meta.is_dir() => ensure!(
                fs::read_dir(entry.path())?.next().is_none(),
                "Worktree 含有本地引用，已保留"
            ),
            "logs" if meta.is_dir() => {
                for item in fs::read_dir(entry.path())? {
                    let item = item?;
                    let meta = fs::symlink_metadata(item.path())?;
                    ensure!(
                        item.file_name() == "HEAD" && ordinary(&meta) && meta.is_file(),
                        "Worktree 含有未知引用日志，已保留"
                    );
                }
            }
            _ => bail!("Worktree 含有 Git 状态文件 {name}，已保留"),
        }
    }
    let head = git_text(path, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    ensure!(valid_oid(&head), "未能验证 Worktree 提交");
    ensure!(
        git_text(path, &["symbolic-ref", "HEAD"])? == format!("refs/heads/{}", worktree.branch),
        "Worktree 当前分支已变化"
    );
    Ok((head, admin))
}

fn pinned_history(admin: &Path) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut add = |oid: &str| -> Result<()> {
        ensure!(valid_oid(oid), "无法解析 Git 工作树历史，已保留");
        if oid.bytes().any(|b| b != b'0') {
            ids.insert(oid.to_owned());
        }
        Ok(())
    };
    let log = admin.join("logs").join("HEAD");
    if log.try_exists()? {
        // Git author names can contain non-UTF8 bytes. Object names are ASCII.
        for line in fs::read(log)?
            .split(|b| *b == b'\n')
            .filter(|s| !s.is_empty())
        {
            let mut fields = line.split(|b| *b == b' ');
            for _ in 0..2 {
                add(std::str::from_utf8(
                    fields.next().context("Git 引用日志缺少提交")?,
                )?)?;
            }
        }
    }
    let orig = admin.join("ORIG_HEAD");
    if orig.try_exists()? {
        add(fs::read_to_string(orig)?.trim())?;
    }
    let fetch = admin.join("FETCH_HEAD");
    if fetch.try_exists()? {
        for line in fs::read(fetch)?
            .split(|b| *b == b'\n')
            .filter(|s| !s.is_empty())
        {
            add(std::str::from_utf8(
                line.split(|b| *b == b'\t')
                    .next()
                    .context("FETCH_HEAD 缺少提交")?,
            )?)?;
        }
    }
    Ok(ids)
}

fn mode(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o7777
    }
    #[cfg(windows)]
    {
        match (meta.is_dir(), meta.permissions().readonly()) {
            (true, false) => 0o755,
            (true, true) => 0o555,
            (false, false) => 0o644,
            (false, true) => 0o444,
        }
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(windows)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_readonly(mode & 0o200 == 0);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(unix)]
fn portable_metadata(path: &Path, meta: &fs::Metadata) -> Result<()> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    ensure!(
        !meta.is_file() || meta.nlink() == 1,
        "含有硬链接文件，已保留"
    );
    ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.gid() == unsafe { libc::getegid() },
        "文件有独立的所有者或组，已保留"
    );
    let name = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    let length =
        unsafe { libc::listxattr(name.as_ptr(), std::ptr::null_mut(), 0, libc::XATTR_NOFOLLOW) };
    #[cfg(not(target_os = "macos"))]
    let length = unsafe { libc::llistxattr(name.as_ptr(), std::ptr::null_mut(), 0) };
    if length < 0 {
        return Err(io::Error::last_os_error()).context("无法验证扩展文件属性，已保留");
    }
    ensure!(length == 0, "文件带有扩展属性或访问控制列表，已保留");
    Ok(())
}

/// A normal Git checkout on NTFS inherits its ACL and has one unnamed data stream.
/// Named streams, external hard links, explicit ACLs and filesystem features such as EFS
/// could be lost in a portable content archive, so preserve such checkouts in place.
#[cfg(windows)]
fn portable_metadata(path: &Path, meta: &fs::Metadata) -> Result<()> {
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt, io::AsRawHandle};
    use std::ptr::{addr_of_mut, null_mut};
    use windows_sys::Win32::{
        Foundation::{ERROR_HANDLE_EOF, INVALID_HANDLE_VALUE},
        Security::*,
        Storage::FileSystem::*,
    };
    let basic = FILE_ATTRIBUTE_ARCHIVE
        | FILE_ATTRIBUTE_DIRECTORY
        | FILE_ATTRIBUTE_NORMAL
        | FILE_ATTRIBUTE_READONLY;
    ensure!(
        meta.file_attributes() & !basic == 0,
        "文件带有特殊 Windows 属性，已保留：{}",
        path.display()
    );
    if meta.is_file() {
        let file = File::open(path)?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        ensure!(
            info.nNumberOfLinks == 1,
            "含有硬链接文件，已保留：{}",
            path.display()
        );
    }
    let name: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut stream: WIN32_FIND_STREAM_DATA = unsafe { std::mem::zeroed() };
    let handle = unsafe {
        FindFirstStreamW(
            name.as_ptr(),
            FindStreamInfoStandard,
            addr_of_mut!(stream).cast(),
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_HANDLE_EOF as i32) {
            return Err(error).context("无法验证 NTFS 数据流");
        }
    } else {
        let end = stream
            .cStreamName
            .iter()
            .position(|c| *c == 0)
            .context("无法解析 NTFS 数据流名称")?;
        let ordinary_stream = String::from_utf16_lossy(&stream.cStreamName[..end]) == "::$DATA";
        let next = unsafe { FindNextStreamW(handle, addr_of_mut!(stream).cast()) };
        let error = io::Error::last_os_error();
        unsafe {
            FindClose(handle);
        }
        ensure!(
            ordinary_stream && next == 0 && error.raw_os_error() == Some(ERROR_HANDLE_EOF as i32),
            "文件含额外数据流或无法验证，已保留：{}",
            path.display()
        );
    }
    let mut needed = 0;
    unsafe {
        GetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION,
            null_mut(),
            0,
            &mut needed,
        );
    }
    ensure!(needed > 0, "无法确认 Windows 文件权限，已保留");
    let mut descriptor = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    let ptr = descriptor.as_mut_ptr().cast();
    if unsafe {
        GetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION,
            ptr,
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    let (mut control, mut revision, mut present, mut defaulted) = (0, 0, 0, 0);
    let mut acl = null_mut();
    if unsafe { GetSecurityDescriptorControl(ptr, &mut control, &mut revision) } == 0
        || unsafe { GetSecurityDescriptorDacl(ptr, &mut present, &mut acl, &mut defaulted) } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    ensure!(
        present != 0 && !acl.is_null() && control & SE_DACL_PROTECTED == 0,
        "文件有独立 Windows 权限，已保留"
    );
    for i in 0..unsafe { (*acl).AceCount } {
        let mut ace = null_mut();
        if unsafe { GetAce(acl, u32::from(i), &mut ace) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        ensure!(
            !ace.is_null()
                && u32::from(unsafe { (*(ace as *const ACE_HEADER)).AceFlags }) & INHERITED_ACE
                    != 0,
            "文件有独立 Windows 权限，已保留"
        );
    }
    Ok(())
}

/// Store actual checkout bytes and empty directories. A clean Git status alone cannot reproduce
/// smudge filters, line endings, local modes or directory-only content.
fn visit_checkout(
    path: &Path,
    prefix: &Path,
    visitor: &mut impl FnMut(&Path, &Path, &fs::Metadata) -> Result<()>,
) -> Result<()> {
    let mut entries = fs::read_dir(path)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if prefix == Path::new("checkout") && entry.file_name() == ".git" {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("文件名无法无损记录，已保留"))?;
        ensure!(
            relative_file(&name)?.components().count() == 1,
            "不支持的文件名，已保留"
        );
        let relative = prefix.join(name);
        let source = entry.path();
        let meta = fs::symlink_metadata(&source)?;
        ensure!(
            ordinary(&meta),
            "链接或特殊文件无法无损保存：{}",
            source.display()
        );
        portable_metadata(&source, &meta)?;
        visitor(&source, &relative, &meta)?;
        if meta.is_dir() {
            visit_checkout(&source, &relative, visitor)?;
        }
    }
    Ok(())
}

fn visit_contents(
    worktree: &SessionWorktree,
    admin: &Path,
    mut visitor: impl FnMut(&Path, &Path, &fs::Metadata) -> Result<()>,
) -> Result<()> {
    visit_checkout(
        Path::new(&worktree.path),
        Path::new("checkout"),
        &mut visitor,
    )?;
    for name in PORTABLE_ADMIN_FILES {
        let source = admin.join(name);
        if source.try_exists()? {
            visitor(
                &source,
                &Path::new("admin").join(name),
                &fs::symlink_metadata(&source)?,
            )?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct EntryStamp(String, u32, Duration, Option<String>);

fn modified(meta: &fs::Metadata) -> Result<Duration> {
    Ok(meta.modified()?.duration_since(std::time::UNIX_EPOCH)?)
}

fn restore_metadata(path: &Path, mode: u32, modified: Duration) -> Result<()> {
    let mut options = OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
        };
        options
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    #[cfg(unix)]
    options.read(true);
    let timestamp = std::time::UNIX_EPOCH
        .checked_add(modified)
        .context("文件修改时间无效")?;
    options
        .open(path)?
        .set_times(fs::FileTimes::new().set_modified(timestamp))?;
    set_mode(path, mode)
}

fn stamp(path: &Path, mode: u32, modified: Duration, data: Option<String>) -> Result<EntryStamp> {
    Ok(EntryStamp(
        path.to_str()
            .context("快照路径不可编码")?
            .replace('\\', "/"),
        mode,
        modified,
        data,
    ))
}

fn stamps_digest(stamps: &[EntryStamp]) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(stamps)?)))
}

fn live_digest(worktree: &SessionWorktree, admin: &Path) -> Result<String> {
    let mut stamps = Vec::new();
    visit_contents(worktree, admin, |source, relative, meta| {
        let data = if meta.is_file() {
            Some(digest(File::open(source)?)?)
        } else {
            None
        };
        stamps.push(stamp(relative, mode(meta), modified(meta)?, data)?);
        Ok(())
    })?;
    stamps_digest(&stamps)
}

fn archive_checkout(worktree: &SessionWorktree, admin: &Path, destination: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut archive = tar::Builder::new(GzEncoder::new(
        options.open(destination)?,
        Compression::fast(),
    ));
    visit_contents(worktree, admin, |source, relative, meta| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(if meta.is_dir() {
            tar::EntryType::Directory
        } else {
            tar::EntryType::Regular
        });
        header.set_size(if meta.is_dir() { 0 } else { meta.len() });
        header.set_mode(mode(meta));
        let timestamp = modified(meta)?;
        header.set_mtime(timestamp.as_secs());
        let mtime = format!("{}.{:09}", timestamp.as_secs(), timestamp.subsec_nanos());
        archive.append_pax_extensions([("mtime", mtime.as_bytes())])?;
        header.set_cksum();
        if meta.is_dir() {
            archive.append_data(&mut header, relative, io::empty())?;
        } else {
            archive.append_data(&mut header, relative, File::open(source)?)?;
        }
        Ok(())
    })?;
    archive.into_inner()?.finish()?.sync_all()?;
    Ok(())
}

/// Validate path, type and bytes on every read, including before restoring a single entry.
fn read_archive(path: &Path, target: Option<(&Path, &Path)>) -> Result<String> {
    let mut archive = tar::Archive::new(GzDecoder::new(File::open(path)?));
    let mut stamps = Vec::new();
    let mut directories = Vec::new();
    let mut stored_admin = BTreeSet::new();
    for item in archive.entries()? {
        let mut item = item?;
        let name = item.path()?.into_owned();
        let admin_entry = name.strip_prefix("admin").ok().is_some_and(|relative| {
            PORTABLE_ADMIN_FILES
                .iter()
                .any(|allowed| relative == Path::new(allowed))
        });
        let relative = if admin_entry {
            name.strip_prefix("admin")?
        } else {
            name.strip_prefix("checkout")?
        };
        let relative = relative_file(relative.to_str().context("快照路径不可编码")?)?;
        let kind = item.header().entry_type();
        ensure!(
            kind.is_file() || (kind.is_dir() && !admin_entry),
            "快照包含未知文件类型"
        );
        let file_mode = item.header().mode()?;
        let mut mtime = None;
        for field in item.pax_extensions()?.context("快照缺少精确文件时间")? {
            let field = field?;
            if field.key_bytes() == b"mtime" {
                let (seconds, nanos) = std::str::from_utf8(field.value_bytes())?
                    .split_once('.')
                    .context("快照文件时间格式无效")?;
                ensure!(
                    nanos.len() == 9 && nanos.bytes().all(|b| b.is_ascii_digit()),
                    "快照文件时间无效"
                );
                mtime = Some(Duration::new(seconds.parse()?, nanos.parse()?));
            }
        }
        let mtime = mtime.context("快照缺少文件修改时间")?;
        if admin_entry {
            stored_admin.insert(relative.clone());
        }
        let data = if let Some((checkout, admin)) = target {
            let root = if admin_entry { admin } else { checkout };
            if kind.is_dir() {
                let location = checked_directory(root, &relative, true)?;
                directories.push((location, file_mode, mtime));
                None
            } else {
                let location = checked_file(root, &relative, true)?;
                if admin_entry {
                    // Git created a fresh HEAD reflog when it registered the new checkout.
                    let mut bytes = Vec::new();
                    item.read_to_end(&mut bytes)?;
                    let expected = digest(bytes.as_slice())?;
                    let marker: RestoreMarker =
                        read_optional(&admin.join("crewvela-restoring.json"))?
                            .context("缺少恢复操作标记")?;
                    let initial = marker
                        .initial
                        .get(&relative)
                        .context("恢复标记缺少原始 Git 元数据")?;
                    let current = file_digest(&location)?;
                    ensure!(
                        &current == initial || current.as_ref() == Some(&expected),
                        "工作树本地引用日志已变化，已保留"
                    );
                    if current.as_ref() != Some(&expected) {
                        write_atomic(&location, &bytes)?;
                    }
                    restore_metadata(&location, file_mode, mtime)?;
                    Some(expected)
                } else if location.try_exists()? {
                    let expected = digest(&mut item)?;
                    ensure!(
                        digest(File::open(&location)?)? == expected,
                        "目标已有内容不同的文件，已保留：{}",
                        location.display()
                    );
                    Some(expected)
                } else {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&location)?;
                    io::copy(&mut item, &mut file)?;
                    file.sync_all()?;
                    drop(file);
                    restore_metadata(&location, file_mode, mtime)?;
                    Some(digest(File::open(&location)?)?)
                }
            }
        } else if kind.is_file() {
            Some(digest(&mut item)?)
        } else {
            None
        };
        stamps.push(stamp(&name, file_mode, mtime, data)?);
    }
    for (directory, file_mode, mtime) in directories.into_iter().rev() {
        restore_metadata(&directory, file_mode, mtime)?;
    }
    if let Some((_, admin)) = target {
        let marker: RestoreMarker =
            read_optional(&admin.join("crewvela-restoring.json"))?.context("缺少恢复操作标记")?;
        for (name, initial) in marker.initial {
            ensure!(
                PORTABLE_ADMIN_FILES
                    .iter()
                    .any(|allowed| name == Path::new(allowed)),
                "恢复标记包含未知 Git 元数据"
            );
            if stored_admin.contains(&name) || initial.is_none() {
                continue;
            }
            let path = checked_file(admin, &name, false)?;
            let current = file_digest(&path)?;
            if current.is_some() {
                ensure!(current == initial, "恢复时 Git 元数据已被修改，已保留");
                fs::remove_file(path)?;
            }
        }
    }
    stamps_digest(&stamps)
}

pub(crate) fn snapshot_for_cleanup(
    worktree: &SessionWorktree,
    session_id: &str,
) -> Result<RecoverySnapshot> {
    snapshot_in(&state_root()?, worktree, session_id)
}

// Attempts that never published a manifest still own their two freshly created paths.
// Remove only those paths; an unrelated file in the directory keeps the directory intact.
struct PendingSnapshot {
    folder: PathBuf,
    published: bool,
}

impl Drop for PendingSnapshot {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(self.folder.join("checkout.tar.gz"));
            let _ = fs::remove_dir(&self.folder);
        }
    }
}

fn snapshot_in(
    state: &Path,
    worktree: &SessionWorktree,
    session_id: &str,
) -> Result<RecoverySnapshot> {
    verify_managed_worktree(worktree, session_id)?;
    let repo = Path::new(&worktree.repo_root);
    let project = project_dir(state, repo)?;
    ready(&project, session_id)?;
    let repository = root_identity(repo)?;
    let (commit, admin) = clean_head(worktree)?;
    let root_meta = fs::symlink_metadata(&worktree.path)?;
    portable_metadata(Path::new(&worktree.path), &root_meta)?;
    let id = Uuid::new_v4().to_string();
    let folder = snapshot_dir(&project, &id)?;
    fs::create_dir_all(folder.parent().context("缺少快照目录")?)?;
    fs::create_dir(&folder)?;
    let mut pending = PendingSnapshot {
        folder: folder.clone(),
        published: false,
    };
    let payload = folder.join("checkout.tar.gz");
    archive_checkout(worktree, &admin, &payload)?;
    let contents = read_archive(&payload, None)?;
    ensure!(
        contents == live_digest(worktree, &admin)?,
        "保存期间文件已变化，已保留原 Worktree"
    );
    let base = format!("refs/crewvela/worktree-snapshots/{id}");
    let git_ref = format!("{base}/head");
    let mut refs = format!("start\ncreate {git_ref} {commit}\n");
    let mut history = pinned_history(&admin)?;
    history.remove(&commit);
    for oid in &history {
        refs.push_str(&format!("create {base}/history/{oid} {oid}\n"));
    }
    refs.push_str("prepare\ncommit\n");
    checked_git(repo, &["update-ref", "--stdin"], Some(refs.into_bytes()))?;
    let checkout_parent = directory_identity(
        Path::new(&worktree.path)
            .parent()
            .context("缺少 Worktree 父目录")?,
    )?;
    let snapshot = RecoverySnapshot {
        id,
        session_id: session_id.into(),
        worktree: worktree.clone(),
        commit,
        git_ref,
        created_at_ms: crate::automation::now_millis(),
        repository,
        contents,
        checkout_parent,
        history,
        root_mode: mode(&root_meta),
        root_modified: modified(&root_meta)?,
    };
    save_json(&folder.join("snapshot.json"), &snapshot)?;
    pending.published = true;
    Ok(snapshot)
}

pub(crate) fn verify_cleanup_snapshot(snapshot: &RecoverySnapshot) -> Result<()> {
    verify_snapshot_in(&state_root()?, snapshot)
}

fn verify_snapshot_in(state: &Path, snapshot: &RecoverySnapshot) -> Result<()> {
    let repo = Path::new(&snapshot.worktree.repo_root);
    let saved = load_snapshot_in(state, repo, &snapshot.id)?;
    ensure!(
        saved.session_id == snapshot.session_id
            && saved.worktree == snapshot.worktree
            && saved.contents == snapshot.contents
            && saved.commit == snapshot.commit,
        "快照记录已变化"
    );
    verify_managed_worktree(&saved.worktree, &saved.session_id)?;
    let project = project_dir(state, repo)?;
    ready(&project, &saved.session_id)?;
    let (head, admin) = clean_head(&saved.worktree)?;
    let root_meta = fs::symlink_metadata(&saved.worktree.path)?;
    portable_metadata(Path::new(&saved.worktree.path), &root_meta)?;
    ensure!(
        head == saved.commit
            && mode(&root_meta) == saved.root_mode
            && modified(&root_meta)? == saved.root_modified
            && live_digest(&saved.worktree, &admin)? == saved.contents,
        "快照生成后工作树已变化，已保留原目录"
    );
    Ok(())
}

fn load_snapshot_in(state: &Path, repo: &Path, id: &str) -> Result<RecoverySnapshot> {
    let folder = snapshot_dir(&project_dir(state, repo)?, id)?;
    let saved: RecoverySnapshot =
        read_optional(&folder.join("snapshot.json"))?.context("找不到已完成的恢复快照")?;
    ensure!(
        saved.id == id
            && saved.repository == root_identity(repo)?
            && Path::new(&saved.worktree.repo_root).canonicalize()? == repo.canonicalize()?
            && saved.git_ref == format!("refs/crewvela/worktree-snapshots/{id}/head"),
        "恢复快照与当前仓库不匹配"
    );
    let refs = git_text(
        repo,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            &format!("refs/crewvela/worktree-snapshots/{id}/history/"),
        ],
    )?;
    ensure!(
        refs.lines().collect::<BTreeSet<_>>() == saved.history.iter().map(String::as_str).collect(),
        "快照关联的 Git 历史引用已变化"
    );
    ensure!(
        git_text(
            repo,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{commit}}", saved.git_ref)
            ]
        )? == saved.commit
            && read_archive(&folder.join("checkout.tar.gz"), None)? == saved.contents,
        "快照引用或文件校验失败，已保留原数据"
    );
    Ok(saved)
}

pub(crate) fn load_recovery_snapshot(repo: &Path, id: &str) -> Result<RecoverySnapshot> {
    load_snapshot_in(&state_root()?, repo, id)
}

/// Recreate the original checkout without touching another checkout, advancing a branch,
/// overwriting workspace files or running user hooks. The caller reattaches the ownership token.
pub(crate) fn restore_snapshot_checkout(snapshot: &RecoverySnapshot) -> Result<()> {
    restore_in(&state_root()?, snapshot)
}

#[derive(Deserialize, Serialize)]
struct RestoreMarker {
    snapshot_id: String,
    initial: BTreeMap<PathBuf, Option<String>>,
}

fn file_digest(path: &Path) -> Result<Option<String>> {
    match File::open(path) {
        Ok(file) => Ok(Some(digest(file)?)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn restore_in(state: &Path, request: &RecoverySnapshot) -> Result<()> {
    let repo = Path::new(&request.worktree.repo_root);
    let saved = load_snapshot_in(state, repo, &request.id)?;
    let target = Path::new(&saved.worktree.path);
    ensure!(
        directory_identity(target.parent().context("缺少 Worktree 父目录")?)?
            == saved.checkout_parent,
        "原 Worktree 父目录已变更，已保留快照"
    );
    let payload = snapshot_dir(&project_dir(state, repo)?, &saved.id)?.join("checkout.tar.gz");
    let branch_ref = format!("refs/heads/{}", saved.worktree.branch);
    checked_git(repo, &["check-ref-format", &branch_ref], None)?;
    let known = git_text(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)",
            &branch_ref,
        ],
    )?;
    let branch = known
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(name, _)| *name == branch_ref);
    ensure!(
        branch.is_none_or(|(_, oid)| oid == saved.commit),
        "原分支已前移，已保留当前分支与快照 {}",
        saved.git_ref
    );
    let existing = match fs::symlink_metadata(target) {
        Ok(meta) => {
            ensure!(ordinary(&meta) && meta.is_dir(), "原路径已被占用，已保留");
            true
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    if !existing {
        // Reserve the *exact* path. Git otherwise accepts an unrelated existing empty directory.
        fs::create_dir(target)?;
        let mut args = vec!["worktree", "add", "--no-checkout"];
        if branch.is_none() {
            args.extend(["-b", &saved.worktree.branch]);
        }
        args.extend([
            "--",
            &saved.worktree.path,
            if branch.is_some() {
                &saved.worktree.branch
            } else {
                &saved.git_ref
            },
        ]);
        if let Err(e) = checked_git(repo, &args, None) {
            let _ = fs::remove_dir(target); // Only the newly reserved, still-empty directory.
            return Err(e);
        }
    }
    ensure!(
        Path::new(&git_text(target, &["rev-parse", "--show-toplevel"])?).canonicalize()?
            == target.canonicalize()?
            && root_identity(target)? == saved.repository
            && git_text(target, &["symbolic-ref", "HEAD"])? == branch_ref
            && git_text(target, &["rev-parse", "--verify", "HEAD^{commit}"])? == saved.commit,
        "原路径或分支的登记与恢复快照不一致"
    );
    let admin = PathBuf::from(git_text(target, &["rev-parse", "--absolute-git-dir"])?);
    let marker = admin.join("crewvela-restoring.json");
    if existing {
        ensure!(
            read_optional::<RestoreMarker>(&marker)?.is_some_and(|m| m.snapshot_id == saved.id),
            "原路径已被占用，已保留现有目录及快照"
        );
        if admin.join("crewvela-owner.json").try_exists()? {
            ensure!(
                live_digest(&saved.worktree, &admin)? == saved.contents,
                "已恢复的工作树发生变化，已保留"
            );
            return Ok(());
        }
    } else {
        let mut initial = BTreeMap::new();
        for name in PORTABLE_ADMIN_FILES {
            initial.insert(PathBuf::from(name), file_digest(&admin.join(name))?);
        }
        save_json(
            &marker,
            &RestoreMarker {
                snapshot_id: saved.id.clone(),
                initial,
            },
        )?;
    }
    if existing && admin.join("index").try_exists()? {
        // Retrying an interrupted restore must never reset changes staged by an external Git.
        checked_git(
            target,
            &["diff-index", "--cached", "--quiet", &saved.commit, "--"],
            None,
        )
        .context("恢复期间暂存区发生变化，已保留")?;
    } else {
        checked_git(target, &["read-tree", &saved.commit], None)?;
    }
    ensure!(
        read_archive(&payload, Some((target, &admin)))? == saved.contents,
        "恢复文件校验失败，快照已保留"
    );
    restore_metadata(target, saved.root_mode, saved.root_modified)?;
    let (head, admin) = clean_head(&saved.worktree)?;
    ensure!(
        head == saved.commit && live_digest(&saved.worktree, &admin)? == saved.contents,
        "恢复后的日志、文件或分支校验失败，快照已保留"
    );
    Ok(())
}

fn view(repo: &Path, session_id: Option<&str>) -> Result<LifecycleView> {
    let project = project_dir(&state_root()?, repo)?;
    let identity = root_identity(repo)?;
    let (policy, warning) = match read_optional::<TrustedPolicy>(&project.join("policy.json"))? {
        Some(saved) if saved.repository == identity => {
            validate_policy(&saved.policy)?;
            (saved.policy, None)
        }
        Some(_) => (
            LifecyclePolicy::default(),
            Some("这个路径上的 Git 仓库已更换，请重新配置并保存。".into()),
        ),
        None => (LifecyclePolicy::default(), None),
    };
    let run = session_id
        .map(|id| read_optional(&run_path(&project, id)))
        .transpose()?
        .flatten();
    let directory = project.join("snapshots");
    let mut snapshots: Vec<SnapshotView> = Vec::new();
    if directory.try_exists()? {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(saved) =
                read_optional::<RecoverySnapshot>(&entry.path().join("snapshot.json"))?
            {
                if saved.repository == identity
                    && session_id.is_none_or(|id| id == saved.session_id)
                {
                    snapshots.push(SnapshotView {
                        id: saved.id,
                        session_id: saved.session_id,
                        branch: saved.worktree.branch,
                        path: saved.worktree.path,
                        commit: saved.commit,
                        git_ref: saved.git_ref,
                        archive_path: entry.path().join("checkout.tar.gz").display().to_string(),
                        created_at_ms: saved.created_at_ms,
                    });
                }
            }
        }
        snapshots.sort_by_key(|s| std::cmp::Reverse(s.created_at_ms));
    }
    Ok(LifecycleView {
        policy,
        run,
        snapshots,
        warning,
    })
}

#[tauri::command]
pub(crate) async fn worktree_lifecycle_get(
    repo_root: String,
    session_id: Option<String>,
) -> Result<LifecycleView, String> {
    crate::blocking_command(move || {
        view(Path::new(&repo_root), session_id.as_deref()).map_err(crate::to_err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn worktree_lifecycle_save(
    repo_root: String,
    policy: LifecyclePolicy,
) -> Result<(), String> {
    crate::blocking_command(move || {
        (|| -> Result<()> {
            validate_policy(&policy)?;
            let repo = Path::new(&repo_root);
            let repository = root_identity(repo)?;
            let project = project_dir(&state_root()?, repo)?;
            save_json(
                &project.join("policy.json"),
                &TrustedPolicy { repository, policy },
            )
        })()
        .map_err(crate::to_err)
    })
    .await
}

#[cfg(test)]
#[path = "worktree_lifecycle_tests.rs"]
mod tests;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// 会话级独立 Git Worktree 元数据，随 SessionRecord 持久化。
///
/// 借鉴 Orca 的「一个 Agent 任务 = 一个隔离 checkout」模型：多会话并行改同一个
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
}

fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir);
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

/// 基于 dir 所在仓库的当前 HEAD 为会话创建独立 Worktree（新分支）。
pub fn create_session_worktree(dir: &Path, session_id: &str) -> Result<SessionWorktree> {
    let root = crate::providers::config_dir()?
        .join("worktrees")
        .join("sessions");
    create_session_worktree_in(dir, session_id, &root)
}

fn create_session_worktree_in(
    dir: &Path,
    session_id: &str,
    worktrees_root: &Path,
) -> Result<SessionWorktree> {
    let root = repo_root(dir)?;
    let path = worktrees_root.join(session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let short = session_id.chars().take(8).collect::<String>();
    let branch = crate::git_source_control::managed_branch_name("session", &short);
    let path_text = path.display().to_string();
    let output = git_output(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            branch.as_str(),
            path_text.as_str(),
            "HEAD",
        ],
    )?;
    if !output.status.success() {
        return Err(anyhow!("创建会话 Worktree 失败：{}", git_stderr(&output)));
    }
    Ok(SessionWorktree {
        repo_root: root.display().to_string(),
        path: path_text,
        branch,
    })
}

/// Worktree 是否存在未提交改动（含未跟踪文件）。
pub fn worktree_has_changes(path: &Path) -> Result<bool> {
    let output = git_output(path, &["status", "--porcelain"])?;
    if !output.status.success() {
        return Err(anyhow!("读取 Worktree 状态失败：{}", git_stderr(&output)));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

/// 强制删除会话 Worktree（仅用于启动失败的回滚路径，刚建的 Worktree 不会有改动）。
pub fn force_remove_session_worktree(worktree: &SessionWorktree) -> Result<()> {
    let output = git_output(
        Path::new(&worktree.repo_root),
        &["worktree", "remove", "--force", worktree.path.as_str()],
    )?;
    if !output.status.success() {
        return Err(anyhow!("清理会话 Worktree 失败：{}", git_stderr(&output)));
    }
    Ok(())
}

/// 删除会话前的 Worktree 清理。存在未提交改动时拒绝（返回 Err，调用方应保留会话记录），
/// 避免静默丢掉 Agent 已写入的代码。清理成功后保留 crewvela/session-* 分支：
/// 已提交的 commit 仍可从该分支找回，是否合并/删除分支交给用户决定。
pub fn cleanup_session_worktree(worktree: &SessionWorktree) -> Result<()> {
    let path = Path::new(&worktree.path);
    let root = Path::new(&worktree.repo_root);
    if !path.exists() {
        // 目录已被手动删除，prune 掉 git 侧的注册信息即可。
        let _ = git_output(root, &["worktree", "prune"]);
        return Ok(());
    }
    if worktree_has_changes(path)? {
        return Err(anyhow!(
            "Worktree 存在未提交改动，请先提交或清理：{}",
            worktree.path
        ));
    }
    let output = git_output(root, &["worktree", "remove", worktree.path.as_str()])?;
    if !output.status.success() {
        return Err(anyhow!("清理会话 Worktree 失败：{}", git_stderr(&output)));
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
            let base = std::env::temp_dir().join(format!(
                "crewvela-worktree-{name}-{}",
                uuid::Uuid::new_v4()
            ));
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

        cleanup_session_worktree(&worktree).unwrap();
        assert!(!Path::new(&worktree.path).exists());
    }

    #[test]
    fn cleanup_rejects_worktree_with_uncommitted_changes() {
        let repo = TestRepo::new("dirty");
        let worktree =
            create_session_worktree_in(&repo.root, "session-dirty123456", &repo.worktrees_root)
                .unwrap();
        fs::write(Path::new(&worktree.path).join("new-file.rs"), "fn main() {}").unwrap();

        let error = cleanup_session_worktree(&worktree).unwrap_err();

        assert!(error.to_string().contains("未提交改动"));
        assert!(Path::new(&worktree.path).exists(), "脏 worktree 必须保留");

        force_remove_session_worktree(&worktree).unwrap();
        assert!(!Path::new(&worktree.path).exists());
    }

    #[test]
    fn cleanup_tolerates_manually_deleted_worktree() {
        let repo = TestRepo::new("prune");
        let worktree =
            create_session_worktree_in(&repo.root, "session-prune123456", &repo.worktrees_root)
                .unwrap();
        fs::remove_dir_all(&worktree.path).unwrap();

        cleanup_session_worktree(&worktree).unwrap();

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
}

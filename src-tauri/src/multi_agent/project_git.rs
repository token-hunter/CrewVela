use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Mutex,
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

static PREPARATION: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectGitStatus {
    state: GitState,
    repository_path: Option<String>,
    has_files: bool,
    staged_files: Vec<String>,
    staged_token: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum GitState {
    Ready,
    MissingRepository,
    MissingCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectGitPreparation {
    confirm_existing_files: bool,
    staged_token: Option<String>,
}

fn git(path: &Path) -> Command {
    let mut command = super::executor::hidden_command("git");
    command
        .arg("-C")
        .arg(path)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
    command
}

fn output(command: &mut Command) -> Result<Output> {
    command.output().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            anyhow::anyhow!("找不到 Git，请安装 Git 后重新检查。")
        } else {
            anyhow::anyhow!("无法运行 Git：{error}")
        }
    })
}

fn checked(command: &mut Command, action: &str) -> Result<Vec<u8>> {
    let result = output(command)?;
    if !result.status.success() {
        bail!(
            "{action}：{}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    Ok(result.stdout)
}

fn nearest_directory(path: &Path) -> Result<&Path> {
    if !path.is_absolute() {
        bail!("项目目录请填写本地绝对路径。");
    }
    for ancestor in path.ancestors() {
        match fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => return Ok(ancestor),
            Ok(_) => bail!("项目路径包含普通文件：{}", ancestor.display()),
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("无法读取项目目录：{}", ancestor.display()))
            }
        }
    }
    bail!("找不到项目所在的磁盘或共享目录：{}", path.display())
}

fn has_files(path: &Path) -> Result<bool> {
    for entry in fs::read_dir(path).with_context(|| format!("无法读取目录：{}", path.display()))?
    {
        if entry?.file_name() != ".git" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn staged_entries(command: &mut Command) -> Result<Vec<u8>> {
    checked(
        command.args(["ls-files", "--stage", "-z"]),
        "读取已暂存文件失败",
    )
}

fn staged_token(entries: &[u8]) -> String {
    format!("{:x}", Sha256::digest(entries))
}

pub(crate) fn inspect(path: &Path) -> Result<ProjectGitStatus> {
    let parent = nearest_directory(path)?;
    let probe = output(git(parent).args(["rev-parse", "--show-toplevel"]))?;
    if !probe.status.success() {
        let reason = String::from_utf8_lossy(&probe.stderr);
        if !reason.contains("not a git repository") {
            bail!("无法使用 Git 工作区：{}", reason.trim());
        }
        for dir in parent.ancestors() {
            match fs::symlink_metadata(dir.join(".git")) {
                Ok(_) => bail!("无法读取已有 Git 仓库：{}", dir.display()),
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("无法检查已有 Git 仓库"),
            }
        }
        return Ok(ProjectGitStatus {
            state: GitState::MissingRepository,
            repository_path: None,
            has_files: parent == path && has_files(path)?,
            staged_files: Vec::new(),
            staged_token: None,
        });
    }
    let repository =
        PathBuf::from(String::from_utf8_lossy(&probe.stdout).trim_end_matches(['\r', '\n']));
    let repository_path = Some(repository.to_string_lossy().into_owned());
    if output(git(&repository).args(["rev-parse", "--verify", "HEAD^{commit}"]))?
        .status
        .success()
    {
        return Ok(ProjectGitStatus {
            state: GitState::Ready,
            repository_path,
            has_files: false,
            staged_files: Vec::new(),
            staged_token: None,
        });
    }
    let branch = checked(
        git(&repository).args(["symbolic-ref", "--quiet", "HEAD"]),
        "读取仓库当前分支失败",
    )?;
    let branch = String::from_utf8_lossy(&branch);
    if output(git(&repository).args(["show-ref", "--verify", "--quiet", branch.trim()]))?
        .status
        .code()
        != Some(1)
    {
        bail!("当前 Git 分支无法解析到有效提交：{}", branch.trim());
    }
    let entries = staged_entries(&mut git(&repository))?;
    let staged_files = entries
        .split(|byte| *byte == 0)
        .filter_map(|record| {
            let separator = record.iter().position(|byte| *byte == b'\t')?;
            Some(String::from_utf8_lossy(&record[separator + 1..]).into_owned())
        })
        .collect();
    Ok(ProjectGitStatus {
        state: GitState::MissingCommit,
        repository_path,
        has_files: !entries.is_empty() || has_files(&repository)?,
        staged_files,
        staged_token: (!entries.is_empty()).then(|| staged_token(&entries)),
    })
}

fn first_commit(command: &mut Command, empty: bool) -> Result<()> {
    command.args([
        "-c",
        "user.name=CrewVela",
        "-c",
        "user.email=crewvela@local",
        "commit",
        "--allow-empty",
    ]);
    if empty {
        command.arg("--only");
    }
    checked(
        command.args(["-m", "Initial commit"]),
        "创建首次 Git 提交失败",
    )?;
    Ok(())
}

pub(crate) fn prepare(path: &Path, confirmation: ProjectGitPreparation) -> Result<()> {
    let _guard = PREPARATION.lock().unwrap();
    let status = inspect(path)?;
    if status.state == GitState::Ready {
        return Ok(());
    }
    if status.has_files && !confirmation.confirm_existing_files {
        bail!("项目目录已有内容，请重新检查并确认首次提交范围。");
    }
    if confirmation
        .staged_token
        .as_ref()
        .is_some_and(|token| Some(token) != status.staged_token.as_ref())
    {
        bail!("已暂存内容发生变化，请重新检查并确认首次提交范围。");
    }
    let repository = status
        .repository_path
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf());
    if status.state == GitState::MissingRepository {
        fs::create_dir_all(path)
            .with_context(|| format!("创建项目目录失败：{}", path.display()))?;
        checked(
            git(path).args(["init", "--initial-branch=main"]),
            "初始化 Git 仓库失败",
        )?;
    }
    if let Some(token) = confirmation.staged_token {
        let index_path = checked(
            git(&repository).args(["rev-parse", "--git-path", "index"]),
            "定位 Git 暂存区失败",
        )?;
        let index =
            repository.join(String::from_utf8_lossy(&index_path).trim_end_matches(['\r', '\n']));
        let copy = index.with_file_name(format!("crewvela-first-{}.index", uuid::Uuid::new_v4()));
        fs::copy(index, &copy).context("无法读取 Git 暂存区")?;
        let result = (|| {
            let mut command = git(&repository);
            command.env("GIT_INDEX_FILE", &copy);
            if staged_token(&staged_entries(&mut command)?) != token {
                bail!("已暂存内容发生变化，请重新检查并确认首次提交范围。");
            }
            let mut commit = git(&repository);
            first_commit(commit.env("GIT_INDEX_FILE", &copy), false)
        })();
        let _ = fs::remove_file(copy);
        result?;
    } else {
        first_commit(&mut git(&repository), true)?;
    }
    checked(
        git(&repository).args(["rev-parse", "--verify", "HEAD^{commit}"]),
        "验证首次提交失败",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multi_agent::{CreateProjectInput, MultiAgentStore};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("crewvela-project-git-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn empty(confirm_existing_files: bool) -> ProjectGitPreparation {
        ProjectGitPreparation {
            confirm_existing_files,
            staged_token: None,
        }
    }

    fn run(path: &Path, args: &[&str]) -> Vec<u8> {
        checked(git(path).args(args), "test Git command").unwrap()
    }

    #[test]
    fn creates_a_new_workspace_and_reuses_existing_head_in_subdirectories() {
        let fixture = Fixture::new();
        let root = fixture.0.join("新项目 & space");
        let store = MultiAgentStore::open_at(fixture.0.join("control.db")).unwrap();
        let input = |path: &Path| CreateProjectInput {
            name: "测试项目".into(),
            goal: "准备工作树".into(),
            root_path: path.to_string_lossy().into_owned(),
            default_branch: "main".into(),
        };
        assert_eq!(inspect(&root).unwrap().state, GitState::MissingRepository);
        assert!(
            !root.exists(),
            "inspection must not create the project folder"
        );
        store
            .create_project_with_git(input(&root), Some(empty(false)))
            .unwrap();
        let head = run(&root, &["rev-parse", "HEAD"]);
        assert!(run(&root, &["ls-tree", "-r", "HEAD"]).is_empty());
        assert_eq!(run(&root, &["branch", "--show-current"]), b"main\n");
        let subdirectory = root.join("child");
        assert_eq!(inspect(&subdirectory).unwrap().state, GitState::Ready);
        fs::write(root.join("local.txt"), "uncommitted").unwrap();
        run(&root, &["add", "--", "local.txt"]);
        store
            .create_project_with_git(input(&subdirectory), Some(empty(false)))
            .unwrap();
        assert_eq!(run(&root, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            run(&root, &["diff", "--cached", "--name-only"]),
            b"local.txt\n"
        );
        assert!(!subdirectory.join(".git").exists());
        let chat = fixture.0.join("chat-only");
        store.create_project_with_git(input(&chat), None).unwrap();
        assert!(chat.is_dir() && !chat.join(".git").exists());
    }

    #[test]
    fn existing_files_require_confirmation_and_can_stay_outside_the_first_commit() {
        let fixture = Fixture::new();
        let root = &fixture.0;
        fs::write(root.join("local.txt"), "keep").unwrap();
        assert!(prepare(root, empty(false)).is_err());
        assert!(!root.join(".git").exists());
        run(root, &["init", "--initial-branch=main"]);
        run(root, &["add", "--", "local.txt"]);
        let status = inspect(root).unwrap();
        assert_eq!(status.state, GitState::MissingCommit);
        assert_eq!(status.staged_files, ["local.txt"]);
        prepare(root, empty(true)).unwrap();
        assert!(run(root, &["ls-tree", "-r", "HEAD"]).is_empty());
        assert_eq!(
            run(root, &["diff", "--cached", "--name-only"]),
            b"local.txt\n"
        );
    }

    #[test]
    fn includes_only_the_confirmed_staged_contents_and_detects_changes_to_the_index() {
        let fixture = Fixture::new();
        let root = &fixture.0;
        run(root, &["init", "--initial-branch=dev"]);
        fs::write(root.join("tracked.txt"), "first").unwrap();
        run(root, &["add", "--", "tracked.txt"]);
        let initial = inspect(root).unwrap().staged_token;
        fs::write(root.join("tracked.txt"), "staged").unwrap();
        run(root, &["add", "--", "tracked.txt"]);
        fs::write(root.join("tracked.txt"), "unstaged").unwrap();
        fs::write(root.join("local.txt"), "untracked").unwrap();
        assert!(prepare(
            root,
            ProjectGitPreparation {
                confirm_existing_files: true,
                staged_token: initial
            }
        )
        .is_err());
        let current = inspect(root).unwrap().staged_token;
        prepare(
            root,
            ProjectGitPreparation {
                confirm_existing_files: true,
                staged_token: current,
            },
        )
        .unwrap();
        assert_eq!(run(root, &["show", "HEAD:tracked.txt"]), b"staged");
        assert_eq!(
            run(root, &["ls-tree", "-r", "--name-only", "HEAD"]),
            b"tracked.txt\n"
        );
        assert_eq!(run(root, &["branch", "--show-current"]), b"dev\n");
        assert_eq!(fs::read(root.join("tracked.txt")).unwrap(), b"unstaged");
        assert!(run(root, &["diff", "--cached", "--name-only"]).is_empty());
    }
}

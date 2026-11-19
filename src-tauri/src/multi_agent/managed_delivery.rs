use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::conversation::{ConversationExecutionPlan, ConversationSendInput};
use super::executor::{git_head, hidden_command};
use super::project_preview::PreviewConfig;
use super::{
    AgentRuntimeKind, MultiAgentProject, MultiAgentSnapshot, MultiAgentStore, ProjectStatus,
    TaskStatus, WorkspaceMode,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedTarget {
    pub root: String,
    pub branch: String,
    pub(super) base: String,
    pub planner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedDelivery {
    pub target: ManagedTarget,
    pub status: String,
    pub summary: Option<String>,
    pub commit: Option<String>,
    pub verification_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewConfig>,
}

impl ManagedDelivery {
    pub(super) fn new(target: ManagedTarget) -> Self {
        Self {
            target,
            status: "planning".into(),
            summary: None,
            commit: None,
            verification_command: None,
            preview: None,
        }
    }
}

pub(super) fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = hidden_command("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .context("无法启动 Git")?;
    ensure!(
        output.status.success(),
        "Git 交付检查失败：{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl ManagedTarget {
    pub(super) fn check(&self, project_root: &str) -> Result<PathBuf> {
        let root = Path::new(&self.root);
        ensure!(
            root.canonicalize()? == Path::new(project_root).canonicalize()?,
            "项目目录已变更"
        );
        ensure!(
            git(root, &["symbolic-ref", "--short", "HEAD"])? == self.branch,
            "项目已切换分支"
        );
        ensure!(
            git(root, &["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
            "交付目录存在未提交的变更"
        );
        Ok(root.to_path_buf())
    }

    pub(super) fn check_before_start(&self, project_root: &str) -> Result<()> {
        let root = self.check(project_root)?;
        ensure!(
            git_head(&root)? == self.base,
            "交付分支已有新提交，请重新启动托管"
        );
        Ok(())
    }

    pub(super) fn merge(&self, project: &MultiAgentProject, commit: &str) -> Result<()> {
        ensure!(commit != self.base, "本轮没有实际交付提交");
        let root = self.check(&project.root_path)?;
        let readme = git(&root, &["show", &format!("{commit}:./README.md")])
            .context("成品须提供已提交的 README.md，说明如何启动和使用")?;
        ensure!(!readme.is_empty(), "成品的 README.md 为空");
        let head = git_head(&root)?;
        if head == commit {
            return Ok(());
        }
        ensure!(head == self.base, "交付分支已移动，无法自动合入本轮成品");
        git(&root, &["merge", "--ff-only", commit]).context("无法快进合入已验收的成品")?;
        ensure!(git_head(&root)? == commit, "交付分支与验收提交不一致");
        Ok(())
    }
}

pub(super) fn check_runtime(kind: AgentRuntimeKind, mode: WorkspaceMode) -> Result<()> {
    let preferences = crate::runtime_preferences::get()?;
    check_launch_mode(kind, mode, preferences.default_launch_modes.get(kind.as_str()).map(String::as_str))
}

pub(super) fn check_launch_mode(kind: AgentRuntimeKind, mode: WorkspaceMode, saved: Option<&str>) -> Result<()> {
    ensure!(
        matches!(kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex),
        "全自动托管当前支持 Claude Code 和 Codex"
    );
    if mode == WorkspaceMode::ReadOnly {
        return Ok(());
    }
    ensure!(mode == WorkspaceMode::Worktree, "托管开发或联调需要独立 Git Worktree");
    match kind {
        AgentRuntimeKind::ClaudeCode => ensure!(saved == Some("auto"),
            "Claude Code 托管开发或联调需要在「管理 Runtime」中选择 Auto"),
        AgentRuntimeKind::Codex => ensure!(matches!(saved, None | Some("auto")),
            "Codex 托管开发或联调需要工作区写入沙箱和无人值守审批，请在「管理 Runtime」中选择 Approve for me"),
        _ => unreachable!("checked supported runtimes"),
    }
    Ok(())
}

pub(super) fn prepare(
    connection: &Connection,
    input: &ConversationSendInput,
    root: String,
    branch: String,
) -> Result<ManagedTarget> {
    let mut planner = None;
    let mut roles = std::collections::HashSet::new();
    for id in &input.participant_agent_ids {
        let (role, runtime): (String, String) = connection.query_row(
            "SELECT role,runtime_kind FROM ma_agents WHERE id=?1 AND project_id=?2 AND enabled=1",
            params![id, input.project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let kind: AgentRuntimeKind = serde_json::from_value(serde_json::Value::String(runtime))?;
        if kind == AgentRuntimeKind::ClaudeCode && planner.is_none() {
            planner = Some(id.clone());
        }
        let mode = if matches!(role.as_str(), "worker" | "integrator") {
            WorkspaceMode::Worktree
        } else {
            WorkspaceMode::ReadOnly
        };
        check_runtime(kind, mode)?;
        roles.insert(role);
    }
    ensure!(
        ["worker", "reviewer", "integrator"]
            .iter()
            .all(|role| roles.contains(*role)),
        "全自动托管需要开发、审查和集成成员"
    );
    let planner = planner.context("请选择至少一位 Claude Code 成员作只读规划")?;
    let base = git_head(Path::new(&root))?;
    let target = ManagedTarget {
        root,
        branch,
        base,
        planner,
    };
    target.check_before_start(&target.root)?;
    Ok(target)
}

// Run a single test entry point with literal arguments. Manual plans continue
// to use the existing confirmed shell command contract.
pub(super) fn verification(command: &str) -> Result<(Command, Option<&str>)> {
    let args: Vec<_> = command.split_ascii_whitespace().collect();
    let (program, arguments, test_path) = match args.as_slice() {
        ["node", "--test", file] => {
            ensure!(
                file.ends_with(".js") || file.ends_with(".mjs") || file.ends_with(".cjs"),
                "Node 验收须指定测试文件"
            );
            ensure!(
                file.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/_.-".contains(&c))
                    && file
                        .split('/')
                        .all(|part| !part.is_empty() && part != "." && part != "..")
                    && !file.starts_with('-'),
                "验收测试路径必须是项目内的相对文件路径"
            );
            ("node", vec!["--test", *file], Some(*file))
        }
        ["npm", "test"] => (
            if cfg!(windows) { "npm.cmd" } else { "npm" },
            vec!["test"],
            None,
        ),
        ["python", "-m", "unittest"] => ("python", vec!["-m", "unittest"], None),
        ["cargo", "test"] => ("cargo", vec!["test"], None),
        ["go", "test", "./..."] => ("go", vec!["test", "./..."], None),
        ["dotnet", "test"] => ("dotnet", vec!["test"], None),
        _ => bail!(
            "托管验收需要一条独立测试命令，例如 node --test tests/acceptance.test.js 或 npm test"
        ),
    };
    let mut cmd = hidden_command(program);
    cmd.args(arguments);
    Ok((cmd, test_path))
}

pub(super) fn task_target(connection: &Connection, task_id: &str) -> Result<Option<ManagedTarget>> {
    let raw: Option<String> = connection
        .query_row(
            "SELECT json_extract(p.data_json,'$.managed.target') FROM ma_conversation_task_scopes s
         JOIN ma_conversation_plans p ON p.id=s.plan_id
         WHERE s.task_id=?1 AND json_type(p.data_json,'$.managed.target')='object'",
            [task_id],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into))
        .transpose()
}

impl MultiAgentStore {
    pub(super) fn managed_task_target(&self, task_id: &str) -> Result<Option<ManagedTarget>> {
        task_target(&self.connection.lock().unwrap(), task_id)
    }

    pub(super) fn managed_write_paths(&self, task_id: &str) -> Result<Vec<String>> {
        let raw: String = self.connection.lock().unwrap().query_row(
            "SELECT write_paths_json FROM ma_conversation_task_scopes WHERE task_id=?1",
            [task_id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub(super) fn managed_plan_running(
        &self,
        project_id: &str,
    ) -> Result<Option<ConversationExecutionPlan>> {
        let connection = self.connection.lock().unwrap();
        let raw: Option<String> = connection
            .query_row(
                "SELECT data_json FROM ma_conversation_plans
             WHERE project_id=?1 AND json_extract(data_json,'$.managed.status')='running'",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into))
            .transpose()
    }

    pub(super) fn finish_managed_plan(
        &self,
        plan_id: &str,
        result: Result<(String, String, String), String>,
    ) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row(
            "SELECT data_json FROM ma_conversation_plans WHERE id=?1",
            [plan_id],
            |row| row.get(0),
        )?;
        let mut plan: ConversationExecutionPlan = serde_json::from_str(&raw)?;
        let managed = plan.managed.as_mut().context("缺少托管状态")?;
        if matches!(managed.status.as_str(), "completed" | "failed") {
            return Ok(());
        }
        match result {
            Ok((hash, command, summary)) => {
                managed.status = "completed".into();
                managed.preview = PreviewConfig::from_delivery(Path::new(&managed.target.root), &hash);
                managed.commit = Some(hash);
                managed.verification_command = Some(command);
                managed.summary = Some(summary);
            }
            Err(reason) => {
                managed.status = "failed".into();
                managed.summary = Some(reason);
                // Leave finished work and unverified processes untouched. Unclaimed
                // work from this plan must not be picked up by later manual plans.
                for task in &plan.task_ids {
                    tx.execute("UPDATE ma_tasks SET status='cancelled' WHERE id=?1 AND status IN ('ready','draft','blocked','changes_requested')", [task])?;
                }
            }
        }
        tx.execute(
            "UPDATE ma_conversation_plans SET data_json=?2 WHERE id=?1",
            params![plan_id, serde_json::to_string(&plan)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn settle_idle_managed_plan(
        &self,
        snapshot: &MultiAgentSnapshot,
        plan: &ConversationExecutionPlan,
    ) -> Result<()> {
        let tasks: Vec<_> = snapshot
            .tasks
            .iter()
            .filter(|task| plan.task_ids.contains(&task.id))
            .collect();
        let result = (|| -> Result<(String, String, String)> {
            if let Some(run) = snapshot.runs.iter().find(|run| {
                plan.task_ids.contains(&run.task_id)
                    && run
                        .session_locator
                        .as_ref()
                        .is_some_and(|locator| locator["stopUnverified"] == true)
            }) {
                bail!(
                    "{}",
                    run.error
                        .as_deref()
                        .unwrap_or("原 Agent 进程停止未获确认，已停止派发")
                );
            }
            if let Some(task) = tasks
                .iter()
                .find(|task| matches!(task.status, TaskStatus::Failed | TaskStatus::Cancelled))
            {
                let error = snapshot
                    .runs
                    .iter()
                    .filter(|run| run.task_id == task.id)
                    .max_by_key(|run| run.attempt)
                    .and_then(|run| run.error.as_deref());
                bail!(
                    "{}：{}",
                    task.title,
                    error.unwrap_or("执行或审查未通过，已用尽可用尝试")
                );
            }
            ensure!(
                tasks.len() == plan.task_ids.len()
                    && tasks.iter().all(|task| task.status == TaskStatus::Done),
                "托管任务已无可领取的工作。未完成：{}",
                tasks
                    .iter()
                    .filter(|task| task.status != TaskStatus::Done)
                    .map(|task| format!("{} ({})", task.title, task.status.as_str()))
                    .collect::<Vec<_>>()
                    .join("、")
            );
            let final_task = tasks
                .iter()
                .find(|task| {
                    snapshot.agents.iter().any(|agent| {
                        Some(&agent.id) == task.assignee_agent_id.as_ref()
                            && agent.role == super::AgentRole::Integrator
                    })
                })
                .context("缺少集成交付任务")?;
            let run = snapshot
                .runs
                .iter()
                .filter(|run| {
                    run.task_id == final_task.id && run.status == super::RunStatus::Succeeded
                })
                .max_by_key(|run| run.attempt)
                .context("缺少成功的集成运行")?;
            let artifact = snapshot
                .artifacts
                .iter()
                .find(|artifact| {
                    artifact.run_id == run.id && artifact.artifact_type == "git_commit"
                })
                .context("缺少集成提交")?;
            let hash = artifact.content_hash.clone().context("集成提交未记录")?;
            ensure!(
                git_head(Path::new(&snapshot.project.root_path))? == hash,
                "交付分支与验收提交不一致"
            );
            Ok((
                hash,
                final_task
                    .verification_command
                    .clone()
                    .context("缺少成品验收命令")?,
                run.output_summary
                    .clone()
                    .unwrap_or_else(|| "交付已通过验证。".into()),
            ))
        })();
        let status = if result.is_ok() {
            ProjectStatus::Completed
        } else {
            ProjectStatus::Paused
        };
        self.finish_managed_plan(&plan.id, result.map_err(|error| format!("{error:#}")))?;
        self.set_project_status(&snapshot.project.id, status)?;
        Ok(())
    }

    pub(crate) fn recover_managed_deliveries_at_startup(&self) -> Result<()> {
        let plans = {
            let connection = self.connection.lock().unwrap();
            let mut statement = connection.prepare("SELECT id FROM ma_conversation_plans WHERE json_extract(data_json,'$.managed.status') IN ('planning','running')")?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        for plan in plans {
            self.finish_managed_plan(
                &plan,
                Err("CrewVela 已退出或重启，本轮未完成交付。已保存任务与工作树记录。".into()),
            )?;
        }
        Ok(())
    }
}

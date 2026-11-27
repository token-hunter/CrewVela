use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::conversation::{ConversationExecutionPlan, ConversationSendInput};
use super::executor::{git_head, hidden_command};
use super::project_preview::{DeliveryDemo, PreviewConfig, ProjectPreviews, MANIFEST};
use super::{
    AgentRole, AgentRuntimeKind, MultiAgentProject, MultiAgentSnapshot, MultiAgentStore, ProjectStatus,
    TaskStatus, WorkspaceMode,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ManagedTeamMode {
    Solo,
    Pair,
    #[default]
    Team,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedTarget {
    pub root: String,
    pub branch: String,
    pub(super) base: String,
    pub planner: String,
    #[serde(default)]
    pub mode: ManagedTeamMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration_owner: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(super) members: std::collections::BTreeMap<String, ManagedMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedMember {
    runtime: AgentRuntimeKind,
    role: AgentRole,
    write_mode: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ManagedAcceptance {
    pub(super) task_id: String,
    run_id: String,
    artifact_id: String,
    pub(super) commit: String,
    acceptance_verified: bool,
    verification_command: Option<String>,
    output_tail: Option<String>,
}

impl ManagedAcceptance {
    pub(super) fn prompt(receipts: &[Self]) -> String {
        if receipts.is_empty() { return "未找到与前置交付匹配的宿主验收回执。".into(); }
        receipts.iter().map(|receipt| format!(
            "CrewVela 已在运行 {} 实际完成并通过验收，随后登记交付提交 {}（产物 {}）。命令：{}。测试输出末段：\n{}",
            receipt.run_id,receipt.commit,receipt.artifact_id,receipt.verification_command.as_deref().unwrap_or("未记录"),
            receipt.output_tail.as_deref().unwrap_or("未记录输出"))).collect::<Vec<_>>().join("\n")
    }
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
    pub final_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_acceptance: Option<FinalAcceptance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo: Option<DeliveryDemo>,
}

/// Host receipt for the fixed commit tested after all tasks finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FinalAcceptance {
    task_id: String,
    run_id: String,
    artifact_id: String,
    commit: String,
    verification_command: String,
    review_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    source_reviews: Vec<(String, String)>,
}

impl ManagedDelivery {
    pub(super) fn new(target: ManagedTarget) -> Self {
        Self {
            target,
            status: "planning".into(),
            summary: None,
            commit: None,
            verification_command: None,
            final_task_id: None,
            final_acceptance: None,
            preview: None,
            demo: None,
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
    pub(super) fn host_finalizes(&self) -> bool {
        self.mode != ManagedTeamMode::Team || self.integration_owner.is_some()
    }

    pub(super) fn reviews_exact_commits(&self) -> bool {
        self.mode == ManagedTeamMode::Pair || self.integration_owner.is_some()
    }

    pub(super) fn check_assignment(&self, id: &str, role: AgentRole, runtime: AgentRuntimeKind) -> Result<()> {
        if !self.members.is_empty() {
            ensure!(self.members.get(id).is_some_and(|grant| grant.role == role && grant.runtime == runtime),
                "托管成员的角色或 Runtime 与本轮启动授权不一致，请重新生成分工");
        }
        Ok(())
    }

    pub(super) fn is_final_assignee(&self, id: &str) -> bool {
        let role = if self.mode == ManagedTeamMode::Team { AgentRole::Integrator } else { AgentRole::Worker };
        self.members.get(id).is_some_and(|member| member.role == role)
    }

    pub(super) fn check_member(&self, member: &super::AgentProfile, usage: ManagedUse, saved: Option<&str>) -> Result<()> {
        // Legacy deliveries have no immutable per-member grant. A running older
        // delivery still checks the role at dispatch; startup never replays it.
        if self.members.is_empty() {
            let legacy = match usage {
                ManagedUse::Plan => member.runtime_kind == AgentRuntimeKind::ClaudeCode,
                ManagedUse::Review => matches!(member.runtime_kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex | AgentRuntimeKind::ZCodeCli),
                ManagedUse::Write => matches!(member.runtime_kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex),
            };
            ensure!(legacy, "历史托管分工没有对此 Runtime 和用途的授权，请重新生成分工");
        } else {
            let grant = self.members.get(&member.id).context("成员不在本轮托管授权中")?;
            ensure!(grant.runtime == member.runtime_kind && grant.role == member.role,
                "{} 的 Runtime 或团队角色已变更，请重新确认托管分工", member.name);
            if usage == ManagedUse::Write {
                ensure!(grant.write_mode.as_deref() == saved, "{} 的托管启动权限已变更", member.name);
            }
        }
        usage.authorize(member.runtime_kind, saved)
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ManagedUse {
    Plan,
    Review,
    Write,
}

impl ManagedUse {
    pub(super) fn engineering(role: AgentRole, workspace: WorkspaceMode) -> Result<Self> {
        match (role, workspace) {
            (AgentRole::Worker | AgentRole::Integrator, WorkspaceMode::Worktree) => Ok(Self::Write),
            (AgentRole::Reviewer, WorkspaceMode::ReadOnly) => Ok(Self::Review),
            _ => bail!("全自动托管只接受独立工作树开发、只读审查和独立工作树集成"),
        }
    }

    pub(super) fn supported(self, kind: AgentRuntimeKind) -> bool {
        match kind {
            AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex | AgentRuntimeKind::Pi => true,
            AgentRuntimeKind::KimiCode
            | AgentRuntimeKind::OpenCode
            | AgentRuntimeKind::ZCodeCli => super::executor::supports_managed_provider(kind, self),
            _ => false,
        }
    }

    pub(super) fn authorize(self, kind: AgentRuntimeKind, saved: Option<&str>) -> Result<()> {
        let role = match self {
            Self::Plan => "只读规划",
            Self::Review => "只读审查",
            Self::Write => "受限开发或集成",
        };
        let runtime = super::conversation::plan_runtime_label(kind.as_str());
        ensure!(
            self.supported(kind),
            "{runtime} 尚未验证全自动托管的{role}权限与原生回执"
        );
        if self != Self::Write {
            return Ok(());
        }
        match kind {
            AgentRuntimeKind::ClaudeCode => ensure!(saved == Some("auto"),
                "Claude Code 托管开发或联调需要在「管理 Runtime」中选择 Auto"),
            AgentRuntimeKind::Codex => ensure!(matches!(saved, None | Some("auto")),
                "Codex 托管开发或联调需要工作区写入沙箱和无人值守审批，请在「管理 Runtime」中选择 Approve for me"),
            _ => ensure!(!matches!(saved, Some("read_tools" | "read_only" | "plan" | "yolo")),
                "{runtime} 已保存的模式不适用于托管的受限写入，请检查「管理 Runtime」"),
        }
        Ok(())
    }

    pub(super) fn check(self, kind: AgentRuntimeKind) -> Result<()> {
        let preferences = crate::runtime_preferences::get()?;
        self.authorize(
            kind,
            preferences
                .default_launch_modes
                .get(kind.as_str())
                .map(String::as_str),
        )
    }
}

pub(super) fn prepare(
    connection: &Connection,
    input: &ConversationSendInput,
    root: String,
    branch: String,
) -> Result<ManagedTarget> {
    let preferences = crate::runtime_preferences::get()?;
    let mut planner = None;
    let mut members = std::collections::BTreeMap::new();
    let mut roles = std::collections::HashSet::new();
    for id in &input.participant_agent_ids {
        let (role, runtime): (String, String) = connection.query_row(
            "SELECT role,runtime_kind FROM ma_agents WHERE id=?1 AND project_id=?2 AND enabled=1",
            params![id, input.project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let kind: AgentRuntimeKind = serde_json::from_value(serde_json::Value::String(runtime))?;
        ensure!(
            !preferences.disabled_runtimes.contains(&kind),
            "{} 已停用，无法接收托管任务",
            super::conversation::plan_runtime_label(kind.as_str())
        );
        let usage = match role.as_str() {
            "worker" | "integrator" => ManagedUse::Write,
            "reviewer" => ManagedUse::Review,
            "orchestrator" => ManagedUse::Plan,
            _ => bail!("托管成员角色无效"),
        };
        let saved = preferences.default_launch_modes.get(kind.as_str());
        usage.authorize(kind, saved.map(String::as_str))?;
        members.insert(
            id.clone(),
            ManagedMember {
                runtime: kind,
                role: serde_json::from_value(role.clone().into())?,
                write_mode: if usage == ManagedUse::Write {
                    saved.cloned()
                } else {
                    None
                },
            },
        );
        if ManagedUse::Plan.supported(kind) {
            // Explicit coordinators take precedence. Among other members prefer a
            // planner with the project's policy-filtered research tools attached.
            let rank = (
                role != "orchestrator",
                !matches!(kind, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Pi),
            );
            if planner
                .as_ref()
                .is_none_or(|(previous, _)| rank < *previous)
            {
                planner = Some((rank, id.clone()));
            }
        }
        roles.insert(role);
    }
    let mode = match members.len() {
        0 => bail!("全自动托管至少需要一位开发成员"),
        1 => {
            ensure!(roles.contains("worker"), "本轮缺少「开发执行」成员。单人托管需要一位可写的开发成员。");
            let goal: String = connection.query_row("SELECT goal FROM ma_projects WHERE id=?1", [&input.project_id], |r| r.get(0))?;
            ensure!(!requires_review(&input.body) && !requires_review(&goal),
                "本轮请求或项目目标要求独立审查，请再选择一位「独立审查」成员。");
            ManagedTeamMode::Solo
        }
        2 => {
            ensure!(roles.contains("worker"), "本轮缺少「开发执行」成员。双人托管需要一位开发成员负责开发和联调。");
            ensure!(roles.contains("reviewer"), "本轮缺少一位「独立审查」成员。双人托管需要另一位成员负责只读审查。");
            ManagedTeamMode::Pair
        }
        _ => {
            ensure!(roles.contains("worker"), "本轮缺少「开发执行」成员。团队托管需要至少一位可写的开发成员。");
            ensure!(roles.contains("reviewer"), "本轮缺少一位「独立审查」成员。团队托管需要至少一位成员负责只读审查。");
            ManagedTeamMode::Team
        }
    };
    let integration_owner = if mode == ManagedTeamMode::Team && !roles.contains("integrator") {
        let owner = input.integration_owner.as_ref().context("请确认本轮兼任联调的开发成员")?;
        ensure!(members.get(owner).is_some_and(|member| member.role == AgentRole::Worker),
            "兼任联调的成员或权限已变更，请重新确认本轮分工");
        Some(owner.clone())
    } else {
        ensure!(input.integration_owner.is_none(), "本轮成员已变更，请重新确认联调安排");
        None
    };
    // Small teams plan with the same developer who will implement and integrate.
    if mode != ManagedTeamMode::Team {
        planner = members.iter().find(|(_, m)| m.role == AgentRole::Worker && ManagedUse::Plan.supported(m.runtime))
            .map(|(id, _)| ((false, false), id.clone()));
    }
    let (_, planner) = planner.context("请选择至少一位已验证支持只读规划的托管成员")?;
    let base = git_head(Path::new(&root))?;
    let target = ManagedTarget {
        root,
        branch,
        base,
        planner,
        mode,
        integration_owner,
        members,
    };
    target.check_before_start(&target.root)?;
    Ok(target)
}

pub(super) fn requires_review(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let terms = ["独立审查", "独立代码审查", "独立评审", "只读审查", "交叉审查", "交叉评审", "审查",
        "independent review", "independent code review", "code review", "cross-review"];
    terms.iter().any(|term| text.match_indices(term).any(|(index, term)| {
        let mut before = text[..index].trim_end();
        if (term == "审查" && ["独立", "独立代码", "只读", "交叉"].iter().any(|word| before.ends_with(word)))
            || (term == "code review" && before.ends_with("independent")) { return false; }
        for modifier in ["一次", " an", " a", "进行", "完成", "接受", "做"] {
            if let Some(prefix) = before.strip_suffix(modifier) { before = prefix.trim_end(); }
        }
        let after = text[index + term.len()..].trim_start();
        let excluded = ["无需", "无须", "不需要", "不用", "不安排", "不要求", "未安排", "不必", "跳过",
            "without", "does not require", "do not require"].iter().any(|s| before.ends_with(s));
        !excluded && (text.trim() == term || ["必须", "须", "需", "需要", "要求", "请", "安排", "经过", "须经", "必须通过", "需要通过",
            "must have", "must undergo", "require", "requires", "mandatory"].iter().any(|s| before.ends_with(s))
            || ["通过后", "通过才", "后再合", "后才能", "后方可", "是必需", "is required", "required"]
                .iter().any(|s| after.starts_with(s)))
    }))
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
    pub(super) fn managed_acceptance_receipts(
        &self,
        artifacts: &[super::ArtifactRecord],
    ) -> Result<Vec<ManagedAcceptance>> {
        let connection = self.connection.lock().unwrap();
        let mut query = connection.prepare(
            "SELECT COALESCE(json_extract(e.payload_json,'$.verificationCommand'),
                (SELECT substr(content,length('运行验收命令：')+1) FROM ma_run_logs
                 WHERE run_id=r.id AND stream='system' AND content LIKE '运行验收命令：%' ORDER BY seq DESC LIMIT 1)),
                (SELECT substr(content,-1600) FROM ma_run_logs WHERE run_id=r.id AND stream='verification' ORDER BY seq DESC LIMIT 1)
             FROM ma_artifacts a JOIN ma_runs r ON r.id=a.run_id
             JOIN ma_runtime_events e ON e.run_id=r.id AND e.task_id=a.task_id AND e.agent_id=r.agent_id
               AND e.event_type='completed' AND e.created_at=a.created_at AND e.created_at=r.finished_at
             WHERE a.id=?1 AND a.project_id=?2 AND a.task_id=?3 AND a.run_id=?4 AND a.content_hash=?5
               AND a.artifact_type='git_commit' AND r.status='succeeded'
               AND json_extract(e.payload_json,'$.artifactId')=a.id AND json_type(e.payload_json,'$.acceptanceVerified')='true'")?;
        let mut receipts = Vec::new();
        for artifact in artifacts
            .iter()
            .filter(|artifact| artifact.artifact_type == "git_commit")
        {
            let Some(commit) = artifact
                .content_hash
                .as_ref()
                .filter(|sha| super::executor::valid_commit_hash(sha))
            else {
                continue;
            };
            let evidence = query
                .query_row(
                    params![
                        artifact.id,
                        artifact.project_id,
                        artifact.task_id,
                        artifact.run_id,
                        commit
                    ],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                        ))
                    },
                )
                .optional()?;
            if let Some((command, output)) = evidence {
                receipts.push(ManagedAcceptance {
                    task_id: artifact.task_id.clone(),
                    run_id: artifact.run_id.clone(),
                    artifact_id: artifact.id.clone(),
                    commit: commit.clone(),
                    acceptance_verified: true,
                    verification_command: command,
                    output_tail: output,
                });
            }
        }
        Ok(receipts)
    }
    pub(super) fn managed_planner_target(
        &self,
        task: &str,
        agent: &str,
    ) -> Result<Option<ManagedTarget>> {
        let connection = self.connection.lock().unwrap();
        let selected: Option<String> = connection
            .query_row(
                "SELECT json_extract(r.data_json,'$.managedTarget') FROM ma_conversation_turns t
             JOIN ma_conversation_rounds r ON r.id=t.round_id WHERE t.task_id=?1",
                [task],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let target: Option<ManagedTarget> =
            selected.map(|raw| serde_json::from_str(&raw)).transpose()?;
        ensure!(
            target.as_ref().is_none_or(|target| target.planner == agent),
            "该成员未获本轮只读规划授权"
        );
        Ok(target)
    }

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
        Self::finish_managed_plan_in(&tx, plan_id, result)?;
        tx.commit()?;
        Ok(())
    }

    fn finish_managed_plan_in(tx: &Connection, plan_id: &str, result: Result<(String, String, String), String>) -> Result<()> {
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
                let declared_demo = plan.tasks.iter().any(|task| task.write_paths.iter().any(|path| path == MANIFEST));
                match PreviewConfig::from_delivery(Path::new(&managed.target.root), &hash) {
                    Ok(Some(config)) => {
                        managed.preview = Some(config);
                        managed.demo = Some(DeliveryDemo::Preparing);
                    }
                    Ok(None) if !declared_demo => {}
                    Ok(None) => managed.demo = Some(DeliveryDemo::Unavailable { error: format!("交付提交缺少 {MANIFEST}，请让联调成员补齐并验证启动方式。") }),
                    Err(error) => managed.demo = Some(DeliveryDemo::Unavailable { error: format!("交付启动配置无效：{error:#}") }),
                }
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
        Ok(())
    }

    fn final_candidate<'a>(
        &self, snapshot: &'a MultiAgentSnapshot, plan: &ConversationExecutionPlan,
    ) -> Result<(&'a super::MultiAgentTask, &'a super::ArtifactRecord, FinalAcceptance)> {
        let managed = plan.managed.as_ref().context("缺少托管授权")?;
        let final_id = managed.final_task_id.as_ref().context("缺少本轮最终交付任务标识")?;
        let pair = managed.target.mode == ManagedTeamMode::Pair;
        let flexible = managed.target.integration_owner.is_some();
        ensure!(plan.task_ids.len() == plan.tasks.len() && (flexible || plan.tasks.len() == 1 + usize::from(pair)),
            "最终交付与已授权分工不一致");
        let latest_delivery = |id: &str, role: AgentRole, workspace, artifact_type| -> Result<_> {
            let index = plan.task_ids.iter().position(|value| value == id).context("交付任务不属于本轮授权")?;
            let planned = &plan.tasks[index];
            let task = snapshot.tasks.iter().find(|t| t.id == id && t.project_id == plan.project_id).context("缺少交付任务")?;
            ensure!(task.status == TaskStatus::Done && task.workspace_mode == workspace
                && planned.workspace_mode == workspace && task.assignee_agent_id.as_ref() == Some(&planned.assignee_agent_id)
                && task.verification_command == planned.verification_command, "交付任务或验收命令与原分工不一致");
            let member = snapshot.agents.iter().find(|a| a.id == planned.assignee_agent_id && a.enabled && a.role == role)
                .context("负责交付或审查的成员已变更")?;
            let preferences = crate::runtime_preferences::get()?;
            ensure!(!preferences.disabled_runtimes.contains(&member.runtime_kind)
                && !snapshot.agent_sessions.iter().any(|s| s.agent_id == member.id && s.control_mode == super::AgentControlMode::Manual),
                "{} 已停用或正在由用户接管", member.name);
            managed.target.check_member(member, ManagedUse::engineering(role, workspace)?,
                preferences.default_launch_modes.get(member.runtime_kind.as_str()).map(String::as_str))?;
            let run = snapshot.runs.iter().filter(|r| r.task_id == id).max_by_key(|r| r.attempt)
                .filter(|r| r.status == super::RunStatus::Succeeded && r.agent_id == member.id).context("缺少最后一次成功交付运行")?;
            let artifact = snapshot.artifacts.iter().find(|a| a.run_id == run.id && a.task_id == id && a.artifact_type == artifact_type)
                .context("缺少本次运行的交付产物")?;
            Ok((task, run, artifact))
        };
        let (task, run, artifact) = latest_delivery(final_id, AgentRole::Worker, WorkspaceMode::Worktree, "git_commit")?;
        if let Some(owner) = &managed.target.integration_owner {
            ensure!(&run.agent_id == owner, "最终交付负责人已偏离授权");
        }
        let receipts = self.managed_acceptance_receipts(std::slice::from_ref(artifact))?;
        let [receipt] = receipts.as_slice() else { bail!("候选提交缺少开发任务的宿主测试回执") };
        let command = task.verification_command.clone().context("缺少成品验收命令")?;
        ensure!(receipt.verification_command.as_ref() == Some(&command), "开发测试回执与本轮验收命令不一致");
        let approve = |review_id: &String, target_id: &String, writer_id: &str, commit: &str| -> Result<String> {
            let (_, review_run, review_artifact) = latest_delivery(review_id, AgentRole::Reviewer, WorkspaceMode::ReadOnly, "review")?;
            let host_artifact: bool = self.connection.lock().unwrap().query_row("SELECT EXISTS(
                SELECT 1 FROM ma_runtime_events e JOIN ma_artifacts a ON a.id=json_extract(e.payload_json,'$.artifactId')
                JOIN ma_runs r ON r.id=e.run_id WHERE r.id=?1 AND a.id=?2 AND a.run_id=r.id AND e.task_id=a.task_id
                AND e.agent_id=r.agent_id AND e.created_at=r.finished_at AND e.created_at=a.created_at AND e.event_type='completed')",
                params![review_run.id,review_artifact.id], |r| r.get(0))?;
            ensure!(host_artifact && review_run.agent_id != writer_id && review_artifact.content_hash.as_deref() == Some(commit)
                && snapshot.review_decisions.iter().any(|d| d.review_task_id == *review_id && d.review_run_id == review_run.id
                    && d.target_task_id == *target_id && d.reviewer_agent_id == review_run.agent_id && d.applied_at.is_some()
                    && d.verdict == super::ReviewVerdict::Approved), "最终候选提交尚未获得对应的独立只读审查通过回执");
            Ok(review_run.id.clone())
        };
        let review_for = |key: &str| -> Result<&String> {
            let mut ids = plan.tasks.iter().zip(&plan.task_ids).filter(|(t, _)| {
                t.workspace_mode == WorkspaceMode::ReadOnly && t.depends_on.as_slice() == [key]
            }).map(|(_, id)| id);
            let id = ids.next().context("缺少候选提交的独立审查任务")?;
            ensure!(ids.next().is_none(), "对应候选提交的审查任务不唯一");
            Ok(id)
        };
        let review_run_id = if pair || flexible {
            let key = &plan.tasks[plan.task_ids.iter().position(|id| id == final_id).context("最终任务未登记")?].key;
            Some(approve(review_for(key)?, final_id, &run.agent_id, &receipt.commit)?)
        } else { None };
        let mut source_reviews = Vec::new();
        if flexible {
            for (planned, id) in plan.tasks.iter().zip(&plan.task_ids) {
                if id == final_id || planned.workspace_mode != WorkspaceMode::Worktree { continue; }
                let (_, source, source_artifact) = latest_delivery(id, AgentRole::Worker, WorkspaceMode::Worktree, "git_commit")?;
                let source_receipts = self.managed_acceptance_receipts(std::slice::from_ref(source_artifact))?;
                let [source_receipt] = source_receipts.as_slice() else { bail!("模块缺少宿主验收回执"); };
                let approved = approve(review_for(&planned.key)?, id, &source.agent_id, &source_receipt.commit)?;
                source_reviews.push((source_artifact.id.clone(), approved));
            }
            let mut expected = source_reviews.iter().map(|(artifact, _)| artifact).collect::<Vec<_>>();
            expected.sort();
            ensure!(!expected.is_empty() && run.session_locator.as_ref().and_then(|locator| locator.get("managedInputArtifacts")) == Some(&json!(expected)),
                "最终候选使用的模块交付与当前已审查提交不一致");
        }
        Ok((task, artifact, FinalAcceptance { task_id: final_id.clone(), run_id: run.id.clone(),
            artifact_id: artifact.id.clone(), commit: receipt.commit.clone(), verification_command: command, review_run_id, source_reviews }))
    }

    fn verify_final_delivery(
        &self, snapshot: &MultiAgentSnapshot, plan: &ConversationExecutionPlan, cancelled: &AtomicBool,
    ) -> Result<FinalAcceptance> {
        let managed = plan.managed.as_ref().context("缺少托管授权")?;
        let (task, artifact, candidate) = self.final_candidate(snapshot, plan)?;
        let root = managed.target.check(&snapshot.project.root_path)?;
        let head = git_head(&root)?;
        ensure!(head == managed.target.base || (head == candidate.commit && managed.final_acceptance.as_ref() == Some(&candidate)),
            "交付分支已移动或缺少对应的最终验收回执");
        let cwd = PathBuf::from(&artifact.uri);
        let final_scopes = self.managed_write_paths(&task.id)?;
        let mut scopes = final_scopes.clone();
        if managed.target.integration_owner.is_some() {
            for (planned, id) in plan.tasks.iter().zip(&plan.task_ids) {
                if id != &task.id && planned.workspace_mode == WorkspaceMode::Worktree {
                    scopes.extend(self.managed_write_paths(id)?);
                }
            }
        }
        let check_workspace = || super::executor::check_final_candidate(&snapshot.project, &cwd, &managed.target.base, &candidate.commit, &scopes);
        check_workspace()?;
        if let Some(saved) = &managed.final_acceptance {
            ensure!(saved == &candidate, "已记录的最终验收与当前候选提交或审查运行不一致");
            return Ok(candidate);
        }
        if let (_, Some(entry)) = verification(&candidate.verification_command)? {
            ensure!(!git(&cwd, &["ls-tree", &candidate.commit, "--", entry])?.is_empty(), "验收测试入口必须已提交到最终候选版本");
        }
        if final_scopes.iter().any(|path| path == MANIFEST) {
            PreviewConfig::from_delivery(&cwd, &candidate.commit)?.with_context(|| format!("网页成品缺少 {MANIFEST}"))?;
        }
        self.append_run_log(&candidate.run_id, "system", &format!("最终候选提交验收：{}", candidate.commit))?;
        ensure!(super::executor::verify_task_acceptance(task, &cwd, cancelled, Some((self, &candidate.run_id)))?,
            "最终候选提交未通过宿主验收");
        check_workspace()?;
        ensure!(!cancelled.load(Ordering::Acquire), "最终验收已停止");
        let saved = self.connection.lock().unwrap().execute(
            "UPDATE ma_conversation_plans SET data_json=json_set(data_json,'$.managed.finalAcceptance',json(?2))
             WHERE id=?1 AND applied_request_id='managed-'||id AND json_extract(data_json,'$.managed.status')='running'
             AND EXISTS (SELECT 1 FROM ma_projects WHERE id=ma_conversation_plans.project_id AND status='active')",
            params![plan.id, serde_json::to_string(&candidate)?])?;
        ensure!(saved == 1, "本轮托管已暂停或结束");
        Ok(candidate)
    }

    pub(super) fn settle_idle_managed_plan(
        &self,
        snapshot: &MultiAgentSnapshot,
        plan: &ConversationExecutionPlan,
        previews: &ProjectPreviews,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let managed = plan.managed.as_ref().context("缺少托管授权")?;
        let host_finalizes = managed.target.host_finalizes();
        if host_finalizes && (cancelled.load(Ordering::Acquire) || self.project(&plan.project_id)?.status != ProjectStatus::Active) {
            return Ok(());
        }
        let mut accepted_candidate = None;
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
            if host_finalizes {
                let candidate = self.verify_final_delivery(snapshot, plan, cancelled)?;
                let summary = snapshot.runs.iter().find(|run| run.id == candidate.run_id)
                    .and_then(|run| run.output_summary.clone()).unwrap_or_default();
                let result = (candidate.commit.clone(), candidate.verification_command.clone(), summary);
                accepted_candidate = Some(candidate);
                return Ok(result);
            }
            let final_task = tasks
                .iter()
                .find(|task| {
                    if let Some(id) = &managed.final_task_id { return task.id == *id; }
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
        let accepted = if host_finalizes {
            let mut connection = self.connection.lock().unwrap();
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current: Option<String> = tx.query_row("SELECT p.data_json FROM ma_conversation_plans p
                JOIN ma_projects j ON j.id=p.project_id WHERE p.id=?1 AND p.applied_request_id='managed-'||p.id
                AND j.status='active' AND json_extract(p.data_json,'$.managed.status')='running'",
                [&plan.id], |r| r.get(0)).optional()?;
            let Some(raw) = current.filter(|_| !cancelled.load(Ordering::Acquire)) else { return Ok(()); };
            let result = result.and_then(|delivery| {
                let current: ConversationExecutionPlan = serde_json::from_str(&raw)?;
                let grant = current.managed.context("最终交付授权缺失")?;
                ensure!(grant.final_acceptance.is_some() && grant.final_acceptance == accepted_candidate, "最终验收回执已变更");
                let (root, branch, goal): (String, String, String) = tx.query_row("SELECT root_path,default_branch,goal FROM ma_projects WHERE id=?1",
                    [&plan.project_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                ensure!(root == snapshot.project.root_path && branch == grant.target.branch, "项目交付目录或目标分支已变更");
                ensure!(grant.target.mode != ManagedTeamMode::Solo || !requires_review(&goal), "当前项目目标要求独立审查");
                grant.target.merge(&snapshot.project, &delivery.0)?;
                Ok(delivery)
            });
            let accepted = result.is_ok();
            Self::finish_managed_plan_in(&tx, &plan.id, result.map_err(|e| format!("{e:#}")))?;
            Self::set_project_status_in(&tx, &snapshot.project.id, if accepted { ProjectStatus::Completed } else { ProjectStatus::Paused })?;
            tx.commit()?;
            accepted
        } else {
            let accepted = result.is_ok();
            self.finish_managed_plan(&plan.id, result.map_err(|error| format!("{error:#}")))?;
            self.set_project_status(&snapshot.project.id, if accepted { ProjectStatus::Completed } else { ProjectStatus::Paused })?;
            accepted
        };
        if accepted {
            previews.prepare_delivery(self, &snapshot.project.id, &plan.id)?;
        }
        Ok(())
    }

    fn check_recovery(
        &self,
        plan: &ConversationExecutionPlan,
        receipt: Option<&str>,
    ) -> Result<()> {
        let managed = plan.managed.as_ref().context("分工缺少托管授权")?;
        ensure!(
            managed.status == "running"
                && plan.status == "applied"
                && receipt == Some(format!("managed-{}", plan.id).as_str())
                && !plan.task_ids.is_empty(),
            "工程分工尚未登记为已授权执行"
        );
        ensure!(
            !managed.target.members.is_empty(),
            "历史分工缺少可核对的成员权限快照"
        );
        let snapshot = self.snapshot(&plan.project_id)?;
        ensure!(
            snapshot.project.status == ProjectStatus::Active
                && snapshot.project.default_branch == managed.target.branch,
            "项目已暂停、结束或更换交付分支"
        );
        let root = managed.target.check(&snapshot.project.root_path)?;
        let head = git_head(&root)?;
        let preferences = crate::runtime_preferences::get()?;
        let mut merged = None;
        let mut all_done = true;
        for id in &plan.task_ids {
            let task = snapshot
                .tasks
                .iter()
                .find(|task| &task.id == id)
                .context("托管任务记录缺失")?;
            let member = snapshot
                .agents
                .iter()
                .find(|a| Some(&a.id) == task.assignee_agent_id.as_ref() && a.enabled)
                .context("托管成员已停用或发生变更")?;
            ensure!(
                !preferences.disabled_runtimes.contains(&member.runtime_kind)
                    && !snapshot
                        .agent_sessions
                        .iter()
                        .any(|s| s.agent_id == member.id
                            && s.control_mode == super::AgentControlMode::Manual),
                "{} 的 Runtime 已停用或成员正在由用户接管",
                member.name
            );
            let usage = ManagedUse::engineering(member.role, task.workspace_mode)?;
            managed.target.check_member(
                member,
                usage,
                preferences
                    .default_launch_modes
                    .get(member.runtime_kind.as_str())
                    .map(String::as_str),
            )?;
            let runs: Vec<_> = snapshot
                .runs
                .iter()
                .filter(|run| &run.task_id == id)
                .collect();
            ensure!(
                !runs.iter().any(|run| run
                    .session_locator
                    .as_ref()
                    .is_some_and(|loc| loc["stopUnverified"] == true)),
                "{} 的原生进程停止未确认，未重复派发",
                task.title
            );
            all_done &= task.status == TaskStatus::Done;
            let Some(last) = runs.into_iter().max_by_key(|run| run.attempt) else {
                ensure!(
                    matches!(
                        task.status,
                        TaskStatus::Draft | TaskStatus::Ready | TaskStatus::Blocked
                    ),
                    "{} 缺少与当前状态匹配的执行记录",
                    task.title
                );
                continue;
            };
            ensure!(
                last.agent_id == member.id
                    && matches!(
                        last.status,
                        super::RunStatus::Succeeded | super::RunStatus::Failed
                    ),
                "{} 上一次进程或验收未持久结算，未重复派发",
                task.title
            );
            let loc = last.session_locator.as_ref().context("缺少原生会话回执")?;
            ensure!(
                loc["adapter"] == member.runtime_kind.as_str()
                    && matches!(
                        loc["managedTurn"].as_str(),
                        Some("completed" | "failed" | "interrupted" | "rejected")
                    )
                    && (loc["sessionId"].is_string() || loc["threadId"].is_string()),
                "{} 的原生终态或身份不可核实",
                task.title
            );
            let cwd = Path::new(loc["cwd"].as_str().context("原工作树位置缺失")?);
            let (repo, _) = super::executor::verify_delivery_worktree(&snapshot.project, cwd)?;
            let owner = crate::providers::config_dir()?
                .join("worktrees")
                .join(&plan.project_id)
                .join(&task.id)
                .canonicalize()?;
            ensure!(
                repo.parent() == Some(owner.as_path()),
                "原工作树不属于该托管任务"
            );
            match last.status {
                super::RunStatus::Succeeded if task.workspace_mode == WorkspaceMode::Worktree => {
                    let artifact = snapshot
                        .artifacts
                        .iter()
                        .find(|a| a.run_id == last.id && a.artifact_type == "git_commit")
                        .context("成功写入的原验收提交缺失")?;
                    ensure!(
                        !self
                            .managed_acceptance_receipts(std::slice::from_ref(artifact))?
                            .is_empty(),
                        "{} 缺少与该交付提交匹配的宿主验收通过回执",
                        task.title
                    );
                    let sha = artifact
                        .content_hash
                        .as_deref()
                        .context("原交付缺少提交标识")?;
                    ensure!(
                        git_head(cwd)? == sha
                            && git(cwd, &["status", "--porcelain", "--untracked-files=all"])?
                                .is_empty(),
                        "{} 的工作树已偏离原验收提交",
                        task.title
                    );
                    if !managed.target.host_finalizes() && member.role == AgentRole::Integrator {
                        merged = Some(sha.to_owned());
                    }
                }
                super::RunStatus::Failed => ensure!(
                    task.status == TaskStatus::Ready && last.attempt < task.max_attempts,
                    "{} 不在已批准的重试预算中",
                    task.title
                ),
                _ => {}
            }
        }
        if all_done && managed.target.host_finalizes() {
            let (_, _, candidate) = self.final_candidate(&snapshot, plan)?;
            if let Some(accepted) = &managed.final_acceptance {
                ensure!(accepted == &candidate, "已登记的最终验收与当前交付或审查不符");
            }
            ensure!(head == managed.target.base ||
                (head == candidate.commit && managed.final_acceptance.as_ref() == Some(&candidate)),
                "授权分支与基线、已验收的最终候选提交不一致");
        } else if all_done {
            ensure!(
                merged.as_deref() == Some(&head),
                "授权分支并未指向已登记的集成验收提交"
            );
        } else {
            ensure!(
                head == managed.target.base && merged.is_none(),
                "交付分支已移动，原合入尚未完整结算"
            );
        }
        Ok(())
    }

    pub(crate) fn recover_managed_deliveries_at_startup(&self) -> Result<Vec<String>> {
        let plans = {
            let connection = self.connection.lock().unwrap();
            connection.execute(
                "UPDATE ma_conversation_plans SET data_json=json_set(data_json,'$.managed.demo',json_object('state','unavailable','error',?1))
                 WHERE json_extract(data_json,'$.managed.demo.state')='preparing'",
                ["应用重启中断了 Demo 启动，可以重新打开 Demo。"],
            )?;
            let mut statement = connection.prepare("SELECT data_json,applied_request_id FROM ma_conversation_plans WHERE json_extract(data_json,'$.managed.status') IN ('planning','running')")?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let mut resumable = Vec::new();
        for (raw, receipt) in plans {
            let plan: ConversationExecutionPlan = serde_json::from_str(&raw)?;
            match self.check_recovery(&plan, receipt.as_deref()) {
                Ok(()) => resumable.push(plan.project_id),
                Err(error) => {
                    self.finish_managed_plan(
                        &plan.id,
                        Err(format!(
                            "重启后无法安全继续：{error:#}。已保留任务与工作树记录。"
                        )),
                    )?;
                    if self.project(&plan.project_id)?.status == ProjectStatus::Active {
                        self.set_project_status(&plan.project_id, ProjectStatus::Paused)?;
                    }
                }
            }
        }
        Ok(resumable)
    }

    pub(crate) fn fail_recovered_managed_project(
        &self,
        project: &str,
        error: &anyhow::Error,
    ) -> Result<()> {
        if let Some(plan) = self.managed_plan_running(project)? {
            self.finish_managed_plan(&plan.id, Err(format!("重启后未能恢复调度：{error:#}")))?;
            if self.project(project)?.status == ProjectStatus::Active {
                self.set_project_status(project, ProjectStatus::Paused)?;
            }
        }
        Ok(())
    }
}

// 工作流模板（最小验证版）：硬编码「从 0 到 1 · Vibecoding」流水线，
// 实例化 = 向现有 MultiAgentStore 写 agents + tasks + dependencies，
// 执行、调度、干预全部复用既有机制。人工卡点通过 reviewer_agent_id 实现：
// 带该字段的任务完成后进入 in_review（finalize 语义），由人在任务图上
// 批准（ma_approve_task）后置 done 并解锁下游依赖，零内核改动。

use anyhow::{bail, Context, Result};
use serde::Serialize;

use super::domain::{
    AddDependencyInput, AgentPersona, AgentRole, AgentRuntimeKind, CreateAgentInput,
    CreateProjectInput, CreateTaskInput, PermissionProfile, WorkspaceMode,
};
use super::MultiAgentStore;

pub(crate) struct TemplateAgent {
    pub(crate) key: &'static str,
    pub(crate) name: &'static str,
    pub(crate) runtime: AgentRuntimeKind,
    pub(crate) role: AgentRole,
    pub(crate) description: &'static str,
    pub(crate) persona: AgentPersona,
}

pub(crate) struct TemplateTask {
    pub(crate) key: &'static str,
    pub(crate) stage: u32,
    pub(crate) title: &'static str,
    /// {goal} 会被项目目标替换。
    pub(crate) description: &'static str,
    pub(crate) acceptance: &'static str,
    pub(crate) agent_key: &'static str,
    pub(crate) depends_on: &'static [&'static str],
    /// 卡点：Some(复核 Agent key) 表示完成后停 in_review 等人工批准
    /// （机制 = reviewer_agent_id 挂靠该复核 Agent）。
    pub(crate) gate: Option<&'static str>,
    pub(crate) workspace: WorkspaceMode,
}

pub struct WorkflowTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub(crate) agents: &'static [TemplateAgent],
    pub(crate) tasks: &'static [TemplateTask],
}

/// 给前端的模板元数据（含环节预览，不含执行细节）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTemplateView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub agent_count: usize,
    pub stages: Vec<WorkflowStageView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStageView {
    pub stage: u32,
    pub title: String,
    pub parallel: bool,
    pub human_gate: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInstantiateReceipt {
    pub project_id: String,
    pub template_id: String,
    pub created_agents: usize,
    pub created_tasks: usize,
}

static VIBECODING_AGENTS: &[TemplateAgent] = &[
    TemplateAgent {
        key: "architect",
        name: "Claude 架构师",
        runtime: AgentRuntimeKind::ClaudeCode,
        role: AgentRole::Orchestrator,
        description: "负责产品调研、撰写 PRD 与架构设计，输出 docs/ 下的决策文档。",
    
    persona: AgentPersona::Rigorous,},
    TemplateAgent {
        key: "designer",
        name: "OpenCode 设计师",
        runtime: AgentRuntimeKind::OpenCode,
        role: AgentRole::Worker,
        description: "负责界面视觉与交互设计，产出可实施的设计说明。",
    
    persona: AgentPersona::Innovative,},
    TemplateAgent {
        key: "frontend",
        name: "Codex 前端工程师",
        runtime: AgentRuntimeKind::Codex,
        role: AgentRole::Worker,
        description: "负责前端页面与交互实现。",
    
    persona: AgentPersona::Efficient,},
    TemplateAgent {
        key: "backend",
        name: "Kimi 后端工程师",
        runtime: AgentRuntimeKind::KimiCode,
        role: AgentRole::Worker,
        description: "负责服务端、数据模型与接口实现。",
    
    persona: AgentPersona::Efficient,},
    TemplateAgent {
        key: "critic",
        name: "Pi 审查工程师",
        runtime: AgentRuntimeKind::Pi,
        role: AgentRole::Worker,
        description: "审查前后端代码，只修改审查报告，产出问题清单与整改建议，不修改实现文件。",
    
    persona: AgentPersona::Rigorous,},
    TemplateAgent {
        key: "tester",
        name: "Codex 测试工程师",
        runtime: AgentRuntimeKind::Codex,
        role: AgentRole::Worker,
        description: "负责测试设计、用例执行与回归验证。",
    
    persona: AgentPersona::Rigorous,},
    TemplateAgent {
        key: "ops",
        name: "OpenCode 交付工程师",
        runtime: AgentRuntimeKind::OpenCode,
        role: AgentRole::Integrator,
        description: "负责构建、部署与最终交付。",
    
    persona: AgentPersona::Concise,},
];

static VIBECODING_TASKS: &[TemplateTask] = &[
    TemplateTask {
        key: "research",
        stage: 1,
        title: "产品调研",
        agent_key: "architect",
        description: "围绕目标「{goal}」做产品调研：同类产品与竞品分析、目标用户与核心场景、差异化切入点。把结论写入 docs/research.md（含要点清单与风险）。",
        acceptance: "docs/research.md 存在，包含竞品、用户场景、差异化三部分，不少于 500 字。",
        depends_on: &[],
        gate: None,
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "prd",
        stage: 2,
        title: "撰写 PRD",
        agent_key: "architect",
        description: "基于 docs/research.md，为目标「{goal}」撰写 PRD 写入 docs/PRD.md：产品概述、用户故事、功能范围（MVP 与后续）、非功能约束、里程碑与验收标准。",
        acceptance: "docs/PRD.md 存在，功能范围有明确的 MVP 边界，每个功能有可验证的验收标准。",
        depends_on: &["research"],
        // 人工卡点：PRD 定稿由审查工程师复核、人工放行后再进入设计与开发，避免方向跑偏烧额度。
        gate: Some("critic"),
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "architecture",
        stage: 3,
        title: "架构设计",
        agent_key: "architect",
        description: "依据 docs/PRD.md 做技术架构设计，写入 docs/architecture.md：技术选型与理由、模块划分、数据模型、接口契约、目录结构约定。",
        acceptance: "docs/architecture.md 存在，模块与接口划分完整，前后端目录结构可直接开工。",
        depends_on: &["prd"],
        gate: None,
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "ui",
        stage: 4,
        title: "UI 设计",
        agent_key: "designer",
        description: "依据 docs/PRD.md 完成界面设计，写入 docs/ui-design.md：信息结构、关键页面布局与组件说明、视觉基调（色彩/字体/间距）、响应式要求。",
        acceptance: "docs/ui-design.md 存在，覆盖 PRD 的全部 MVP 页面，组件描述可被前端直接实现。",
        depends_on: &["prd"],
        gate: None,
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "frontend",
        stage: 5,
        title: "前端开发",
        agent_key: "frontend",
        description: "按 docs/architecture.md 与 docs/ui-design.md 实现前端全部 MVP 功能，与后端接口契约对齐。",
        acceptance: "前端可构建通过，PRD 中 MVP 页面全部可访问，无阻断性报错。",
        depends_on: &["architecture", "ui"],
        gate: None,
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "backend",
        stage: 5,
        title: "后端开发",
        agent_key: "backend",
        description: "按 docs/architecture.md 的数据模型与接口契约实现后端 MVP，提供可运行入口。",
        acceptance: "后端可构建并通过接口冒烟，PRD 中 MVP 接口全部实现。",
        depends_on: &["architecture"],
        gate: None,
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "review",
        stage: 6,
        title: "代码审核",
        agent_key: "critic",
        description: "审查前端与后端交付：与 PRD/架构的一致性、明显缺陷、安全隐患。仅允许新建或修改 docs/review.md，把问题清单按阻断/建议分级；不得修改实现文件或其他文件。",
        acceptance: "docs/review.md 存在且区分阻断与建议两级；无阻断级问题时明确说明；本任务的交付变更仅包含 docs/review.md，未修改实现文件。",
        depends_on: &["frontend", "backend"],
        // 人工卡点：审核报告由架构师代项目复核、人工放行后才能进入测试与部署。
        gate: Some("architect"),
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "testing",
        stage: 7,
        title: "测试验证",
        agent_key: "tester",
        description: "依据 docs/PRD.md 验收标准与 docs/review.md 整改结果执行验证：核心路径用例、构建与运行检查，结果写入 docs/test-report.md。",
        acceptance: "docs/test-report.md 存在，逐条对照 PRD 验收标准给出通过/未通过结论。",
        depends_on: &["review"],
        gate: None,
        workspace: WorkspaceMode::Worktree,
    },
    TemplateTask {
        key: "deploy",
        stage: 8,
        title: "部署交付",
        agent_key: "ops",
        description: "在独立 Worktree 整合全部交付：确认构建产物可用、编写 docs/DEPLOY.md（本地运行步骤与依赖），提交最终集成产物；等待人工批准后由系统合并，不直接修改主工作区或自行合并。",
        acceptance: "按 docs/DEPLOY.md 可从零启动项目；构建产物齐备，提交可审查的隔离交付并等待人工批准合并。",
        depends_on: &["testing"],
        gate: None,
        workspace: WorkspaceMode::Worktree,
    },
];

static VIBECODING: WorkflowTemplate = WorkflowTemplate {
    id: "vibecoding",
    name: "从 0 到 1 · Vibecoding",
    description: "调研 → PRD（人工确认）→ 架构/UI 设计 → 前后端并行开发 → 代码审核（人工确认）→ 测试 → 部署交付的完整产品流水线。",
    agents: VIBECODING_AGENTS,
    tasks: VIBECODING_TASKS,
};

static TEMPLATES: &[&WorkflowTemplate] = &[&VIBECODING];

/// 供工作流画布投影内置模板，避免画布与模板两份内容漂移。
pub(crate) fn vibecoding() -> &'static WorkflowTemplate {
    &VIBECODING
}

fn template(id: &str) -> Result<&'static WorkflowTemplate> {
    TEMPLATES
        .iter()
        .find(|item| item.id == id)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("未找到工作流模板：{id}"))
}

pub fn list() -> Vec<WorkflowTemplateView> {
    TEMPLATES
        .iter()
        .map(|item| WorkflowTemplateView {
            id: item.id.to_string(),
            name: item.name.to_string(),
            description: item.description.to_string(),
            agent_count: item.agents.len(),
            stages: stage_views(item),
        })
        .collect()
}

fn stage_views(item: &WorkflowTemplate) -> Vec<WorkflowStageView> {
    let mut stages: Vec<WorkflowStageView> = Vec::new();
    for task in item.tasks {
        let existing = stages.iter_mut().find(|stage| stage.stage == task.stage);
        match existing {
            Some(stage) => {
                stage.parallel = true;
                stage.human_gate = stage.human_gate || task.gate.is_some();
            }
            None => stages.push(WorkflowStageView {
                stage: task.stage,
                title: task.title.to_string(),
                parallel: false,
                human_gate: task.gate.is_some(),
            }),
        }
    }
    stages
}

/// 把模板实例化为多 Agent 项目。root_path 必须是已存在目录；
/// 项目目标会替换任务描述里的 {goal} 占位符。
pub fn instantiate(
    store: &MultiAgentStore,
    template_id: &str,
    name: &str,
    goal: &str,
    root_path: &str,
) -> Result<WorkflowInstantiateReceipt> {
    let item = template(template_id)?;
    let mut runtime_leases = Vec::new();
    for agent in item.agents {
        runtime_leases.push(crate::runtime_preferences::acquire(agent.runtime)?);
    }
    let name = name.trim();
    let goal = goal.trim();
    if name.is_empty() {
        bail!("项目名称不能为空");
    }
    if goal.is_empty() {
        bail!("项目目标不能为空");
    }
    if !std::path::Path::new(root_path.trim()).is_dir() {
        bail!("项目目录不存在：{root_path}");
    }
    let fill = |text: &str| text.replace("{goal}", goal);

    let project = store.create_project(CreateProjectInput {
        name: name.to_string(),
        goal: goal.to_string(),
        root_path: root_path.trim().to_string(),
        default_branch: "main".to_string(),
    })?;

    let mut agent_ids = std::collections::HashMap::new();
    for agent in item.agents {
        let created = store.create_agent(CreateAgentInput {
            avatar_id: None,
            project_id: project.id.clone(),
            name: agent.name.to_string(),
            runtime_kind: agent.runtime,
            role: agent.role,
            description: agent.description.to_string(),
            persona: agent.persona,
            persona_custom: String::new(),
            model: None,
            // 写入型任务需要自动权限推进；Worktree 隔离兜底安全。
            permission_profile: PermissionProfile::Auto,
            max_concurrency: 1,
        })?;
        agent_ids.insert(agent.key, created.id);
    }

    // 卡点任务的 reviewer_agent_id 挂靠模板声明的复核 Agent：finalize 语义下
    // 带 reviewer 的任务完成后进入 in_review，等待人工批准。
    let mut task_ids = std::collections::HashMap::new();
    for task in item.tasks {
        let assignee = agent_ids
            .get(task.agent_key)
            .context("模板内部错误：任务引用了未知 Agent")?;
        let reviewer = match task.gate {
            Some(key) => Some(
                agent_ids
                    .get(key)
                    .with_context(|| format!("模板内部错误：卡点引用了未知复核 Agent {key}"))?
                    .clone(),
            ),
            None => None,
        };
        let created = store.create_task(CreateTaskInput {
            project_id: project.id.clone(),
            parent_id: None,
            stage: task.stage,
            title: task.title.to_string(),
            description: fill(task.description),
            acceptance_criteria: fill(task.acceptance),
            verification_command: None,
            assignee_agent_id: Some(assignee.clone()),
            reviewer_agent_id: reviewer,
            workspace_mode: task.workspace,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 60,
        })?;
        task_ids.insert(task.key, created.id);
    }

    for task in item.tasks {
        let task_id = &task_ids[task.key];
        for dependency in task.depends_on {
            store.add_dependency(AddDependencyInput {
                task_id: task_id.clone(),
                depends_on_task_id: task_ids[dependency].clone(),
            })?;
        }
    }

    Ok(WorkflowInstantiateReceipt {
        project_id: project.id,
        template_id: item.id.to_string(),
        created_agents: item.agents.len(),
        created_tasks: item.tasks.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multi_agent::MultiAgentStore;

    struct Fixture {
        root: std::path::PathBuf,
        store: Option<MultiAgentStore>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "crewvela-template-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
            Self {
                root,
                store: Some(store),
            }
        }

        fn store(&self) -> &MultiAgentStore {
            self.store.as_ref().unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.store.take();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn vibecoding_template_lists_stages_with_two_gates() {
        let views = list();
        assert_eq!(views.len(), 1);
        let view = &views[0];
        assert_eq!(view.id, "vibecoding");
        assert_eq!(view.agent_count, 7);
        // 8 个阶段编号（5 并行两个任务），卡点在阶段 2（PRD）与阶段 6（代码审核）。
        assert_eq!(view.stages.len(), 8);
        assert_eq!(view.stages[0].stage, 1);
        assert!(!view.stages[0].human_gate);
        assert!(view.stages[1].human_gate);
        assert!(view.stages[5].human_gate);
        let gated: Vec<u32> = view
            .stages
            .iter()
            .filter(|stage| stage.human_gate)
            .map(|stage| stage.stage)
            .collect();
        assert_eq!(gated, vec![2, 6]);
        assert_eq!(view.stages.iter().filter(|s| s.parallel).count(), 1);
        assert!(view
            .stages
            .iter()
            .filter(|s| s.parallel)
            .all(|s| s.stage == 5));
    }

    #[test]
    fn instantiate_creates_agents_tasks_dependencies_and_gate() {
        let fixture = Fixture::new();
        let receipt = instantiate(
            fixture.store(),
            "vibecoding",
            "示例产品",
            "做一个本地密码管理器",
            fixture.root.display().to_string().as_str(),
        )
        .unwrap();
        assert_eq!(receipt.created_agents, 7);
        assert_eq!(receipt.created_tasks, 9);

        let snapshot = fixture.store().snapshot(&receipt.project_id).unwrap();
        assert_eq!(snapshot.agents.len(), 7);
        assert_eq!(snapshot.tasks.len(), 9);
        // 目标占位符已替换。
        assert!(snapshot
            .tasks
            .iter()
            .find(|task| task.title == "产品调研")
            .unwrap()
            .description
            .contains("本地密码管理器"));

        // 人工卡点 ×2：PRD 由审查工程师复核，代码审核由架构师复核；
        // 其余任务不带 reviewer。
        let agent_id = |name: &str| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.name == name)
                .unwrap()
                .id
                .clone()
        };
        let prd = snapshot
            .tasks
            .iter()
            .find(|task| task.title == "撰写 PRD")
            .unwrap();
        assert_eq!(prd.reviewer_agent_id, Some(agent_id("Pi 审查工程师")));
        let review = snapshot
            .tasks
            .iter()
            .find(|task| task.title == "代码审核")
            .unwrap();
        assert_eq!(review.reviewer_agent_id, Some(agent_id("Claude 架构师")));
        // 审查需要实际交付报告，必须允许隔离写入，不能宣称整个工作区只读。
        assert_eq!(review.workspace_mode, WorkspaceMode::Worktree);
        assert!(review.description.contains("仅允许新建或修改 docs/review.md"));
        assert!(review.description.contains("不得修改实现文件或其他文件"));
        assert!(review.acceptance_criteria.contains("变更仅包含 docs/review.md"));
        let deploy = snapshot.tasks.iter().find(|task| task.title == "部署交付").unwrap();
        assert_eq!(deploy.workspace_mode, WorkspaceMode::Worktree);
        assert!(deploy.description.contains("等待人工批准后由系统合并"));
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .filter(|task| task.reviewer_agent_id.is_some())
                .count(),
            2
        );

        // 依赖链：10 条边（research→prd；prd→{architecture,ui}；architecture→{frontend,backend}；
        // ui→frontend；{frontend,backend}→review；review→testing→deploy）。
        assert_eq!(snapshot.dependencies.len(), 10);
        let title_id = |title: &str| {
            snapshot
                .tasks
                .iter()
                .find(|task| task.title == title)
                .unwrap()
                .id
                .clone()
        };
        let depends_on = |title: &str| {
            snapshot
                .dependencies
                .iter()
                .filter(|dep| dep.task_id == title_id(title))
                .map(|dep| {
                    snapshot
                        .tasks
                        .iter()
                        .find(|task| task.id == dep.depends_on_task_id)
                        .unwrap()
                        .title
                        .clone()
                })
                .collect::<Vec<_>>()
        };
        assert!(depends_on("产品调研").is_empty());
        assert_eq!(depends_on("撰写 PRD"), vec!["产品调研".to_string()]);
        let mut architecture_deps = depends_on("架构设计");
        let mut ui_deps = depends_on("UI 设计");
        architecture_deps.sort();
        ui_deps.sort();
        assert_eq!(architecture_deps, vec!["撰写 PRD".to_string()]);
        assert_eq!(ui_deps, vec!["撰写 PRD".to_string()]);
        let mut frontend_deps = depends_on("前端开发");
        frontend_deps.sort();
        assert_eq!(
            frontend_deps,
            vec!["UI 设计".to_string(), "架构设计".to_string()]
        );
        assert_eq!(depends_on("后端开发"), vec!["架构设计".to_string()]);
        let mut review_deps = depends_on("代码审核");
        review_deps.sort();
        assert_eq!(
            review_deps,
            vec!["前端开发".to_string(), "后端开发".to_string()]
        );
        assert_eq!(depends_on("测试验证"), vec!["代码审核".to_string()]);
        assert_eq!(depends_on("部署交付"), vec!["测试验证".to_string()]);
    }

    #[test]
    fn instantiate_validates_inputs() {
        let fixture = Fixture::new();
        let root = fixture.root.display().to_string();
        assert!(instantiate(fixture.store(), "vibecoding", "  ", "g", &root).is_err());
        assert!(instantiate(fixture.store(), "vibecoding", "n", " ", &root).is_err());
        assert!(instantiate(
            fixture.store(),
            "vibecoding",
            "n",
            "g",
            "F:/definitely-missing-dir"
        )
        .is_err());
        assert!(instantiate(fixture.store(), "unknown", "n", "g", &root).is_err());
    }
}

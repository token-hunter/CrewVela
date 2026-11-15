//! The coordinator and public conversation must enforce the same review graph.
//! These borrowed views keep entry-specific DTOs out of the shared policy.
use std::collections::{HashMap, HashSet};

use super::{AgentRole, AgentRuntimeKind, WorkspaceMode};

pub(crate) struct ReviewGraphAgent<'a> {
    pub key: &'a str,
    pub runtime_kind: AgentRuntimeKind,
    pub role: AgentRole,
}

pub(crate) struct ReviewGraphTask<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub assignee_key: &'a str,
    pub depends_on: &'a [String],
    pub workspace_mode: WorkspaceMode,
}

/// Call after validating unique task/member keys and dependency references.
/// This is the existing coordinator policy, shared without relaxing its checks.
pub(crate) fn validate_parallel_review_graph(
    agents: &[ReviewGraphAgent<'_>],
    tasks: &[ReviewGraphTask<'_>],
) -> Result<(), String> {
    let agents_by_key = agents
        .iter()
        .map(|agent| (agent.key, agent))
        .collect::<HashMap<_, _>>();
    let review_task_keys = tasks
        .iter()
        .filter(|task| {
            agents_by_key
                .get(task.assignee_key)
                .is_some_and(|agent| agent.role == AgentRole::Reviewer)
        })
        .map(|task| task.key)
        .collect::<HashSet<_>>();
    let implementation_tasks = tasks
        .iter()
        .filter(|task| {
            task.workspace_mode != WorkspaceMode::ReadOnly
                && !task
                    .depends_on
                    .iter()
                    .any(|dependency| review_task_keys.contains(dependency.as_str()))
                && agents_by_key
                    .get(task.assignee_key)
                    .is_some_and(|agent| agent.role == AgentRole::Worker)
        })
        .collect::<Vec<_>>();
    let implementation_runtimes = implementation_tasks
        .iter()
        .filter_map(|task| agents_by_key.get(task.assignee_key))
        .map(|agent| agent.runtime_kind.as_str())
        .collect::<HashSet<_>>();
    if implementation_tasks.len() >= 2 && implementation_runtimes.len() >= 2 {
        let review_tasks = tasks
            .iter()
            .filter(|task| {
                agents_by_key
                    .get(task.assignee_key)
                    .is_some_and(|agent| agent.role == AgentRole::Reviewer)
            })
            .collect::<Vec<_>>();
        for implementation in &implementation_tasks {
            if implementation.workspace_mode != WorkspaceMode::Worktree {
                return Err(format!(
                    "并行实现任务“{}”必须使用独立 Worktree，才能生成可审计并可安全集成的提交",
                    implementation.title
                ));
            }
            let implementation_runtime = agents_by_key
                .get(implementation.assignee_key)
                .expect("validated assignee")
                .runtime_kind;
            let cross_reviewed = review_tasks.iter().any(|review| {
                review.workspace_mode == WorkspaceMode::ReadOnly
                    && review
                        .depends_on
                        .iter()
                        .any(|dependency| dependency == implementation.key)
                    && agents_by_key
                        .get(review.assignee_key)
                        .is_some_and(|reviewer| reviewer.runtime_kind != implementation_runtime)
            });
            if !cross_reviewed {
                return Err(format!(
                    "并行实现任务“{}”缺少由不同 Runtime 执行的独立只读 Review 任务",
                    implementation.title
                ));
            }
        }
        let integration_tasks = tasks
            .iter()
            .filter(|task| {
                agents_by_key
                    .get(task.assignee_key)
                    .is_some_and(|agent| agent.role == AgentRole::Integrator)
            })
            .collect::<Vec<_>>();
        if integration_tasks.is_empty()
            || review_tasks.iter().any(|review| {
                !integration_tasks.iter().any(|integration| {
                    integration
                        .depends_on
                        .iter()
                        .any(|dependency| dependency == review.key)
                })
            })
        {
            return Err(
                "多 Runtime 并行实现必须由最终集成任务依赖全部交叉 Review 任务".to_string(),
            );
        }
        if integration_tasks
            .iter()
            .any(|task| task.workspace_mode != WorkspaceMode::Worktree)
        {
            return Err("最终集成任务必须使用独立 Worktree".to_string());
        }
    }
    Ok(())
}

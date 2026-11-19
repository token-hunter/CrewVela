import { Play, Square, Stack } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { invoke } from "@tauri-apps/api/core";
import type { AgentProfile, ConversationExecutionPlan } from "../types";

export function ConversationPlanCard({ plan, agents, blocked, paused, onConfirm, onAdjust, onTasks }: {
  plan: ConversationExecutionPlan;
  agents: AgentProfile[];
  blocked: boolean;
  paused: boolean;
  onConfirm: () => void;
  onAdjust: () => void;
  onTasks: () => void;
}) {
  const managed = plan.managed;
  if (managed) {
    const ready = managed.status === "completed";
    const summary = managed.status === "failed" && managed.summary?.includes("MCP config file not found:")
      && managed.summary.includes("{mcpServers:")
      ? "Claude Code 启动失败。Windows 未能正确传递 MCP 配置，可在任务执行查看详情。"
      : managed.summary;
    return <section className="conversation-plan team-plan-card" aria-label="全自动托管结果">
      <header className="team-plan-heading"><Stack size={23} /><h3>{ready ? "成品已交付" : managed.status === "failed" ? "全自动托管未交付" : paused && managed.status === "running" ? "全自动托管已暂停" : "全自动托管中"}</h3></header>
      {summary && <p className="team-plan-summary">{summary}</p>}
      {ready && <p className="team-plan-acceptance">验收通过 <code>{managed.verificationCommand}</code> · {managed.target.branch}（{managed.commit?.slice(0, 8)}）</p>}
      <div className="team-plan-actions">
        {ready && <button type="button" className="conversation-primary" onClick={() => void invoke("workspace_reveal_path", { root: managed.target.root, relativePath: "README.md" })}>查看启动说明</button>}
        {plan.status === "applied" && <button type="button" className="conversation-text-button" onClick={onTasks}>查看任务执行</button>}
      </div>
    </section>;
  }
  const pending = plan.status === "pending";
  const acceptance = [...new Set(plan.tasks.map(task => task.acceptanceCriteria).filter(Boolean))].join("；");
  return <section className="conversation-plan team-plan-card" aria-label={pending ? "待确认分工" : "项目分工"}>
    <header className="team-plan-heading"><Stack size={23} /><h3>{pending ? "待确认分工" : plan.status === "applied" ? "分工已确认" : "分工已更新"}</h3></header>
    <p className="team-plan-summary">{plan.summary}</p>
    <div className="team-plan-rows">{plan.tasks.map(task => {
      const agent = agents.find(item => item.id === task.assigneeAgentId);
      return <div className="team-plan-row" key={task.key}>
        <strong><Square size={20} aria-hidden="true" />{task.title}</strong>
        <span className="team-task-agent">{agent && <AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative />}<span>{agent?.name ?? "历史成员"}</span></span>
        <span className="team-plan-description" title={task.description}>{task.description}</span>
      </div>;
    })}</div>
    {acceptance && <p className="team-plan-acceptance">验收标准：{acceptance}</p>}
    <details className="team-plan-contract"><summary>查看完整分工</summary>{plan.tasks.map(task => <div key={task.key}>
      <strong>{task.title}</strong><p>{task.description}</p><p>{task.workspaceMode === "worktree" ? "独立工作树" : "只读任务"} · 写入：{task.writePaths.join("、") || "无"}</p>
      <p>依赖：{task.dependsOn.map(key => plan.tasks.find(item => item.key === key)?.title ?? key).join("、") || "无"}</p>
      <p>验收：{task.acceptanceCriteria}</p><p>验收命令：<code>{task.verificationCommand ?? "未指定命令"}</code></p>
      {task.reviewerAgentId && <p>审查：{agents.find(agent => agent.id === task.reviewerAgentId)?.name ?? "历史成员"}</p>}
    </div>)}</details>
    <div className="team-plan-actions">{pending ? <>
      <button type="button" className="conversation-primary" disabled={blocked} onClick={onConfirm}><Play size={18} />确认并开始执行</button>
      <button type="button" className="conversation-text-button team-plan-adjust" disabled={blocked} onClick={onAdjust}>调整分工</button>
    </> : plan.status === "applied" ? <button type="button" className="conversation-text-button" onClick={onTasks}>查看任务执行</button> : <span>请使用最新的分工计划。</span>}</div>
  </section>;
}

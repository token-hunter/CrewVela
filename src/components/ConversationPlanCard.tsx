import { useState } from "react";
import { CaretDown, Check, Copy, Play, Square, Stack } from "@phosphor-icons/react";
import { AgentAvatar } from "./AgentAvatar";
import { ManagedDeliveryMembers } from "./ManagedDeliveryMembers";
import { ProjectPreviewActions } from "./ProjectPreviewActions";
import type { ConversationExecutionPlan, MultiAgentSnapshot, MultiAgentStyle } from "../types";

export function ConversationPlanCard({ plan, snapshot, blocked, paused, style = "classic", onConfirm, onCodexLogin, onAdjust, onTasks, onTaskRecords }: {
  plan: ConversationExecutionPlan;
  snapshot: MultiAgentSnapshot | null;
  blocked: boolean;
  paused: boolean;
  style?: MultiAgentStyle;
  onConfirm: () => void;
  onCodexLogin?: () => void;
  onAdjust: () => void;
  onTasks: () => void;
  onTaskRecords: (taskId: string) => void;
}) {
  const [foldOpen, setFoldOpen] = useState(true);
  const [copiedCommand, setCopiedCommand] = useState<string | null>(null);
  const isGrokNoir = style === "grok_noir";
  const agents = snapshot?.agents ?? [];

  const copyCommand = (cmd: string) => {
    void navigator.clipboard?.writeText(cmd).then(() => {
      setCopiedCommand(cmd);
      window.setTimeout(() => setCopiedCommand(current => current === cmd ? null : current), 1600);
    });
  };

  const managed = plan.managed;
  if (managed) {
    const smallTeam = managed.target.mode === "solo" || managed.target.mode === "pair";
    const receipt = managed.finalAcceptance;
    const commands = [...new Set(plan.tasks.map(task => task.verificationCommand).filter(Boolean))];
    if (managed.status === "completed") return <section className={`conversation-plan team-plan-card team-delivery-card${isGrokNoir ? " grok-delivery-card" : ""}`} aria-label="全自动托管结果">
      <ProjectPreviewActions key={plan.id} projectId={plan.projectId} planId={plan.id} config={managed.preview} demo={managed.demo} root={managed.target.root}
        onTasks={plan.status === "applied" ? onTasks : undefined} />
      {smallTeam && receipt && receipt.commit === managed.commit && <p className="team-plan-acceptance">
        宿主测试通过 · {receipt.reviewRunId ? "独立只读审查通过" : "本轮未安排独立审查"}<br />
        <code>{receipt.verificationCommand}</code><br />
        已合入 {managed.target.branch} · <code title={receipt.commit}>{receipt.commit.slice(0, 12)}</code>
      </p>}
    </section>;
    const summary = managed.status === "failed" && managed.summary?.includes("MCP config file not found:")
      && managed.summary.includes("{mcpServers:")
      ? "Claude Code 启动失败。Windows 未能正确传递 MCP 配置，可在任务执行查看详情。"
      : managed.summary;
    const headingText = managed.status === "waiting_login"
      ? "全自动托管等待 Codex 登录"
      : managed.status === "failed"
        ? "全自动托管未交付"
        : paused && managed.status === "running"
          ? "全自动托管已暂停"
          : "全自动托管中";

    if (isGrokNoir) {
      const tone = managed.status === "failed" ? "fail" : (managed.status === "waiting_login" || paused) ? "warn" : "running";
      const stepCount = Math.max(plan.tasks.length, plan.taskIds.length);
      const uniqueAssignees = [...new Set(plan.tasks.map(task => task.assigneeAgentId).filter(Boolean))]
        .map(id => agents.find(agent => agent.id === id))
        .filter((agent): agent is NonNullable<typeof agent> => Boolean(agent))
        .slice(0, 3);
      return <section className="conversation-plan team-plan-card grok-plan-wrap" data-tone={tone} aria-label="全自动托管结果">
        <div className={`grok-process-fold${foldOpen ? " is-open" : ""}`}>
          <button type="button" className="grok-fold-trigger" aria-expanded={foldOpen} onClick={() => setFoldOpen(value => !value)}>
            <span className="grok-fold-left">
              <span className={`grok-status-dot is-${tone}`} aria-hidden="true" />
              {uniqueAssignees.length > 0 && (
                <span className="grok-fold-avatars" aria-hidden="true">
                  {uniqueAssignees.map(agent => (
                    <span key={agent.id} className="grok-fold-avatar-chip">
                      <AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative />
                    </span>
                  ))}
                </span>
              )}
              <h3 className="grok-fold-heading">{headingText}</h3>
              {stepCount > 0 && <span className="grok-fold-count">{stepCount} 步</span>}
            </span>
            <span className="grok-fold-toggle">
              <span>{foldOpen ? "收起" : "展开"}</span>
              <CaretDown size={13} aria-hidden="true" />
            </span>
          </button>
          {foldOpen && <div className="grok-fold-body">
            <ManagedDeliveryMembers plan={plan} snapshot={snapshot} onRecords={onTaskRecords} />
            {commands.length > 0 && <div className="grok-fold-commands" aria-label="计划验收命令">
              <span className="grok-fold-cmd-label">计划验收命令</span>
              {commands.map(command => (
                <div className="grok-cmd-box" key={command}>
                  <code>{command}</code>
                  <button type="button" className="grok-icon-btn sm" aria-label="复制验收命令"
                    title={copiedCommand === command ? "已复制" : "复制验收命令"}
                    onClick={() => copyCommand(command!)}>
                    {copiedCommand === command ? <Check size={13} /> : <Copy size={13} />}
                  </button>
                </div>
              ))}
            </div>}
          </div>}
        </div>
        {summary && <p className={`team-plan-summary grok-verdict is-${tone}`}>{summary}</p>}
        {(onCodexLogin || plan.status === "applied") && <div className="team-plan-actions grok-plan-actions">
          {onCodexLogin && <button type="button" className="grok-pill-btn primary" disabled={blocked} onClick={onCodexLogin}>登录 Codex</button>}
          {plan.status === "applied" && <button type="button" className="conversation-text-button grok-pill-btn" onClick={onTasks}>查看任务执行</button>}
        </div>}
      </section>;
    }

    return <section className="conversation-plan team-plan-card" aria-label="全自动托管结果">
      <header className="team-plan-heading"><Stack size={23} /><h3>{headingText}</h3></header>
      <ManagedDeliveryMembers plan={plan} snapshot={snapshot} onRecords={onTaskRecords} />
      {commands.length > 0 && <div className="team-plan-acceptance">计划验收命令{commands.map(command => <div key={command}><code>{command}</code></div>)}</div>}
      {summary && <p className="team-plan-summary">{summary}</p>}
      {onCodexLogin && <button type="button" className="conversation-text-button" disabled={blocked} onClick={onCodexLogin}>登录 Codex</button>}
      {plan.status === "applied" && <div className="team-plan-actions"><button type="button" className="conversation-text-button" onClick={onTasks}>查看任务执行</button></div>}
    </section>;
  }
  const pending = plan.status === "pending";
  const acceptance = [...new Set(plan.tasks.map(task => task.acceptanceCriteria).filter(Boolean))].join("；");
  const planHeading = pending ? "待确认分工" : plan.status === "applied" ? "分工已确认" : "分工已更新";

  if (isGrokNoir) {
    const tone = pending ? "warn" : plan.status === "applied" ? "done" : "idle";
    return <section className="conversation-plan team-plan-card grok-plan-wrap" data-tone={tone} aria-label={pending ? "待确认分工" : "项目分工"}>
      <div className={`grok-process-fold${foldOpen ? " is-open" : ""}`}>
        <button type="button" className="grok-fold-trigger" aria-expanded={foldOpen} onClick={() => setFoldOpen(value => !value)}>
          <span className="grok-fold-left">
            <span className={`grok-status-dot is-${tone}`} aria-hidden="true" />
            <h3 className="grok-fold-heading">{planHeading}</h3>
            <span className="grok-fold-count">{plan.tasks.length} 项任务</span>
          </span>
          <span className="grok-fold-toggle">
            <span>{foldOpen ? "收起" : "展开"}</span>
            <CaretDown size={13} aria-hidden="true" />
          </span>
        </button>
        {foldOpen && <div className="grok-fold-body">
          <div className="team-plan-rows">{plan.tasks.map(task => {
            const agent = agents.find(item => item.id === task.assigneeAgentId);
            return <div className="team-plan-row" key={task.key}>
              <strong><Square size={16} aria-hidden="true" />{task.title}</strong>
              <span className="team-task-agent">{agent && <AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative />}<span>{agent?.name ?? "历史成员"}</span></span>
              <span className="team-plan-description" title={task.description}>{task.description}</span>
            </div>;
          })}</div>
          {acceptance && <p className="team-plan-acceptance">验收标准：{acceptance}</p>}
          <details className="team-plan-contract grok-plan-contract"><summary>查看完整分工</summary>{plan.tasks.map(task => <div key={task.key}>
            <strong>{task.title}</strong><p>{task.description}</p><p>{task.workspaceMode === "worktree" ? "独立工作树" : "只读任务"} · 写入：{task.writePaths.join("、") || "无"}</p>
            <p>依赖：{task.dependsOn.map(key => plan.tasks.find(item => item.key === key)?.title ?? key).join("、") || "无"}</p>
            <p>验收：{task.acceptanceCriteria}</p><p>验收命令：<code>{task.verificationCommand ?? "未指定命令"}</code></p>
            {task.reviewerAgentId && <p>审查：{agents.find(agent => agent.id === task.reviewerAgentId)?.name ?? "历史成员"}</p>}
          </div>)}</details>
        </div>}
      </div>
      {plan.summary && <p className={`team-plan-summary grok-verdict is-${tone}`}>{plan.summary}</p>}
      <div className="team-plan-actions grok-plan-actions">{pending ? <>
        <button type="button" className="conversation-primary grok-pill-btn primary" disabled={blocked} onClick={onCodexLogin ?? onConfirm}><Play size={15} weight="fill" />{onCodexLogin ? "登录 Codex" : "确认并开始执行"}</button>
        <button type="button" className="conversation-text-button team-plan-adjust grok-pill-btn" disabled={blocked} onClick={onAdjust}>调整分工</button>
      </> : plan.status === "applied" ? <button type="button" className="conversation-text-button grok-pill-btn" onClick={onTasks}>查看任务执行</button> : <span>请使用最新的分工计划。</span>}</div>
    </section>;
  }

  return <section className="conversation-plan team-plan-card" aria-label={pending ? "待确认分工" : "项目分工"}>
    <header className="team-plan-heading"><Stack size={23} /><h3>{planHeading}</h3></header>
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
      <button type="button" className="conversation-primary" disabled={blocked} onClick={onCodexLogin ?? onConfirm}><Play size={18} />{onCodexLogin ? "登录 Codex" : "确认并开始执行"}</button>
      <button type="button" className="conversation-text-button team-plan-adjust" disabled={blocked} onClick={onAdjust}>调整分工</button>
    </> : plan.status === "applied" ? <button type="button" className="conversation-text-button" onClick={onTasks}>查看任务执行</button> : <span>请使用最新的分工计划。</span>}</div>
  </section>;
}

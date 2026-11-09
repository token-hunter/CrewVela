import type { DeliveryRecord, MultiAgentSnapshot } from "./types";

export interface TeamMessageItem {
  id: string;
  at: number;
  from: string;
  to: string;
  body: string;
  source: string;
  stage: string | null;
  error: string | null;
}

export function deliveryStageLabel(stage: string | null): string {
  switch (stage) {
    case "queued": return "排队中";
    case "text_written": return "文本已写入，待提交";
    case "submit_sent": return "已提交输入";
    case "bridge_received": return "运行时已接收";
    case "submit_failed": return "提交失败";
    default: return "投递状态未知";
  }
}

/** 只按共同消息 ID 合并。旧数据没有运输关联时不得以相同正文猜测回执。 */
export function buildTeamMessages(snapshot: MultiAgentSnapshot, evidence: DeliveryRecord[]): TeamMessageItem[] {
  const agents = new Map(snapshot.agents.map(agent => [agent.id, agent.name]));
  const sessions = new Map(snapshot.agentSessions.filter(session => session.appSessionId)
    .map(session => [session.appSessionId!, session.agentId]));
  const scoped = evidence.filter(record => record.kind !== "initial_prompt" &&
    (sessions.has(record.toSessionId) || Boolean(record.fromSessionId && sessions.has(record.fromSessionId))));
  const consumed = new Set<string>();
  const items: TeamMessageItem[] = snapshot.messages
    .filter(message => message.projectId === snapshot.project.id)
    .map(message => {
      const matches = scoped.filter(record => record.id === message.id &&
        sessions.get(record.toSessionId) === message.targetAgentId);
      // 即便 ID 相同，目标不一致或有多条运输证据也保留未知状态。
      const record = matches.length === 1 ? matches[0] : undefined;
      if (record) consumed.add(`${record.id}:${record.toSessionId}`);
      return {
        id: `message:${message.id}`, at: message.createdAt,
        from: message.senderId ? agents.get(message.senderId) ?? "历史成员" : message.senderType === "user" ? "你" : "协调器",
        to: message.targetAgentId ? agents.get(message.targetAgentId) ?? "历史成员" : "项目",
        body: message.body, source: message.senderType === "agent" ? "Agent 通信 · 项目历史" : "项目历史",
        stage: message.targetAgentId ? record?.stage ?? "unknown" : null,
        error: record?.error ?? null,
      };
    });
  for (const record of scoped) {
    if (consumed.has(`${record.id}:${record.toSessionId}`)) continue;
    items.push({
      id: `transport:${record.id}:${record.toSessionId}`, at: record.at,
      from: record.fromSessionId ? agents.get(sessions.get(record.fromSessionId) ?? "") ?? record.fromLabel ?? "外部会话" : "你 / 系统",
      to: agents.get(sessions.get(record.toSessionId) ?? "") ?? "外部会话",
      body: record.body, source: "实时通信", stage: record.stage ?? "unknown", error: record.error ?? null,
    });
  }
  for (const run of snapshot.runs) {
    if (run.projectId !== snapshot.project.id || (!run.outputSummary && !run.error)) continue;
    const task = snapshot.tasks.find(task => task.id === run.taskId);
    items.push({
      id: `run:${run.id}`, at: run.finishedAt ?? run.startedAt ?? run.createdAt,
      from: agents.get(run.agentId) ?? "历史成员", to: task?.title ?? "任务",
      body: run.error || run.outputSummary || "", source: `任务执行摘要 · ${run.status} · 第 ${run.attempt} 次（Agent 报告）`,
      stage: null, error: run.error,
    });
  }
  return items.sort((left, right) => left.at - right.at || left.id.localeCompare(right.id));
}

export function normalizeRightSidebarTab(value: string | null): "multi_agent" | "files" | "git" | "browser" {
  return value === "files" || value === "git" || value === "browser" ? value : "multi_agent";
}

export function summarizeTeamSend(outcomes: PromiseSettledResult<unknown>[], sessionIds: string[]) {
  return {
    clearDraft: outcomes.some(outcome => outcome.status === "fulfilled"),
    failures: outcomes.flatMap((outcome, index) => outcome.status === "rejected"
      ? [`${sessionIds[index]}：${String(outcome.reason)}`] : []),
  };
}

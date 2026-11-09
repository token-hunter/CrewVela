import type { DeliveryRecord, MultiAgentSnapshot } from "./types";

/** 只供本地 UI 预览使用，不连接 Tauri 或启动会话。 */
export function createTeamMessagePreview(base: MultiAgentSnapshot) {
  const at = new Date("2026-10-02T09:00:00+08:00").getTime();
  const stages = ["queued", "submit_sent", "submit_failed"];
  const evidence: DeliveryRecord[] = stages.map((stage, index) => ({
    id: `preview-transport-${index}`, at: at + index * 60_000,
    fromSessionId: "preview-claude-session", fromLabel: "Claude",
    toSessionId: "preview-codex-session", body: ["接口契约已整理，请确认字段。", "登录接口已完成，请验证错误分支。", "请继续检查集成测试。"] [index],
    kind: "chat", stage, error: stage === "submit_failed" ? "提交回车失败，消息尚未提交；请检查目标终端。" : null,
  }));
  const snapshot: MultiAgentSnapshot = {
    ...base,
    agentSessions: base.agents.map((agent, index) => ({
      id: `preview-link-${index}`, projectId: base.project.id, agentId: agent.id,
      appSessionId: index === 0 ? "preview-claude-session" : "preview-codex-session",
      currentTaskId: null, currentRunId: null, status: "idle", controlMode: "auto",
      lastOutputAt: null, createdAt: at, updatedAt: at,
    })),
    messages: [
      ...evidence.map(record => ({
        id: record.id, projectId: base.project.id, taskId: null,
        senderType: "agent", senderId: base.agents[0].id, targetAgentId: base.agents[1].id,
        body: record.body, createdAt: record.at,
      })),
      { id: "preview-history", projectId: base.project.id, taskId: null,
        senderType: "agent", senderId: base.agents[1].id, targetAgentId: base.agents[0].id,
        body: "旧版项目消息已保留，原投递记录不可用。", createdAt: at + 180_000 },
    ],
    runs: [{ id: "preview-result", projectId: base.project.id, taskId: base.tasks[1].id,
      agentId: base.agents[1].id, attempt: 1, status: "succeeded", sessionLocator: null,
      leaseExpiresAt: at, lastHeartbeatAt: null, outputSummary: "已补齐登录失败分支和接口测试。验证：12 项测试通过。待人工确认最终体验。",
      error: null, createdAt: at, startedAt: at, finishedAt: at + 240_000 }],
  };
  return { snapshot, evidence };
}

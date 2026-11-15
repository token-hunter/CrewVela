import type { AgentProfile, ConversationMessage, ConversationRound, ConversationStatus } from "./types";
import { hasMentionToken, replaceMentionToken } from "./multiAgentMention.ts";

export const CONVERSATION_STATUS: Record<ConversationStatus, string> = {
  queued: "已排队", running: "正在回复", waiting_approval: "等待你处理", stopping: "正在停止",
  completed: "回复结束", failed: "失败", cancelled: "已取消", paused: "已暂停",
  unconfirmed: "结果未确认", needs_decision: "需要你决策",
};
export const TASK_STATUS: Record<string, string> = {
  draft: "待安排", ready: "等待执行", blocked: "等待依赖", claimed: "已领取", running: "进行中",
  waiting_approval: "等待你处理", in_review: "等待交付审批", changes_requested: "需修改",
  done: "验收通过", failed: "失败", cancelled: "已取消",
};
export function latestConversationRound(rounds: ConversationRound[]): ConversationRound | undefined {
  return rounds.reduce<ConversationRound | undefined>((latest, item) => !latest || item.createdAt >= latest.createdAt ? item : latest, undefined);
}
export function conversationIsActive(round?: ConversationRound): boolean {
  return !!round && ["queued", "running", "waiting_approval", "stopping", "unconfirmed"].includes(round.status);
}
export function conversationMentionToken(agent: AgentProfile, agents: AgentProfile[]): string {
  const duplicate = agents.filter(item => item.name.toLocaleLowerCase() === agent.name.toLocaleLowerCase()).length > 1;
  return `@${agent.name}${duplicate ? `#${agent.id}` : ""}`;
}
/** Bind only enabled, current-project identities. Unknown @ targets must never become a team broadcast. */
export function conversationRecipients(body: string, selected: string[], agents: AgentProfile[], projectId: string): string[] {
  const enabled = agents.filter(agent => agent.enabled && agent.projectId === projectId);
  let remaining = body;
  const mentioned = new Set<string>();
  const tokens = enabled.map(agent => ({ agent, token: conversationMentionToken(agent, enabled) })).sort((a, b) => b.token.length - a.token.length);
  for (const { agent, token } of tokens) {
    if (!hasMentionToken(remaining, token)) continue;
    mentioned.add(agent.id);
    remaining = replaceMentionToken(remaining, token, "");
  }
  const unknown = remaining.match(/(?:^|\s)(@[^\s@]+)/u)?.[1];
  if (unknown) throw new Error(`未找到明确的项目成员 ${unknown}，请从成员列表选择。`);
  const ids = enabled.filter(agent => mentioned.size ? mentioned.has(agent.id) : selected.includes(agent.id)).map(agent => agent.id);
  if (!ids.length) throw new Error("请选择至少一位项目成员。");
  return ids;
}
export function orderedConversationMessages(messages: ConversationMessage[], projectId: string): ConversationMessage[] {
  const unique = new Map<string, ConversationMessage>();
  for (const message of messages) {
    if (message.projectId !== projectId) continue;
    const previous = unique.get(message.id);
    if (!previous || message.updatedAt >= previous.updatedAt) unique.set(message.id, message);
  }
  return [...unique.values()].sort((a, b) => a.createdAt - b.createdAt);
}

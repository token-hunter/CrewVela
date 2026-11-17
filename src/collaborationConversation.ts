import type { AgentProfile, ConversationMessage, ConversationMode, ConversationRound, ConversationStatus } from "./types";
import { hasMentionToken, replaceMentionToken } from "./multiAgentMention.ts";

export const CONVERSATION_STATUS: Record<ConversationStatus, string> = {
  queued: "已排队", running: "正在回复", waiting_approval: "等待工具授权", stopping: "正在停止",
  completed: "回复结束", failed: "失败", cancelled: "已取消", paused: "已暂停",
  unconfirmed: "结果未确认", needs_decision: "等待你补充",
};
/** Conservative routing: uncertain requests stay in chat; a plan still needs explicit execution approval. */
export function conversationRequestMode(body: string, selectedMode: ConversationMode = "discussion"): ConversationMode {
  if (selectedMode === "execution_plan") return selectedMode;
  const text = body.replace(/```[\s\S]*?(?:```|$)/g, "").replace(/`[^`]*`/g, "");
  if (/(?:不要|(?<!分)别|无需|不需要|暂不|先不|不准|只聊|讨论|聊聊|解释|如何|怎么|分析|评估|能否|是否|告诉我|是什么意思|示例)|\b(?:don't|do not|not yet|only discuss|explain|analy[sz]e|how to)\b/iu.test(text)) return "discussion";
  const engineering = /(?:请|帮我|麻烦|你们|大家|团队|一起|共同|各自|分别)/u.test(text)
    && /(?:分工|协作|合作|一起|共同|你们|大家|各自|分别)/u.test(text)
    && /(?:开发|实现|编写|构建|修复|完成).{0,30}(?:项目|功能|程序|系统|应用|页面|任务|代码|bug)/iu.test(text);
  const teamEngineering = /\b(?:team|together|collaborate|split the work)\b/i.test(text)
    && /\b(?:build|implement|develop|fix)\b.{0,80}\b(?:project|feature|app|application|bug|code|page)\b/i.test(text);
  const explicitPlan = /^(?:请|帮我)?(?:生成|制定|调整|更新|重新生成)分工(?:[：:，,\s]|$)/u.test(text.trim())
    || /(?:^|[，。；,;\n])\s*(?:请)?(?:先)?(?:给(?:我)?|生成|制定|安排)(?:一下|个|一份)?分工(?:[：:，,。！!；;\s]|$)/u.test(text.trim());
  return engineering || teamEngineering || explicitPlan ? "execution_plan" : "discussion";
}
export function hasConversationPlanGoal(body: string, mentionTokens: readonly string[]): boolean {
  const goal = [...mentionTokens].sort((a, b) => b.length - a.length)
    .reduce((text, token) => replaceMentionToken(text, token, ""), body);
  return /[\p{L}\p{N}]/u.test(goal);
}
export function conversationRequestIntent(body: string): "chat" | "exchange" {
  const text = body.replace(/```[\s\S]*?(?:```|$)/g, "").replace(/`[^`]*`/g, "");
  if (/(?:不要|(?<!分)别|无需|不需要|先不).{0,8}(?:互聊|互相|讨论|交流|回应)|\b(?:don't|do not|no need to).{0,12}(?:discuss|debate|exchange)/iu.test(text)) return "chat";
  return /(?:互相|彼此).{0,12}(?:聊|讨论|交流|回应|回复)|(?:你们|大家|一起|共同).{0,12}(?:聊|讨论|交流)|(?:聊聊|讨论|交流).{0,12}(?:方案|思路|问题)|(?:对方|另一位).{0,12}(?:回复|回应|意见)|\b(?:discuss|debate|talk about|exchange ideas|exchange views|respond to each other)\b/iu.test(text) ? "exchange" : "chat";
}
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

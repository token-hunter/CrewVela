import type { AgentProfile, AgentRole, ConversationChatDispatch, ConversationMessage, ConversationMode, ConversationRound, ConversationStatus } from "./types";
import { hasMentionToken, replaceMentionToken } from "./multiAgentMention.ts";

export const CONVERSATION_STATUS: Record<ConversationStatus, string> = {
  queued: "已排队", running: "正在回复", waiting_approval: "等待工具授权", stopping: "正在停止",
  completed: "回复结束", failed: "失败", cancelled: "已取消", paused: "已暂停",
  unconfirmed: "结果未确认", needs_decision: "等待你补充",
};
/** Conservative routing: uncertain requests stay in chat; a plan still needs explicit execution approval. */
export function conversationRequestMode(body: string, selectedMode: ConversationMode = "discussion", mentionTokens: readonly string[] = []): ConversationMode {
  if (selectedMode === "execution_plan") return selectedMode;
  const text = body.replace(/```[\s\S]*?(?:```|$)/g, "").replace(/`[^`]*`/g, "");
  const opening = withoutMentionTokens(text, mentionTokens).trim().replace(/^(?:@[^\s@]+\s+)+/u, "");
  const firstRequest = opening.split(/[。！？!?；;\n，,]/u, 1)[0];
  // Negative instructions about execution take precedence over an imperative goal.
  const discussionOnly = /(?:^|[。！？!?；;\n，,])\s*(?:请)?(?:先|暂时)?(?:只|仅)(?:做|想)?(?:分析|讨论|评估|聊|听)|(?:不要|(?<!分)别|无需|不需要|暂不|先不|不准)\s*(?:开始|实际|直接)?\s*(?:开发|执行|动手|写(?:任何)?代码|动代码|修改(?:任何)?(?:代码|文件))|(?:^|[。！？!?；;\n，,])\s*不(?:写(?:任何)?代码|动代码|修改(?:任何)?(?:代码|文件))/u.test(text)
    || /\b(?:don't|do not|not yet|only discuss)\b/i.test(text);
  if (discussionOnly) return "discussion";
  const directDevelopment = /^(?:(?:请|帮我|麻烦|你们|大家|团队|我想|我需要|先|直接|一起|共同|分工|协作)\s*)*(?:开发|实现|编写|构建|制作|做|修复|新增|添加|搭建).{0,40}(?:游戏|项目|功能|程序|系统|应用|工具|网站|页面|接口|脚本|代码|组件|服务|bug)/iu.test(firstRequest)
    && !/(?:需要多久|要多久|需要什么|可行吗|吗$|(?:怎么|如何)(?:做|实现|开发)$|多少钱)/u.test(firstRequest)
    && !/^[^。！!；;\n]*[？?]/u.test(opening);
  const directEnglish = /^(?:please\s+|help me\s+)?(?:build|develop|implement|create|fix)\b.{0,60}\b(?:game|project|feature|app|application|bug|page|tool|site|api)\b/i.test(firstRequest)
    && !/^[^.!;\n]*\?/u.test(opening);
  if (directDevelopment || directEnglish) return "execution_plan";
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
function withoutMentionTokens(body: string, mentionTokens: readonly string[]): string {
  return [...mentionTokens].sort((a, b) => b.length - a.length)
    .reduce((text, token) => replaceMentionToken(text, token, ""), body);
}
export function hasConversationPlanGoal(body: string, mentionTokens: readonly string[]): boolean {
  return /[\p{L}\p{N}]/u.test(withoutMentionTokens(body, mentionTokens));
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
export function conversationRecipients(
  body: string,
  selected: readonly string[],
  agents: AgentProfile[],
  projectId: string,
  preferredRecipient?: string,
): string[] {
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
  if (!mentioned.size && preferredRecipient && !selected.includes(preferredRecipient)) {
    throw new Error("当前收件成员不可用，请 @ 其他成员或切回发给全体。");
  }
  const candidates = enabled.filter(agent => mentioned.size
    ? mentioned.has(agent.id)
    : selected.includes(agent.id) && (!preferredRecipient || preferredRecipient === agent.id));
  if (!candidates.length) throw new Error("请选择至少一位项目成员。");
  return candidates.map(agent => agent.id);
}

export function conversationChatDispatch(body: string, recipients: readonly AgentProfile[], ruleAgentId?: string): {
  intent: "chat" | "exchange";
  chatDispatch?: ConversationChatDispatch;
} {
  const request = body.replace(/```[\s\S]*?(?:```|$)/g, "").replace(/`[^`]*`/g, "");
  const launch = /(?:启动|运行|重启|停止|关掉).{0,18}(?:项目|服务|程序|应用|游戏|成品)|(?:把|将).{0,12}(?:项目|服务|程序|应用|游戏).{0,8}(?:启动|运行|重启|停止|关掉)|(?:跑|运行|启动)起来|(?:打不开|访问不了|连不上).{0,12}(?:地址|网页|服务|页面)|(?:地址|网页|服务|页面).{0,12}(?:打不开|访问不了|连不上)|\b(?:start|run|restart|stop|launch)\s+(?:(?:this|the|my|our)\s+)?(?:project|server|app|application|game)\b/iu.test(request);
  const explanation = /(?:只|仅|先)\s*(?:分析|讨论|解释|说明)|(?:如何|怎么|怎样|何时|什么时候)\s*(?:启动|运行|重启|停止|执行|操作)|(?:不要|无需|先不|别)\s*(?:启动|运行|重启|执行|动手)|\b(?:how to|how do (?:i|we)|do not|don't)\b/iu.test(request);
  const review = /(?:审查|审阅|复审).{0,16}(?:代码|改动|提交|实现)|\breview\s+(?:(?:this|the|my|our)\s+)?(?:code|changes|commit|implementation)\b/iu.test(request);
  const operation = /(?:谁(?:能|来|可以)|请|帮我|麻烦|直接|马上|^\s*)(?:帮我|实际|先|去|直接|把|将|给我|你们|大家|每人|各自|分别|一起|\s)*(?:运行|执行|运行一下|跑一下|重跑|部署|发布|安装|提交|删除|写入|上传|修改|添加|启动|重启|停止)|\b(?:please\s+|can someone\s+)?(?:run|deploy|install|execute|restart|stop)\s+(?:the\s+)?(?:tests?|command|script|service|server|app|project)\b/iu.test(request.replace(/(?:^|\s)@[^\s@]+\s*/gu, " "));
  if (!ruleAgentId && (explanation || !(launch || operation || (review && !/(?:各自|每[人位]|分别|独立|everyone|each)/iu.test(request))))) {
    const intent = conversationRequestIntent(body);
    return { intent, chatDispatch: intent === "chat" && recipients.length > 1 ? { kind: "parallel" } : undefined };
  }
  const roles: AgentRole[] = launch ? ["integrator", "worker", "orchestrator", "reviewer"]
    : review ? ["reviewer", "worker", "integrator", "orchestrator"]
    : ["worker", "integrator", "orchestrator", "reviewer"];
  const owner = ruleAgentId
    ? recipients.find(agent => agent.id === ruleAgentId)
    : roles.map(role => recipients.find(agent => agent.role === role)).find(Boolean) ?? recipients[0];
  return { intent: "chat", chatDispatch: owner ? { kind: "assigned", agentId: owner.id } : undefined };
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

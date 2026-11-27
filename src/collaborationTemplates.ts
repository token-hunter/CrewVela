import type { AgentProfile, AgentRuntimeKind, AutomationAction, ConversationSendInput, NewAutomation } from "./types";

export type TemplateAction = Extract<AutomationAction, { kind: "conversation_template" }>;
export type TemplateId = TemplateAction["templateId"];

export const collaborationTemplates = {
  pr_review: {
    title: "审阅 PR",
    input: "PR 链接或变更内容",
    placeholder: "粘贴公开 PR 链接、diff 或需要审阅的变更摘要",
    instruction: "检查变更的正确性和已有调用是否会回归。列出具体位置、影响和验证建议，并注明证据范围。",
    example: "示例差异（缓存读取函数）\n- if (cache.has(key)) return cache.get(key);\n+ if (cache.get(key)) return cache.get(key);",
    output: "命中缓存的值为 0 时会重新计算。建议补充值为 0 的缓存命中测试。",
  },
  issue_triage: {
    title: "整理 Issue",
    input: "Issue 链接或问题描述",
    placeholder: "粘贴待分类的问题，可逐条附上链接、复现信息或用户反馈",
    instruction: "逐条给出建议分类、优先级依据、还需补充的信息和下一步核查。所有标签与回复均作为草稿。",
    example: "示例反馈：切换项目后，搜索框仍显示上个项目的关键字。清空输入后恢复。只在 Windows 上尝试过。",
    output: "建议归为界面状态问题。已知能通过清空输入恢复，待确认其他平台及切换步骤。",
  },
  release_check: {
    title: "发布前核对",
    input: "发布范围或检查材料",
    placeholder: "填写版本、检查清单路径、变更说明或已有验证记录",
    instruction: "按已有证据区分已核实项、发布阻塞和待验证项。引用相应文件或记录，列出仍需人工完成的检查。",
    example: "示例发布记录：构建成功；升级安装已覆盖；首次安装尚未验证。请整理桌面版的发布检查。",
    output: "构建和升级安装已有记录。首次安装缺少证据，需在干净环境完成一次启动检查。",
  },
} as const;

export const templateIds = Object.keys(collaborationTemplates) as TemplateId[];
export const templateBoundary = "仅使用本轮允许的只读资料。结果作为草稿回复本话题。遇到访问受限或证据不足时，具体说明缺少的内容。";
type Translate = (source: string) => string;

export function templateMembers(agents: AgentProfile[], projectId: string, supported: AgentRuntimeKind[]) {
  return agents.filter(agent => agent.projectId === projectId && agent.enabled && !agent.removedAt
    && (agent.runtimeKind === "claude_code" || agent.runtimeKind === "pi") && supported.includes(agent.runtimeKind));
}

export function templateBody(template: TemplateId, material: string, t: Translate): string {
  const spec = collaborationTemplates[template];
  return [t(spec.title), t(spec.instruction), t(templateBoundary), material.trim()].join("\n\n");
}

export function templateTrial(action: TemplateAction, clientMessageId: string): ConversationSendInput {
  return {
    projectId: action.projectId, expectedConversationId: action.conversationId, clientMessageId,
    body: action.body, participantAgentIds: [action.agentId], mode: "discussion", intent: "chat",
    nativeCapabilities: false, managed: false, maxRounds: 1, maxDurationSeconds: 600,
    taskRules: { tools: "research", instructions: "" },
  };
}

export function templateSchedule(action: TemplateAction, name: string, time: string, weekdays: boolean, creationId: string): NewAutomation {
  if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(time)) throw Error("请选择有效的执行时间");
  const [hour, minute] = time.split(":").map(Number);
  return {
    action: { ...action, creationId }, name, trigger: weekdays ? { kind: "weekly", weekdays: [1, 2, 3, 4, 5], hour, minute } : { kind: "daily", hour, minute },
    maxRuns: null, maxConsecutiveFailures: 3, overlapPolicy: "skip",
  };
}

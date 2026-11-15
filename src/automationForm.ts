import type { AgentProfile, AgentRuntimeKind, AutomationAction, AutomationTrigger, ProviderConfig, SessionRecord } from "./types";
import { CHAT_RUNTIMES, TERMINAL_RUNTIMES } from "./runtimeTaxonomy.ts";

export const AUTOMATION_LABELS: Partial<Record<AgentRuntimeKind, string>> = {
  claude_code: "Claude Code", codex: "Codex CLI", kimi_code: "Kimi Code", pi: "Pi", opencode: "OpenCode", zcode_cli: "ZCode CLI",
  claude_chat: "Claude Code Chat", codex_chat: "Codex Chat", kimi_chat: "Kimi Code Chat", opencode_chat: "OpenCode Chat", pi_chat: "Pi Chat", deepseek_harness: "DeepSeek Harness", zcode: "ZCode Desktop",
};
export function automationProjectRuntime(kind: AgentRuntimeKind | ""): boolean {
  return TERMINAL_RUNTIMES.includes(kind as AgentRuntimeKind) || ["zcode", "deepseek_harness"].includes(kind);
}
export const automationSessionRuntime = (s: SessionRecord) => s.agentRuntimeKind ?? (s.kind === "claude" ? "claude_code" : null);
export function automationSessions(sessions: SessionRecord[], runtime: AgentRuntimeKind): SessionRecord[] {
  return sessions.filter(s => automationSessionRuntime(s) === runtime && !s.retiredAgentRuntime && !s.archivedAt &&
    !s.runtimeCommandHost && (TERMINAL_RUNTIMES.includes(runtime) || CHAT_RUNTIMES.includes(runtime)));
}
export interface AutomationFormAction {
  runtime: AgentRuntimeKind | "";
  mode: "new" | "session" | "member" | "broadcast";
  source: "local" | "provider";
  providerId: string;
  model: string;
  cwd: string;
  prompt: string;
  sessionId: string;
  projectId: string;
  agentId: string;
}
export function buildAutomationAction(form: AutomationFormAction, enabled: (kind: AgentRuntimeKind) => boolean, providers: ProviderConfig[], sessions: SessionRecord[], agents: AgentProfile[]): AutomationAction {
  if (!form.runtime || !enabled(form.runtime) || !AUTOMATION_LABELS[form.runtime]) throw Error("请选择已启用且支持自动化的 Agent");
  const body = form.prompt;
  if (!body.trim() || Array.from(body).length > 32000) throw Error("请填写 1–32000 字符的执行任务");
  if (form.mode === "new") {
    if (!TERMINAL_RUNTIMES.includes(form.runtime)) throw Error("该 Agent 请使用已有会话或项目成员");
    if (!form.cwd.trim()) throw Error("请选择工作目录");
    return { kind: "run_agent", runtimeKind: form.runtime, providerId: null, model: null, cwd: form.cwd.trim(), prompt: body };
  }
  if (form.mode === "broadcast") return { kind: "broadcast_agent", runtimeKind: form.runtime, body };
  if (form.mode === "member") {
    if (!automationProjectRuntime(form.runtime)) throw Error("该 Agent 请复用已打开的原生聊天会话");
    if (!form.projectId || !agents.some(a => a.projectId === form.projectId && a.id === form.agentId && a.enabled && a.runtimeKind === form.runtime)) throw Error("请选择该项目中类型匹配、已启用的成员");
    return { kind: "send_agent", runtimeKind: form.runtime, sessionId: "", body, projectId: form.projectId, agentId: form.agentId };
  }
  if (!automationSessions(sessions, form.runtime).some(s => s.id === form.sessionId)) throw Error("请选择仍存在且 Agent 类型匹配的会话");
  return { kind: "send_agent", runtimeKind: form.runtime, sessionId: form.sessionId, body, projectId: null, agentId: null };
}
export function eventAutomationTrigger(event: "completed" | "failed", source: "session" | "runtime" | "project", id: string, runtime: AgentRuntimeKind | ""): AutomationTrigger {
  if (source === "runtime" && !runtime) throw Error("请选择事件来源 Agent");
  if (source !== "runtime" && !id) throw Error("请选择具体事件来源");
  return { kind: "on_session_event_filtered", event, runtimeKind: source === "runtime" ? runtime as AgentRuntimeKind : null, sessionId: source === "session" ? id : null, projectId: source === "project" ? id : null };
}

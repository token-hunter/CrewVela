import type { SessionRecord } from "./types";
import { isQuickAgentSession, isQuickSessionLive, sessionRuntime, targetIdentity, validateQuickCommand, type QuickCommand } from "./quickCommands.ts";

export interface QuickReceipt { targetId: string; title: string; state: "queued" | "text_written" | "started" | "stopped" | "removed" | "failed"; detail?: string; requestId?: string }
export interface QuickExecutionPlan { command: QuickCommand; targets: SessionRecord[]; cwd: string; count: number }
export interface QuickExecutionDeps {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  enabled: (kind: NonNullable<SessionRecord["agentRuntimeKind"]>) => boolean;
  uuid: () => string;
  progress: (receipt: QuickReceipt) => void;
}
export async function executeQuickPlan(plan: QuickExecutionPlan, deps: QuickExecutionDeps): Promise<QuickReceipt[]> {
  const command = validateQuickCommand(plan.command);
  if ((command.action === "send" || command.action === "launch") && Array.from(command.body).length > 32000) throw Error("消息不能超过 32000 个字符");
  if (command.action === "send" && !command.body.trim()) throw Error("请填写消息内容");
  if (command.action === "launch" && (!command.runtimeKind || !plan.cwd.trim() || !Number.isInteger(plan.count) || plan.count < 1 || plan.count > 12)) throw Error("请选择 Agent、工作目录和 1–12 的启动数量");
  if (command.action !== "launch" && (!plan.targets.length || new Set(plan.targets.map(s => s.id)).size !== plan.targets.length)) throw Error("请选择有效且不重复的目标会话");
  const receipts: QuickReceipt[] = [];
  const report = (row: QuickReceipt) => { receipts.push(row); deps.progress(row); };
  if (command.action === "launch") {
    if (!deps.enabled(command.runtimeKind!)) throw Error("目标 Runtime 未启用");
    for (let i = 0; i < plan.count; i++) {
      let launched: SessionRecord | undefined;
      try {
        if (!deps.enabled(command.runtimeKind!)) throw Error("目标 Runtime 已禁用");
        launched = command.providerId
          ? (await deps.invoke<SessionRecord[]>("launch_multi", { providerId: command.providerId, cwds: [plan.cwd], count: 1, yolo: false, autoMode: false, useWorktree: false, initialPrompt: null, projectId: null }))[0]
          : await deps.invoke<SessionRecord>("launch_runtime_session", { runtimeKind: command.runtimeKind, cwd: plan.cwd, projectId: null });
        if (!launched) throw Error("启动接口未返回会话");
        if (command.body.trim()) {
          const requestId = deps.uuid();
          await deps.invoke("send_session_message", { sessionId: launched.id, body: command.body, projectId: null, requestId });
          report({ targetId: launched.id, title: launched.title, state: "queued", requestId, detail: "会话已启动，任务已排队；尚未确认收到或完成" });
        } else report({ targetId: launched.id, title: launched.title, state: "started", detail: "会话已启动；未提交任务" });
      } catch (error) {
        report({ targetId: launched?.id ?? `launch:${i}`, title: launched?.title ?? `启动 ${i + 1}`, state: "failed", detail: `${launched ? "会话已启动，但任务排队失败；会话仍保留。" : ""}${String(error)}` });
      }
    }
    return receipts;
  }
  for (const target of plan.targets) {
    try {
      // Resolve only the confirmed IDs again; never widen the selection after preview.
      const latest = (await deps.invoke<SessionRecord[]>("session_snapshot")).find(s => s.id === target.id);
      if (!latest || targetIdentity(latest) !== targetIdentity(target) || !isQuickAgentSession(latest)) throw Error("目标已删除、归档或身份发生变化，请重新选择");
      const kind = sessionRuntime(latest)!;
      if (command.runtimeKind && kind !== command.runtimeKind) throw Error("目标与指令限定的 Agent 不符");
      if (!deps.enabled(kind)) throw Error("目标 Runtime 未启用");
      if (command.action !== "delete" && !isQuickSessionLive(latest)) throw Error("目标会话已停止，请先恢复");
      if (command.action === "send") {
        const requestId = deps.uuid();
        await deps.invoke("send_session_message", { sessionId: latest.id, body: command.body, projectId: null, requestId });
        report({ targetId: latest.id, title: latest.title, state: "queued", requestId });
      } else {
        await deps.invoke(command.action === "stop" ? "stop_session" : "delete_session", { sessionId: latest.id });
        report({ targetId: latest.id, title: latest.title, state: command.action === "stop" ? "stopped" : "removed" });
      }
    } catch (error) { report({ targetId: target.id, title: target.title, state: "failed", detail: String(error) }); }
  }
  return receipts;
}
export function applyDeliveryEvidence(receipts: QuickReceipt[], evidence: Array<{ id: string; toSessionId: string; stage: string; error?: string | null }>): QuickReceipt[] {
  return receipts.map(row => {
    if (!row.requestId) return row;
    const found = evidence.find(e => e.id === row.requestId && e.toSessionId === row.targetId);
    if (!found) return row;
    if (found.stage === "text_written") return { ...row, state: "text_written", detail: "内容已写入会话；不代表已提交、已读或任务完成" };
    return { ...row, state: "queued", detail: found.error ? `已排队，投递待重试：${found.error}` : "已排队，等待会话可接收" };
  });
}

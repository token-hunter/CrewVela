import { useTranslation } from "../useInterfaceLanguage";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Modal } from "./ui/Modal";
import { SettingsSection } from "./ui/SettingsSection";
import type { MultiAgentSnapshot } from "../types";
import "../styles/collaboration-support.css";
import { HANDOFF_LABELS, handoffRecoveryHistory, scopeHealth, sessionCheckState } from "../collaborationDiagnostics";

interface SessionHealth {
  id: string; title: string; runtime: string; cwd: string; status?: string; projectId?: string | null; archivedAt?: number | null; commandHost?: boolean; ptyId: number | null;
  processRunning: boolean; ready: boolean; reason: string; error: string | null;
  waitingReason: string | null; interactiveHandoff: boolean;
  identityEnvironment: string; bridge: { configured: boolean; error: string | null };
}
export interface HandoffDraft {
  id: string; mode: string; projectId: string | null; sourceId: string; targetId: string;
  context: string; state: string; error: string | null; consequence: string;
}
export interface Health {
  scopeLabel?: string; checkedAt?: number; cliPath: string; cliPathExists: boolean; replyEvidence: string;
  runtimes: Array<{ runtime: string; label: string; supported?: boolean; installed: boolean;
    skills: Array<{ path: string; globalRoot?: boolean; projectIds?: string[]; sessionIds?: string[]; matches: boolean; error: string | null }>; searchedRoots: string[] }>;
  sessions: SessionHealth[];
  messages: Array<{ id: string; toSessionId: string; fromSessionId?: string; at?: number; fromLabel?: string | null; body: string; stage: string; error?: string | null }>;
  handoffs: HandoffDraft[];
}
const PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";
const runtimeLabels = ["Claude Code", "Codex", "Kimi Code", "Pi", "OpenCode"];
const runtimeKinds = ["claude_code", "codex", "kimi_code", "pi", "opencode"];
const STAGES: Record<string, string> = {
  queued: "排队，尚未投递", text_written: "文本已写入，未确认提交", submit_sent: "提交回车已写入，未确认处理",
  bridge_received: "私有桥已确认接收，未确认回复",
};

const fixture: Health = {
  cliPath: "C:\\example\\crewvela\\src-tauri\\target\\release\\CrewVela.exe", cliPathExists: true,
  replyEvidence: "未提供消息级回复关联证据；不将任意终端输出标为已回复",
  runtimes: [...runtimeKinds.map((runtime, i) => ({ runtime, label: runtimeLabels[i], installed: true, supported: true,
    skills: [{ path: `C:/Users/demo/.agents/skills/crewvela/SKILL.md`, matches: i !== 3, error: null }], searchedRoots: [] })), ...[["zcode", "ZCode Desktop"], ["zcode_cli", "ZCode CLI"], ["deepseek_harness", "DeepSeek Harness"]].map(([runtime, label]) => ({ runtime, label, installed: false, supported: false, skills: [], searchedRoots: [] }))],
  sessions: runtimeKinds.map((runtime, i) => ({ id: `preview-session-${i}`, title: runtimeLabels[i], runtime, cwd: "G:\\tmp", ptyId: i + 1,
    status: "running", projectId: "preview-collaboration-project", processRunning: true, ready: i !== 1, reason: i === 1 ? "尚未观察到输入就绪；可能执行中、授权等待或启动中" : "已观察到可接收提示词状态；尚未测试消息往返",
    error: null, waitingReason: null, interactiveHandoff: true, identityEnvironment: "未知：启动逻辑注入，未读取子进程环境确认",
    bridge: { configured: runtime === "pi", error: null } })),
  messages: [{ id: "preview-message", toSessionId: "preview-session-1", fromSessionId: "preview-session-0", at: Date.now(), body: "请核对协作任务", stage: "submit_sent", error: null }], handoffs: [{ id: "preview-handoff-saved", mode: "task", projectId: "preview-collaboration-project", sourceId: "preview-session-0", targetId: "preview-session-1", context: "目标：会话路由与目标校验\n已完成：需求分析\n剩余：实现、验证与交付\n验证：未执行\n工作树：C:/example/crewvela", state: "transferred", error: null, consequence: "上下文已保存；项目暂停待继续" }],
};
const emptyNotes = { goal: "", progress: "", remaining: "", blockers: "", files: "", verification: "" };

export function CollaborationSupportPanel({ snapshot, enabledRuntimes, onChanged, view, request, onHealthChange, onHandoffClose, initialGlobal = false }: {
  snapshot: MultiAgentSnapshot | null; enabledRuntimes: readonly string[]; onChanged: () => void; view: string;
  request: { nonce: number; taskId?: string; recordId?: string } | null;
  onHealthChange: (health: Health | null) => void; onHandoffClose: () => void;
  initialGlobal?: boolean;
}) {
  const t = useTranslation();
  const [rawHealth, setHealth] = useState<Health | null>(PREVIEW ? { ...fixture, checkedAt: Date.now() } : null);
  const [global, setGlobal] = useState(initialGlobal);
  const [diagnosticTab, setDiagnosticTab] = useState("checks");
  const health = rawHealth ? scopeHealth(rawHealth, snapshot, global, enabledRuntimes) : null;
  const refreshInFlight = useRef<Promise<void> | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [checking, setChecking] = useState(false);
  const [busy, setBusyState] = useState(false);
  const busyRef = useRef(false);
  const setBusy = (value: boolean) => { busyRef.current = value; setBusyState(value); };
  const actionEpoch = useRef(0);
  const [mode, setMode] = useState("session");
  const [sourceId, setSourceId] = useState("");
  const [targetId, setTargetId] = useState("");
  const [notes, setNotes] = useState(emptyNotes);
  const [draft, setDraft] = useState<HandoffDraft | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [handoffOpen, setHandoffOpen] = useState(false);
  useEffect(() => { onHealthChange(rawHealth ? scopeHealth(rawHealth, snapshot, global, enabledRuntimes) : null); }, [rawHealth, snapshot, global, enabledRuntimes, onHealthChange]);
  const refresh = useCallback(() => {
    if (PREVIEW) return Promise.resolve();
    if (refreshInFlight.current) return refreshInFlight.current;
    setChecking(true);
    const next = invoke<Health>("collaboration_health").then((value) => { setHealth({ ...value, checkedAt: Date.now() }); })
      .catch((reason) => { setError(String(reason)); })
      .finally(() => { setChecking(false); refreshInFlight.current = null; });
    refreshInFlight.current = next;
    return next;
  }, []);
  useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 15000); return () => window.clearInterval(timer); }, [refresh]);
  useEffect(() => { actionEpoch.current += 1; setBusy(false); setSourceId(""); setTargetId(""); setDraft(null); setConfirmed(false); setError(""); setNotice(""); }, [snapshot?.project.id]);
  useEffect(() => {
    if (!request) return;
    actionEpoch.current += 1; setBusy(false);
    setHandoffOpen(true); setConfirmed(false); setError(""); setNotice("");
    if (request.recordId) { setDraft(rawHealth?.handoffs.find((item) => item.id === request.recordId) ?? null); }
    else { setDraft(null); setNotes(emptyNotes); setTargetId(""); setMode(request.taskId ? "task" : "session"); setSourceId(request.taskId ?? ""); }
  }, [request]);
  const closeHandoff = () => { if (!busyRef.current) { actionEpoch.current += 1; setHandoffOpen(false); onHandoffClose(); } };
  const sessions = rawHealth?.sessions.filter((session) => session.interactiveHandoff && enabledRuntimes.includes(session.runtime)) ?? [];
  const sources = mode === "session" ? sessions.map((session) => ({ id: session.id, label: `${session.title} · ${session.id}` }))
    : snapshot?.tasks.filter((task) => !["done", "waiting_approval", "in_review"].includes(task.status)).map((task) => ({ id: task.id, label: task.title })) ?? [];
  const sourceTask = snapshot?.tasks.find((task) => task.id === sourceId);
  const targets = mode === "session" ? sessions.map((session) => ({ id: session.id, label: `${session.title} · ${session.id}` }))
    : snapshot?.agents.filter((agent) => agent.enabled && enabledRuntimes.includes(agent.runtimeKind) && agent.runtimeKind !== "oh_my_pi" && agent.id !== sourceTask?.assigneeAgentId)
      .map((agent) => ({ id: agent.id, label: agent.name })) ?? [];
  const preview = async () => {
    if (busyRef.current) return;
    const epoch = actionEpoch.current;
    setBusy(true); setError(""); setNotice(""); setConfirmed(false);
    try {
      const input = { mode, projectId: mode === "task" ? snapshot?.project.id : null, sourceId, targetId, ...notes };
      if (PREVIEW) {
        setDraft({ id: "preview-handoff", mode, projectId: input.projectId ?? null, sourceId, targetId, state: "preview", error: null,
          context: `目标：${notes.goal}\n已完成：${notes.progress}\n剩余：${notes.remaining}\n阻塞：${notes.blockers}\n文件：${notes.files}\n验证：${notes.verification}\n\n自动上下文：G:\\tmp；任务日志 / Git 差异统计 / 最近提交（预览数据）`,
          consequence: mode === "task" ? "确认后暂停整个项目，包括其他执行任务和消息轮次；停止完成后变更本任务负责人。项目保持暂停，需手动继续。" : "确认后停止原会话进程树，向所选现有会话排队交接包。原待收消息保留，不自动重启；不要恢复原会话继续已交接工作。" });
      } else { const next = await invoke<HandoffDraft>("collaboration_handoff_preview", { input }); if (epoch === actionEpoch.current) setDraft(next); }
    } catch (reason) { if (epoch === actionEpoch.current) setError(String(reason)); }
    finally { if (epoch === actionEpoch.current) setBusy(false); }
  };
  const autofill = async () => {
    if (busyRef.current) return;
    const epoch = actionEpoch.current;
    setBusy(true); setError(""); setNotice("");
    try {
      const next = PREVIEW
        ? { goal: "修复登录页在深色模式下的对比度问题", progress: "用户指令 3 条；工具调用 12 次（调用完成不代表验证通过）\n最近助手说明（截取）：已修复对比度。", remaining: "剩余：还需在浅色模式回归一次并更新截图。", blockers: "- error: connection refused", files: "- src/login.css（修改×2）", verification: "- `pnpm test --filter login`（最近执行；退出结果未判定）", source: "规则起草（零 LLM）：基于原会话原生历史的确定性提取；工具调用与输出不代表验证通过，请逐项核对并补全。（预览数据）" }
        : await invoke<Record<string, string>>("collaboration_handoff_autofill", { mode, projectId: mode === "task" ? snapshot?.project.id : null, sourceId });
      if (epoch !== actionEpoch.current) return;
      setNotes({ goal: next.goal ?? "", progress: next.progress ?? "", remaining: next.remaining ?? "", blockers: next.blockers ?? "", files: next.files ?? "", verification: next.verification ?? "" });
      setDraft(null); setConfirmed(false);
      setNotice(next.source ?? "已按规则自动起草，请逐项核对后再生成交接预览");
    } catch (reason) { if (epoch === actionEpoch.current) setError(String(reason)); }
    finally { if (epoch === actionEpoch.current) setBusy(false); }
  };
  const confirm = async (item: HandoffDraft) => {
    if (busyRef.current) return;
    const epoch = actionEpoch.current;
    setBusy(true); setError("");
    try {
      const next = PREVIEW ? { ...item, state: item.mode === "task" ? "transferred" : "queued" }
        : await invoke<HandoffDraft>("collaboration_handoff_confirm", { id: item.id, confirmed: true });
      if (epoch !== actionEpoch.current) return;
      setNotice(HANDOFF_LABELS[next.state]); setDraft(null); setConfirmed(false);
      if (PREVIEW) setHealth((current) => current ? { ...current, handoffs: [next, ...current.handoffs] } : current);
      else { await refresh(); onChanged(); }
    } catch (reason) { await refresh(); if (epoch === actionEpoch.current) setError(String(reason)); }
    finally { if (epoch === actionEpoch.current) setBusy(false); }
  };
  const cancel = async (item: HandoffDraft) => {
    if (busyRef.current) return;
    const epoch = actionEpoch.current;
    setBusy(true); setError("");
    try {
      if (!PREVIEW) await invoke("collaboration_handoff_cancel", { id: item.id });
      if (epoch !== actionEpoch.current) return;
      setDraft(null); setNotice("交接已取消；已暂停项目或已停止会话不会自动恢复"); await refresh();
    } catch (reason) { if (epoch === actionEpoch.current) setError(String(reason)); }
    finally { if (epoch === actionEpoch.current) setBusy(false); }
  };

  const shownView = view === "diagnostics" && diagnosticTab === "messages" ? "messages" : view;
  // 独立会话交接与诊断项目范围无关；保留失败交接的恢复入口。
  const handoffHistory = handoffRecoveryHistory(rawHealth, snapshot?.project.id);
  const name = (id?: string) => id ? rawHealth?.sessions.find((session) => session.id === id)?.title ?? id : "外部发送方";
  return <div className="collaboration-support">
    {view === "diagnostics" && <><nav className="collab-tabs" aria-label={t("诊断内容")}>{[["checks", t("环境与会话检查")], ["messages", t("消息投递")]].map(([id, label]) => <button key={id} className={diagnosticTab === id ? "active" : ""} onClick={() => setDiagnosticTab(id)}>{label}</button>)}</nav>
      <div className="collaboration-support-toolbar"><label>{t("检查范围")} <select value={global ? "global" : "project"} onChange={(event) => setGlobal(event.target.value === "global")}><option value="project">{t("当前项目")}</option><option value="global">{t("全局")}</option></select></label><span>{t("仅启用的 Runtime ·")} {global ? t("全部关联会话") : snapshot?.project.name ?? t("未选择项目")} · {health?.checkedAt ? `检查于 ${new Date(health.checkedAt).toLocaleTimeString("zh-CN")}` : t("尚未检查")}</span></div></>}

    {["diagnostics", "messages"].includes(view) && error && <p className="collaboration-support-error" role="alert">{error}</p>}
    {shownView === "diagnostics" && <><SettingsSection title={t("协作健康检查")} description={t("检查 Skill 与当前会话证据；不会发送测试消息或启动 Agent。")}>
      <div className="collaboration-support-toolbar"><button className="btn secondary" disabled={checking || busy} onClick={() => void refresh()}>{checking ? t("检查中…") : t("重新检查")}</button>
        <span>{PREVIEW ? t("演示数据") : t("只读诊断")}</span></div>
      {health && <>
        <p className="collaboration-support-cli">{t("权威 CLI：")}<code>{health.cliPath}</code> · {health.cliPathExists ? t("文件存在") : t("路径不可用")}{t("。子进程实际环境仍需核验。")}</p>
        <div className="collaboration-support-grid">{health.runtimes.map((runtime) => <article key={runtime.runtime}>
          <strong>{runtime.label}</strong><span>{!runtime.supported ? t("未验证：尚未支持此 Runtime 的 Skill 与通道检查") : !runtime.skills.length ? t("Skill 未发现") : runtime.skills.every((skill) => skill.matches) ? t("Skill 内容与内置版本一致") : t("Skill 内容不一致 / 读取失败")}</span>
          <small>{!runtime.supported ? t("检查未覆盖") : runtime.installed ? t("命令可发现") : t("命令未发现")} {t("· 安装成功不代表通信成功")}</small>
          <details><summary>{t("发现路径")}</summary>{runtime.skills.map((skill) => <p key={skill.path}><code>{skill.path}</code>{skill.error && `：${skill.error}`}</p>)}
            {!runtime.skills.length && runtime.searchedRoots.map((root) => <p key={root}><code>{root}</code></p>)}</details>
        </article>)}</div>
        <details className="collaboration-support-details" open><summary>{t("当前会话与投递就绪")}</summary>
          {!health.sessions.length && <p>{t("当前范围没有关联 Agent 会话。")}</p>}
          {health.sessions.map((session) => <article className="collaboration-support-session" key={session.id}>
            <strong>{session.title}</strong><code>{session.id}</code><span>{sessionCheckState(session)}</span>
            <small>{session.commandHost ? t("项目后台执行宿主，不支持直接聊天输入检查") : sessionCheckState(session) === "未验证" ? t("当前检查缺少适用通道或明确状态证据；不判断为故障") : sessionCheckState(session) === "忙碌" ? t("会话正在执行，暂不可接收输入") : sessionCheckState(session) === "已停止" ? t("会话已停止或归档，不作为故障告警") : session.reason}</small><details><summary>{t("身份与桥证据")}</summary><p>{session.cwd} · PTY {session.ptyId ?? t("无")}</p><p>{session.identityEnvironment}</p>
              <p>{session.runtime === "pi" ? session.bridge.configured ? t("私有桥启动配置存在；接收确认见消息记录") : t("未观察到私有桥启动配置") : t("使用 PTY 通道，无需 Pi 私有桥")}</p>
              {(session.error || session.bridge.error || session.waitingReason) && <p role="alert">{session.error ?? session.bridge.error ?? session.waitingReason}</p>}</details>
          </article>)}</details>
      </>}
    </SettingsSection></>}
    {shownView === "messages" && <SettingsSection title={t("消息投递")} description={t("只展示真实投递证据；消息提交不等于模型已处理或回复。")}>
      <button className="btn secondary" disabled={checking || busy} onClick={() => void refresh()}>{checking ? t("检查中…") : t("刷新记录")}</button>
      {health ? <details open className="collaboration-support-details"><summary>{t("消息投递证据（")}{health.messages.length}）</summary><p>{health.replyEvidence}{t("。成功投递记录为本次应用运行内存记录，排队消息持久保留。")}</p>
          {health.messages.map((message) => <article key={message.id}><strong>{STAGES[message.stage] ?? message.stage}</strong><p><time>{message.at ? new Date(message.at).toLocaleString("zh-CN") : t("时间未记录")}</time> {t("· 来源：")}{message.fromLabel ?? name(message.fromSessionId)} {t("→ 接收：")}{name(message.toSessionId)}</p><p>{message.body}</p>{message.error && <p role="alert">{message.error}</p>}</article>)}
          {!health.messages.length && <p>{t("暂无可展示的排队或投递证据。")}</p>}</details> : <p>{t("尚未取得通信记录。")}</p>}
    </SettingsSection>}
    {handoffOpen && <Modal className="collaboration-handoff-dialog" overlayClassName="dialog-overlay collaboration-handoff-overlay" onClose={closeHandoff} ariaLabel={t("交接当前工作")} closeOnOverlayClick={false}>
      <button className="collab-modal-close" aria-label={t("关闭交接窗口")} disabled={busy} onClick={closeHandoff}>×</button>
      <SettingsSection title={t("交接当前工作")} description={t("交接会停止原执行并转交负责人；普通发消息仍使用 CrewVela 会话通信。")}>
      {error && <p className="collaboration-support-error" role="alert">{error}</p>}{notice && <p role="status">{notice}</p>}
      {!draft || !["queued", "transferred", "cancelled"].includes(draft.state) ? <div className="collaboration-support-form">
        <label>{t("交接类型")}<select value={mode} disabled={busy} onChange={(event) => { setMode(event.target.value); setSourceId(""); setTargetId(""); setDraft(null); }}><option value="session">{t("现有交互会话 → 另一现有会话")}</option><option value="task" disabled={!snapshot}>{t("项目任务 → 另一成员")}</option></select></label>
        <label>{t("原会话 / 任务")}<select value={sourceId} disabled={busy} onChange={(event) => { setSourceId(event.target.value); setDraft(null); }}><option value="">{t("选择来源")}</option>{sources.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select></label>
        <label>{t("接收负责人")}<select value={targetId} disabled={busy} onChange={(event) => { setTargetId(event.target.value); setDraft(null); }}><option value="">{t("选择接收方")}</option>{targets.filter((item) => item.id !== sourceId).map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select></label>
        <div className="collaboration-support-toolbar"><button type="button" className="btn secondary" disabled={busy || !sourceId || (mode === "task" && !snapshot)} onClick={() => void autofill()}>{busy ? t("处理中…") : t("自动起草（规则摘要）")}</button><span>{t("零 LLM 确定性提取；起草结果仅为草稿，需逐项核对")}</span></div>
        {Object.entries({ goal: t("目标（必填）"), progress: t("已完成内容"), remaining: t("剩余步骤（必填）"), blockers: t("阻塞点"), files: t("相关文件 / 工作树"), verification: t("已执行验证及实际结果") }).map(([key, label]) => <label key={key}>{label}<textarea rows={2} disabled={busy} value={notes[key as keyof typeof notes]} onChange={(event) => { setNotes({ ...notes, [key]: event.target.value }); setDraft(null); }} /></label>)}
      </div> : null}
      {(!draft || !["queued", "transferred", "cancelled"].includes(draft.state)) && <button className="btn secondary" disabled={busy || !sourceId || !targetId || !notes.goal.trim() || !notes.remaining.trim()} onClick={() => void preview()}>{busy ? t("处理中…") : t("生成交接预览")}</button>}
      {draft && <div className="collaboration-handoff-preview" role="region" aria-label={t("交接预览")}>
        <h4>{t("交接预览 ·")} {HANDOFF_LABELS[draft.state] ?? draft.state}</h4><p className="collaboration-support-consequence">{draft.consequence}</p><pre>{draft.context}</pre>
        {!["queued", "transferred", "cancelled"].includes(draft.state) && <><label className="collaboration-support-confirm"><input type="checkbox" checked={confirmed} disabled={busy} onChange={(event) => setConfirmed(event.target.checked)} />{t("我确认停止范围及接收负责人，并已核对上下文")}</label>
        <div className="collaboration-support-toolbar"><button className="btn" disabled={busy || !confirmed} onClick={() => void confirm(draft)}>{t("确认交接")}</button><button className="btn secondary" disabled={busy} onClick={() => void cancel(draft)}>{t("取消交接")}</button></div></>}
      </div>}
      {!!handoffHistory.length && <details className="collaboration-support-details"><summary>{t("交接记录与恢复（")}{handoffHistory.length}）</summary>{handoffHistory.map((item) => <article key={item.id}>
        <strong>{HANDOFF_LABELS[item.state] ?? item.state}</strong><p><code>{item.sourceId}</code> → <code>{item.targetId}</code></p><p>{item.consequence}</p>{item.error && <p role="alert">{item.error}</p>}
        <details><summary>{t("持久化交接上下文")}</summary><pre>{item.context}</pre><button className="btn secondary" onClick={() => void navigator.clipboard.writeText(item.context).catch((reason) => setError(`复制失败：${String(reason)}`))}>{t("复制上下文")}</button></details>
        {!["queued", "transferred", "cancelled"].includes(item.state) && <div className="collaboration-support-toolbar"><button className="btn secondary" disabled={busy} onClick={() => { setDraft(item); setConfirmed(false); }}>{t("检查并重试")}</button><button className="btn secondary" disabled={busy} onClick={() => void cancel(item)}>{t("取消并解除交接锁")}</button></div>}
      </article>)}</details>}
    </SettingsSection>
    </Modal>}
  </div>;
}

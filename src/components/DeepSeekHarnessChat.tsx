import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowUp, Cpu, FolderSimple, Paperclip, Plus, Target, X } from "@phosphor-icons/react";
import { NativeAgentSelector, NativeAgentWelcome } from "./NativeAgentWelcome";
import type { NativeAgentSwitchProps } from "../nativeAgentSwitch";
import type { HarnessChatMessage, HarnessSessionSnapshot, SessionRecord } from "../types";

interface HarnessEvent {
  sessionId: string;
  sequence: number;
  kind: string;
  data: Record<string, unknown>;
  record: SessionRecord | null;
  snapshot?: HarnessSessionSnapshot;
}

interface ToolCall {
  toolCallId: string;
  title: string;
  status?: string;
  rawInput?: unknown;
}

interface PermissionRequest {
  requestId: string;
  toolCall: { title?: string; kind?: string; rawInput?: unknown };
}

interface ConfigOption {
  id?: string;
  name?: string;
  value?: string;
  currentValue?: string;
  group?: string;
  options?: ConfigOption[];
}

interface Props extends NativeAgentSwitchProps {
  session: SessionRecord;
  preview?: boolean;
  onSessionRecord: (session: SessionRecord) => void;
}

const previewSnapshot: HarnessSessionSnapshot = {
  sessionId: "preview-harness-session",
  connected: true,
  busy: false,
  history: [
    { id: "preview-user-1", role: "user", text: "先检查项目入口和现有测试。", createdAt: 1 },
    { id: "preview-assistant-1", role: "assistant", reasoning: "先确认项目入口，再检查相关测试。这是演示思考内容。", streamStatus: "streaming", text: "我会从应用入口和测试脚本开始检查。当前是 CrewVela UI 预览，没有连接真实 Agent 或调用模型。", createdAt: 2 },
  ],
  pendingPermission: { requestId: "preview-permission", toolCall: { title: "读取项目文件", rawInput: { path: "package.json" } } },
  toolCalls: [{ toolCallId: "preview-read", title: "读取文件", status: "in_progress", rawInput: { path: "package.json" } }],
  sequence: 0,
  controlState: { skills: [{ name: "arch-map", description: "为项目生成架构图谱", invocation: { userInvocable: true, modelInvocable: true } }, { name: "ask-matt", description: "选择适合当前任务的技能", invocation: { userInvocable: true, modelInvocable: false } }], agentStatus: "idle", commands: [{ name: "goal", description: "设置或查看长期任务目标" }, { name: "compact", description: "压缩当前会话上下文" }], permission: { currentValue: "workspace-write", options: [{ value: "read-only", name: "只读" }, { value: "workspace-write", name: "工作区内修改" }, { value: "danger-full-access", name: "完全访问" }] } },
  configOptions: [{ id: "model", type: "select", currentValue: '["preview","model-a"]', options: [{ group: "预览 Provider", options: [
    { name: "预览模型 A", value: '["preview","model-a"]' },
    { name: "预览模型 B", value: '["preview","model-b"]' },
  ] }] }, { id: "reasoning", name: "推理强度", type: "select", currentValue: "high", options: [{ name: "高", value: "high" }, { name: "低", value: "low" }] }],
};

function modelChoices(options: unknown[], id = "model"): { value: string; label: string }[] {
  const root = options.find((item) => (item as ConfigOption)?.id === id) as ConfigOption | undefined;
  const choices: { value: string; label: string }[] = [];
  const walk = (items: ConfigOption[] | undefined, group = "") => {
    for (const item of items ?? []) {
      if (typeof item.value === "string") {
        choices.push({ value: item.value, label: (id === "model" ? item.name : [group || item.group, item.name].filter(Boolean).join(" · ")) || item.value });
      } else {
        walk(item.options, item.group ?? item.name ?? group);
      }
    }
  };
  walk(root?.options);
  return choices;
}

function currentModel(options: unknown[]): string {
  const root = options.find((item) => (item as ConfigOption)?.id === "model") as ConfigOption | undefined;
  return root?.currentValue ?? modelChoices(options)[0]?.value ?? "";
}

function previewMessages(messages: HarnessChatMessage[], id: string, text: string): HarnessChatMessage[] {
  return [...messages, { id, role: "user", text, createdAt: Date.now() }, {
    id: `${id}-reply`, role: "assistant", text: "这是 UI 预览回复；真实回复由本机 DeepSeek Harness 返回。", createdAt: Date.now(),
  }];
}

export function DeepSeekHarnessChat({ session, preview = false, onSessionRecord, active = true, ...switchProps }: Props) {
  const emptyPreview = preview && (new URLSearchParams(window.location.search).get("harness") === "empty" || new URLSearchParams(window.location.search).get("chat") === "empty");
  const [snapshot, setSnapshot] = useState<HarnessSessionSnapshot | null>(preview ? emptyPreview ? { ...previewSnapshot, history: [], toolCalls: [], pendingPermission: null } : previewSnapshot : null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const sendInFlight = useRef(false);
  const [attachments, setAttachments] = useState<string[]>([]);
  const [commandMenu, setCommandMenu] = useState(false);
  const [browseAllSkills, setBrowseAllSkills] = useState(false);
  const [activeSkillIndex, setActiveSkillIndex] = useState(0);
  const [controlling, setControlling] = useState(false);
  const [goalMode, setGoalMode] = useState(false);
  const [error, setError] = useState("");
  const [connecting, setConnecting] = useState(!preview);
  const [savingModel, setSavingModel] = useState(false);
  const [selectedModel, setSelectedModel] = useState(preview ? currentModel(previewSnapshot.configOptions) : "");
  const [toolCalls, setToolCalls] = useState<ToolCall[]>(preview && !emptyPreview ? [{
    toolCallId: "preview-read", title: "读取文件", status: "in_progress", rawInput: { path: "package.json" },
  }] : []);
  const [permission, setPermission] = useState<PermissionRequest | null>(preview && !emptyPreview ? {
    requestId: "preview-permission", toolCall: { title: "读取项目文件", rawInput: { path: "package.json" } },
  } : null);
  const sequence = useRef(0);
  const revision = useRef(0);
  const messagesEnd = useRef<HTMLDivElement>(null);
  const composerInput = useRef<HTMLTextAreaElement>(null);
  const messages = snapshot?.history ?? [];
  const empty = messages.length === 0 && toolCalls.length === 0 && !permission && !snapshot?.controlState?.goal;
  const matchingSkills = (snapshot?.controlState?.skills ?? []).filter((skill) => `${skill.name} ${skill.description}`.toLowerCase().includes(!browseAllSkills && draft.startsWith("/") ? draft.slice(1).toLowerCase() : ""));
  const activeSkill = matchingSkills[Math.min(activeSkillIndex, Math.max(0, matchingSkills.length - 1))];
  const invokedSkill = (snapshot?.controlState?.skills ?? []).find((skill) => !goalMode && draft.startsWith(`/${skill.name} `));
  const draftRemainder = invokedSkill ? draft.slice(invokedSkill.name.length + 2) : draft;
  const choices = useMemo(() => modelChoices(snapshot?.configOptions ?? []), [snapshot?.configOptions]);

  useEffect(() => { setActiveSkillIndex(0); }, [draft, browseAllSkills, commandMenu]);
  useEffect(() => {
    if (active && commandMenu) document.getElementById(`harness-skill-${session.id}-${activeSkillIndex}`)?.scrollIntoView({ block: "nearest" });
  }, [active, activeSkillIndex, commandMenu, session.id]);

  const completeSkill = (name: string) => {
    setGoalMode(false);
    const remainder = draft.startsWith("/") ? draft.replace(/^\/[^\s]*\s*/, "") : draft;
    setDraft(`/${name} ${remainder}`);
    setCommandMenu(false);
    setBrowseAllSkills(false);
    composerInput.current?.focus();
  };

  const beginGoal = () => {
    setGoalMode(true);
    setDraft(snapshot?.controlState?.goal?.objective ?? (draft.startsWith("/") ? "" : draft));
    setCommandMenu(false);
    setBrowseAllSkills(false);
    composerInput.current?.focus();
  };

  const applySnapshot = useCallback((next: HarnessSessionSnapshot) => {
    if (next.sessionId !== session.id || next.sequence < sequence.current) return;
    sequence.current = next.sequence;
    setSnapshot(next);
    setSelectedModel(currentModel(next.configOptions));
    setToolCalls(next.toolCalls as ToolCall[]);
    setPermission(next.pendingPermission as PermissionRequest | null);
  }, [session.id]);

  const applyEvent = useCallback((event: HarnessEvent) => {
    if (event.sessionId !== session.id || event.sequence <= sequence.current) return;
    sequence.current = event.sequence;
    revision.current += 1;
    if (event.record) onSessionRecord(event.record);
    if (event.snapshot) {
      applySnapshot(event.snapshot);
      if (event.kind === "error") setError(String(event.data.message ?? "DeepSeek Harness 执行失败"));
      return;
    }
    switch (event.kind) {
      case "state": {
        const status = event.data.status;
        if (status !== "waiting") setPermission(null);
        setSnapshot((current) => current ? {
          ...current,
          connected: status === "running" || status === "waiting",
          busy: status === "waiting" ? true : status === "stopped" || status === "error" ? false : current.busy,
          configOptions: Array.isArray(event.data.configOptions) ? event.data.configOptions : current.configOptions,
        } : current);
        break;
      }
      case "user_message":
        setToolCalls([]);
        setPermission(null);
        setSnapshot((current) => current ? {
          ...current,
          busy: true,
          history: [...current.history, { id: String(event.data.id), role: "user", text: String(event.data.text), createdAt: Date.now() }],
        } : current);
        break;
      case "assistant_delta":
        setSnapshot((current) => {
          if (!current) return current;
          const id = String(event.data.id);
          const text = String(event.data.text);
          const existing = current.history.findIndex((message) => message.id === id && message.role === "assistant");
          const history = current.history.slice();
          if (existing >= 0) history[existing] = { ...history[existing], text: history[existing].text + text };
          else history.push({ id, role: "assistant", text, createdAt: Date.now() });
          return { ...current, history };
        });
        break;
      case "tool_call": {
        const item = event.data as unknown as ToolCall;
        setToolCalls((current) => current.some((call) => call.toolCallId === item.toolCallId)
          ? current.map((call) => call.toolCallId === item.toolCallId ? { ...call, ...item } : call)
          : [...current, item]);
        break;
      }
      case "permission_request":
        setPermission(event.data as unknown as PermissionRequest);
        break;
      case "turn_end":
        setSnapshot((current) => current ? { ...current, busy: false } : current);
        setPermission(null);
        break;
      case "error":
        setPermission(null);
        setError(String(event.data.message ?? "DeepSeek Harness 执行失败"));
        setSnapshot((current) => current ? { ...current, connected: false, busy: false } : current);
        break;
      default:
        break;
    }
  }, [applySnapshot, onSessionRecord, session.id]);

  const connect = useCallback(async () => {
    if (preview) return;
    setConnecting(true);
    setError("");
    try {
      const next = await invoke<HarnessSessionSnapshot>("start_deepseek_harness_session", { sessionId: session.id });
      applySnapshot(next);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setConnecting(false);
    }
  }, [applySnapshot, preview, session.id]);

  useEffect(() => {
    if (preview) return;
    let disposed = false;
    let unlistenHarness: (() => void) | undefined;
    let unlistenCloseError: (() => void) | undefined;
    const setup = async () => {
      unlistenHarness = await listen<HarnessEvent>("deepseek-harness-update", ({ payload }) => { if (!disposed) applyEvent(payload); });
      if (disposed) { unlistenHarness(); return; }
      unlistenCloseError = await listen<string>("deepseek-harness-error", ({ payload }) => { if (!disposed) setError(payload); });
      if (disposed) {
        unlistenHarness();
        unlistenCloseError();
        return;
      }
      try {
        const initial = await invoke<HarnessSessionSnapshot>("deepseek_harness_snapshot", { sessionId: session.id });
        if (!disposed) applySnapshot(initial);
      } catch (reason) {
        if (!disposed) setError(String(reason));
      }
      if (!disposed) await connect();
    };
    void setup().catch((reason) => {
      unlistenHarness?.();
      unlistenCloseError?.();
      if (!disposed) { setError(String(reason)); setConnecting(false); }
    });
    return () => {
      disposed = true;
      unlistenHarness?.();
      unlistenCloseError?.();
    };
  }, [applyEvent, applySnapshot, connect, preview, session.id]);

  useEffect(() => {
    if (active) messagesEnd.current?.scrollIntoView({ block: "end" });
  }, [active, messages]);

  const send = async (event: React.FormEvent) => {
    event.preventDefault();
    if (sendInFlight.current) return;
    const text = draft.trim();
    if ((!text && attachments.length === 0) || snapshot?.busy || !snapshot?.connected || controlling) return;
    setError("");
    if (goalMode) {
      if (!text) return;
      if (attachments.length) { setError("当前控制命令不支持文件附件；请先用普通消息发送附件。"); return; }
      if (await runControl("command", `/goal ${snapshot?.controlState?.goal ? "edit " : ""}${text}`)) {
        setDraft(current => current === draft ? "" : current);
        setGoalMode(false);
      }
      return;
    }
    if (text.startsWith("/")) {
      const match = /^\/([a-z0-9][a-z0-9_-]*)(?=$|\s)/.exec(text);
      const name = match?.[1];
      if (name === "file" && text === "/file") { setDraft(""); await addFiles(); return; }
      if (name === "model" && text === "/model") { document.querySelector<HTMLSelectElement>('[aria-label="DeepSeek Harness 模型"]')?.focus(); setDraft(""); return; }
      if (name === "permission" && text === "/permission") { document.querySelector<HTMLSelectElement>('[aria-label="DeepSeek Harness 权限"]')?.focus(); setDraft(""); return; }
      if (snapshot?.controlState?.commands?.some((command) => command.name === name)) {
        if (attachments.length) { setError("当前控制命令不支持文件附件；请先用普通消息发送附件。"); return; }
        if (await runControl("command", text)) setDraft(current => current === draft ? "" : current);
        return;
      }
      if (!snapshot?.controlState?.skills?.some((skill) => skill.name === name)) { setError("当前会话不支持该技能或斜杠命令；没有发送给模型。"); return; }
    }
    if (preview) {
      setSnapshot((current) => current ? { ...current, history: previewMessages(current.history, `preview-${Date.now()}`, text) } : current);
      setDraft("");
      setAttachments([]);
      setCommandMenu(false);
      return;
    }
    sendInFlight.current = true;
    setSending(true);
    try {
      const before = revision.current;
      await invoke("send_deepseek_harness_message", { sessionId: session.id, message: text, attachments });
      if (revision.current === before) setSnapshot((current) => current ? { ...current, busy: true } : current);
      setDraft(current => current === draft ? "" : current);
      setAttachments(current => current.filter(path => !attachments.includes(path)));
      setCommandMenu(false);
    } catch (reason) {
      setError(String(reason));
    } finally {
      sendInFlight.current = false;
      setSending(false);
    }
  };

  const addFiles = async () => {
    if (preview) { setAttachments(["预览附件.txt"]); return; }
    try {
      const selected = await open({ multiple: true, directory: false, title: "添加 DeepSeek Harness 文件" });
      if (!selected) return;
      const paths = typeof selected === "string" ? [selected] : selected;
      setAttachments((current) => [...new Set([...current, ...paths])]);
    } catch (reason) { setError(String(reason)); }
  };

  const runControl = async (action: "command" | "permission", value: string) => {
    if (preview) {
      setError(""); setCommandMenu(false);
      setSnapshot((current) => {
        if (!current?.controlState) return current;
        const controlState = { ...current.controlState };
        if (action === "permission" && controlState.permission) {
          controlState.permission = { ...controlState.permission, currentValue: value };
        } else if (value === "/goal clear") controlState.goal = null;
        else if (value === "/goal pause" && controlState.goal) controlState.goal = { ...controlState.goal, phase: "paused", activation: "disarmed" };
        else if (value === "/goal resume" && controlState.goal) controlState.goal = { ...controlState.goal, phase: "active", activation: "armed" };
        else if (value.startsWith("/goal edit ") && controlState.goal) controlState.goal = { ...controlState.goal, objective: value.slice(11) };
        else if (value.startsWith("/goal ")) controlState.goal = { objective: value.slice(6), phase: "active", activation: "armed", roundsStarted: 1, maxGoalRounds: 10 };
        controlState.execution = { commandId: "preview", result: { kind: "success", text: "UI 演示：未调用 Harness 或模型。" } };
        return { ...current, controlState };
      });
      return true;
    }
    setControlling(true); setError(""); setCommandMenu(false);
    try {
      const next = await invoke<HarnessSessionSnapshot>("control_deepseek_harness_session", { sessionId: session.id, action, value });
      applySnapshot(next);
      if (next.controlState?.execution?.result.kind === "error") { setError(next.controlState.execution.result.text ?? "Harness 命令失败"); return false; }
      return true;
    } catch (reason) { setError(String(reason)); return false; }
    finally { setControlling(false); }
  };

  const answerPermission = async (allow: boolean) => {
    if (!permission) return;
    const requestId = permission.requestId;
    if (preview) {
      setPermission(null);
      setToolCalls((current) => current.map((call) => ({ ...call, status: allow ? "completed" : "failed" })));
      return;
    }
    try {
      await invoke("answer_deepseek_harness_permission", {
        sessionId: session.id, requestId, allow,
      });
      setPermission((current) => current?.requestId === requestId ? null : current);
    } catch (reason) {
      setError(String(reason));
    }
  };

  const stop = async () => {
    if (preview) return;
    try {
      const updated = await invoke<SessionRecord>("stop_session", { sessionId: session.id });
      onSessionRecord(updated);
      setSnapshot((current) => current ? { ...current, connected: false, busy: false } : current);
      setPermission(null);
    } catch (reason) {
      setError(String(reason));
    }
  };

  const cancel = async () => {
    if (preview) {
      setSnapshot((current) => current ? { ...current, busy: false } : current);
      setPermission(null);
      return;
    }
    try {
      await invoke("cancel_deepseek_harness_turn", { sessionId: session.id });
    } catch (reason) {
      setError(String(reason));
    }
  };

  const changeModel = async (value: string, configId = "model") => {
    if (configId === "model") setSelectedModel(value);
    if (preview) {
      setSnapshot((current) => current ? { ...current, configOptions: current.configOptions.map((raw) => (raw as ConfigOption).id === configId ? { ...(raw as ConfigOption), currentValue: value } : raw) } : current);
      return;
    }
    setSavingModel(true);
    setError("");
    try {
      const next = await invoke<HarnessSessionSnapshot>("set_deepseek_harness_model", { sessionId: session.id, value, configId });
      applySnapshot(next);
    } catch (reason) {
      setError(String(reason));
      if (snapshot) setSelectedModel(currentModel(snapshot.configOptions));
    } finally {
      setSavingModel(false);
    }
  };

  return (
    <section className={`harness-chat native-agent-chat${empty ? " harness-chat-empty" : ""}`} aria-label="DeepSeek Harness 会话">
      <header className="harness-chat-header">
        <div className="harness-chat-controls"><span className="harness-workspace" title={session.cwd}><FolderSimple size={17} />工作目录：{session.cwd.split(/[\\/]/).filter(Boolean).pop() || session.cwd}</span><span className={`harness-connection${snapshot?.connected ? " connected" : ""}`}><i />{connecting ? "连接中" : snapshot?.connected ? "已连接" : "未连接"}</span>
          {snapshot?.connected && !preview && <button type="button" onClick={() => void stop()}>停止会话</button>}
          {!snapshot?.connected && !connecting && <button type="button" onClick={() => void connect()}>恢复会话</button>}
        </div>
      </header>
      {preview && <div className="harness-chat-preview-banner">UI 预览：不会连接真实 Agent、调用模型或执行工具。可交互内容为演示数据。</div>}
      <NativeAgentSelector kind="deepseek_harness" {...switchProps} />

      {snapshot?.controlState?.goal && <section className="harness-goal" aria-label="Harness 长期目标"><strong>目标 · {snapshot.controlState.goal.phase}</strong><p>{snapshot.controlState.goal.objective}</p><span>轮次 {snapshot.controlState.goal.roundsStarted ?? 0}/{snapshot.controlState.goal.maxGoalRounds ?? "—"}</span><div>
        <button type="button" disabled={controlling || snapshot.busy} onClick={beginGoal}>编辑目标</button>
        <button type="button" disabled={controlling} onClick={() => void runControl("command", snapshot.controlState?.goal?.phase === "active" && snapshot.controlState.goal.activation === "armed" ? "/goal pause" : "/goal resume")}>{snapshot.controlState.goal.phase === "active" && snapshot.controlState.goal.activation === "armed" ? "暂停目标" : "恢复目标"}</button>
        <button type="button" disabled={controlling} onClick={() => void runControl("command", "/goal clear")}>清除目标</button>
      </div></section>}
      {snapshot?.controlState?.execution?.result.text && <div className="harness-command-result" role="status"><strong>Harness 命令结果</strong><pre>{snapshot.controlState.execution.result.text}</pre></div>}
      <div className="harness-chat-messages" aria-live="polite">
        {empty && <NativeAgentWelcome kind="deepseek_harness" active={active} skillsDisabled={!snapshot?.connected || controlling} onSkills={() => { setBrowseAllSkills(true); setCommandMenu(true); composerInput.current?.focus(); }} />}

        {messages.map((message) => <article className={`harness-message ${message.role}`} key={`${message.role}-${message.id}`}>
          <strong>{message.role === "user" ? "你" : "DeepSeek Harness"}</strong>
          {message.reasoning && <details className="harness-reasoning" open={message.streamStatus === "streaming"}><summary>{message.streamStatus === "streaming" ? "正在思考" : "思考过程"}</summary><p>{message.reasoning}</p></details>}
          {(message.text || !message.reasoning) && <p>{message.text || "…"}</p>}
          {message.streamStatus && message.streamStatus !== "completed" && <small className="harness-stream-status">{({ streaming: "生成中…", interrupted: "已取消，显示部分内容", failed: "本次尝试失败", abandoned: "本次尝试未完成" } as Record<string, string>)[message.streamStatus]}</small>}
          {(message.attachments ?? []).map((file) => <div className="harness-attachment" key={file.uri} title={file.path}><Paperclip size={14} /> {file.name}</div>)}
        </article>)}
        {toolCalls.length > 0 && <div className="harness-tool-list">
          {toolCalls.map((tool) => <article className="harness-tool" key={tool.toolCallId}>
            <strong>{tool.title || "工具调用"}</strong><span>{tool.status || "已提交"}</span>
            {tool.rawInput !== undefined && <pre>{typeof tool.rawInput === "string" ? tool.rawInput : JSON.stringify(tool.rawInput, null, 2)}</pre>}
          </article>)}
        </div>}
        {permission && <article className="harness-permission">
          <strong>需要确认：{permission.toolCall.title ?? "工具调用"}</strong>
          {permission.toolCall.rawInput !== undefined && <pre>{typeof permission.toolCall.rawInput === "string" ? permission.toolCall.rawInput : JSON.stringify(permission.toolCall.rawInput, null, 2)}</pre>}
          <div><button type="button" onClick={() => void answerPermission(true)}>允许一次</button><button type="button" onClick={() => void answerPermission(false)}>拒绝</button></div>
        </article>}
        {!connecting && !snapshot?.connected && <div className="harness-chat-info">会话已停止或未连接。点击“恢复会话”继续原会话。</div>}
        <div ref={messagesEnd} />
      </div>
      {error && <pre className="harness-chat-error" role="alert">{error}</pre>}
      <form className="harness-chat-composer" onSubmit={(event) => void send(event)}>
        {commandMenu && <div className="harness-command-menu" role="menu" aria-label="Harness 命令">
          <button type="button" role="menuitem" onClick={() => { setCommandMenu(false); void addFiles(); }}>文件 file <small>添加本地文件引用</small></button>
          {(snapshot?.controlState?.commands ?? []).map((command) => <button type="button" role="menuitem" key={command.name} onClick={() => { setCommandMenu(false); if (command.name === "goal") beginGoal(); else { setGoalMode(false); setDraft(`/${command.name}`); } }}>{command.name === "goal" ? "目标" : "压缩"} {command.name}<small>{command.description}</small></button>)}
          <div className="harness-skill-menu" aria-label="Harness 技能"><strong>技能 <span className="harness-skill-key-hint">↑↓ 选择 · Tab 补全</span></strong>{matchingSkills.map((skill, index) => <button type="button" role="menuitem" id={`harness-skill-${session.id}-${index}`} className={index === activeSkillIndex ? "harness-skill-active" : ""} key={skill.name} onMouseEnter={() => setActiveSkillIndex(index)} onClick={() => completeSkill(skill.name)}><span aria-hidden="true">✨</span><span className="harness-skill-name">/{skill.name}</span><small>{!skill.invocation.modelInvocable && "仅用户 · "}{skill.description}</small></button>)}{matchingSkills.length === 0 && <small>{snapshot?.controlState?.skills?.length ? "没有匹配技能" : "当前会话没有可供用户调用的技能"}</small>}</div>
        </div>}
        {attachments.length > 0 && <div className="harness-attachments">{attachments.map((path) => <button type="button" key={path} title={path} aria-label={`移除附件 ${path.split(/[\\/]/).pop()}`} onClick={() => setAttachments((current) => current.filter((item) => item !== path))}><Paperclip size={14} /><span>{path.split(/[\\/]/).pop()}</span><X size={12} /></button>)}</div>}
        <div className={invokedSkill ? "harness-composer-input harness-composer-input-command" : "harness-composer-input"}>
        {goalMode && <span className="harness-goal-prefix" aria-label="目标模式"><Target size={18} aria-hidden="true" />Goal</span>}
        {invokedSkill && <span className="harness-input-command-chip" title={invokedSkill.description || invokedSkill.name} aria-label="已识别技能"><span aria-hidden="true">✨</span>/{invokedSkill.name}</span>}
        <textarea ref={composerInput} aria-label="发送给 DeepSeek Harness" aria-controls={commandMenu ? `harness-skill-${session.id}-${activeSkillIndex}` : undefined} value={invokedSkill ? draftRemainder : draft} onChange={(event) => { const value = event.target.value; setBrowseAllSkills(false); setDraft(invokedSkill ? `/${invokedSkill.name} ${value}` : value); setCommandMenu(!goalMode && !invokedSkill && /^\/[^\s]*$/.test(value)); }} placeholder={goalMode || invokedSkill ? "" : "描述你想完成的任务，输入 / 调用技能…"} disabled={!snapshot?.connected || snapshot.busy || connecting || controlling} onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.keyCode === 229) return;
          if (commandMenu && activeSkill && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setActiveSkillIndex((index) => (index + (event.key === "ArrowDown" ? 1 : -1) + matchingSkills.length) % matchingSkills.length);
              return;
            }
            if (event.key === "Tab") { event.preventDefault(); completeSkill(activeSkill.name); return; }
          }
          if (event.key === "Escape") { event.stopPropagation(); setCommandMenu(false); setGoalMode(false); }
          if (event.key === "Backspace" && goalMode && !draft) { event.preventDefault(); setGoalMode(false); }
          if (event.key === "Backspace" && invokedSkill && !draftRemainder) { event.preventDefault(); setDraft(""); return; }
          if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); }
        }} />
        </div>
        <div className="harness-composer-toolbar"><div className="harness-composer-left">
          <button type="button" className="harness-icon-button" aria-label="功能菜单" title="功能菜单" aria-expanded={commandMenu} disabled={!snapshot?.connected || controlling} onClick={() => { setBrowseAllSkills(false); setCommandMenu((current) => !current); }}><Plus size={21} /></button>
          <button type="button" className="harness-icon-button" aria-label="添加文件" title="添加文件" disabled={!snapshot?.connected || snapshot.busy || connecting} onClick={() => void addFiles()}><Paperclip size={21} /></button>
          <label className="harness-picker harness-permission-picker"><FolderSimple size={18} /><select aria-label="DeepSeek Harness 权限" value={snapshot?.controlState?.permission?.currentValue ?? ""} disabled={!snapshot?.connected || snapshot.busy || controlling} onChange={(event) => void runControl("permission", event.target.value)}>
            {(snapshot?.controlState?.permission?.options ?? []).map((option) => <option key={option.value} value={option.value}>{option.name}</option>)}
            {!snapshot?.controlState?.permission?.options.some((option) => option.value === "auto") && <option value="auto" disabled>Auto review（需兼容插件）</option>}
          </select></label>
        </div><div className="harness-composer-right">
          {choices.length > 0 && <label className="harness-picker harness-model-picker">
            <select aria-label="DeepSeek Harness 模型" value={selectedModel || currentModel(snapshot?.configOptions ?? [])} disabled={!snapshot?.connected || snapshot.busy || savingModel} onChange={(event) => void changeModel(event.target.value)}>
              {choices.map((choice) => <option key={choice.value} value={choice.value}>{choice.label}</option>)}
            </select>
          </label>}
          {(snapshot?.configOptions ?? []).filter((option) => (option as ConfigOption)?.id !== "model").map((raw) => {
            const option = raw as ConfigOption;
            const options = modelChoices(snapshot?.configOptions ?? [], option.id);
            return option.id && options.length > 0 ? <label className="harness-picker harness-reasoning-picker" key={option.id}><span className="harness-reasoning-tooltip" role="tooltip" id={`harness-reasoning-tooltip-${session.id}-${option.id}`}>思考级别</span><Cpu size={17} /><select aria-describedby={`harness-reasoning-tooltip-${session.id}-${option.id}`} aria-label={`DeepSeek Harness ${option.id}`} value={option.currentValue ?? ""} disabled={!snapshot?.connected || snapshot.busy || savingModel || controlling} onChange={(event) => void changeModel(event.target.value, option.id)}>{options.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label> : null;
          })}
          {(snapshot?.busy || controlling) && <button type="button" className="harness-cancel-button" onClick={() => void cancel()}>取消本轮</button>}
          <button type="submit" className="harness-send-button" aria-label="发送" title="发送" disabled={sending || (!draft.trim() && (goalMode || attachments.length === 0)) || !snapshot?.connected || snapshot.busy || connecting || controlling}><ArrowUp size={24} /></button>
        </div></div>
        <div className="harness-composer-hint">{snapshot?.busy ? "正在执行" : snapshot?.connected ? "Enter 发送 · Shift+Enter 换行" : "恢复会话后开始任务"}</div>
      </form>
    </section>
  );
}

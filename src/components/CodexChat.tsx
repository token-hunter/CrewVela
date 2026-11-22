import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowUp, Cpu, FolderSimple, Paperclip, Plus, PuzzlePiece, Target, X } from "@phosphor-icons/react";
import { NativeAgentSelector, NativeAgentWelcome } from "./NativeAgentWelcome";
import { CodexContextUsage } from "./CodexContextUsage";
import { CodexGoalPanel } from "./CodexGoalPanel";
import { CodexPluginPicker } from "./CodexPluginPicker";
import "../styles/codex-composer.css";
import { ChatTurnChangesCard } from "./ChatTurnChangesCard";
import type { CodexPlugin } from "../codexPlugins";
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
  options?: { optionId: string; name: string; kind: string }[];
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
  sessionId: "preview-codex-session",
  connected: true,
  busy: false,
  history: [
    { id: "preview-user-1", role: "user", text: "帮我看一下这个模块的测试为什么挂了。", createdAt: 1 },
    { id: "preview-assistant-1", role: "assistant", reasoning: "先定位失败的测试，再检查最近的改动。这是演示思考内容。", streamStatus: "streaming", text: "我先运行失败的测试并查看相关改动。当前是 CrewVela UI 预览，没有连接真实 Agent 或调用模型。", createdAt: 2 },
  ],
  pendingPermission: { requestId: "preview-permission", toolCall: { title: "运行测试", rawInput: { command: "cargo test -p core" } }, options: [{ optionId: "allow", name: "允许一次", kind: "allow_once" }, { optionId: "reject", name: "拒绝", kind: "reject_once" }] },
  toolCalls: [{ toolCallId: "preview-run", title: "运行测试", status: "in_progress", rawInput: { command: "cargo test -p core" } }],
  sequence: 0,
  controlState: {},
  codexGoalSupported: true,
  configOptions: [
    { id: "model", type: "select", currentValue: "gpt-5-codex", options: [{ group: "Codex", options: [{ name: "GPT-5 Codex", value: "gpt-5-codex" }, { name: "GPT-5", value: "gpt-5" }] }] },
  ],
  commands: [
    { name: "model", description: "切换模型", inputHint: "模型名" },
    { name: "clear", description: "清空当前 Codex 上下文并开始新会话" },
  ],
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

function currentModel(options: unknown[], id = "model"): string {
  const root = options.find((item) => (item as ConfigOption)?.id === id) as ConfigOption | undefined;
  return root?.currentValue ?? modelChoices(options, id)[0]?.value ?? "";
}

// codex-acp 的命令两类：内置斜杠命令裸名（调用 /name），技能名自带 $（调用 $name）。
function commandInvocation(name: string): string {
  return name.startsWith("$") ? name : `/${name}`;
}

function commandBareName(name: string): string {
  return name.startsWith("$") ? name.slice(1) : name;
}

export function CodexChat({ session, preview = false, onSessionRecord, active = true, ...switchProps }: Props) {
  const [snapshot, setSnapshot] = useState<HarnessSessionSnapshot | null>(preview ? new URLSearchParams(location.search).get("chat") === "empty" ? { ...previewSnapshot, history: [], toolCalls: [], pendingPermission: null } : previewSnapshot : null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const sendInFlight = useRef(false);
  const [attachments, setAttachments] = useState<string[]>([]);
  const [commandMenu, setCommandMenu] = useState(false);
  const [goalOpen, setGoalOpen] = useState(false);
  const [pluginOpen, setPluginOpen] = useState(false);
  const [plugins, setPlugins] = useState<CodexPlugin[]>([]);
  useEffect(() => { setPlugins([]); setPluginOpen(false); }, [session.id]);
  const [browseAllCommands, setBrowseAllCommands] = useState(false);
  const [activeCommandIndex, setActiveCommandIndex] = useState(0);
  const [error, setError] = useState("");
  const [connecting, setConnecting] = useState(!preview);
  const [savingModel, setSavingModel] = useState(false);
  const [selectedModel, setSelectedModel] = useState(preview ? currentModel(previewSnapshot.configOptions) : "");
  const [toolCalls, setToolCalls] = useState<ToolCall[]>([]);
  const [permission, setPermission] = useState<PermissionRequest | null>(null);
  const sequence = useRef(0);
  const revision = useRef(0);
  const messagesEnd = useRef<HTMLDivElement>(null);
  const composerInput = useRef<HTMLTextAreaElement>(null);
  const messages = snapshot?.history ?? [];
  const turnChanges = new Map(snapshot?.turnChanges ?? []);
  const replies = new Set(messages.filter(message => message.role === "assistant").map(message => message.id));
  const goalActive = snapshot?.codexGoal?.status === "active";
  const empty = messages.length === 0 && toolCalls.length === 0 && !permission;
  const commands = snapshot?.commands ?? [];
  const commandQuery = /^[$/]/.test(draft) ? draft.slice(1).toLowerCase() : "";
  const matchingCommands = commands.filter((command) => {
    const haystack = `${commandBareName(command.name)} ${command.name} ${command.description ?? ""}`.toLowerCase();
    return haystack.includes(commandQuery);
  });
  const activeCommand = matchingCommands[Math.min(activeCommandIndex, Math.max(0, matchingCommands.length - 1))];
  const modelList = useMemo(() => modelChoices(snapshot?.configOptions ?? []), [snapshot?.configOptions]);
  const otherConfigOptions = (snapshot?.configOptions ?? []).filter((option) => {
    const id = (option as ConfigOption)?.id;
    return id && id !== "model" && modelChoices(snapshot?.configOptions ?? [], id).length > 0;
  });

  useEffect(() => { setActiveCommandIndex(0); }, [draft, browseAllCommands, commandMenu]);
  useEffect(() => {
    if (active && commandMenu) document.getElementById(`codex-command-${session.id}-${activeCommandIndex}`)?.scrollIntoView({ block: "nearest" });
  }, [active, activeCommandIndex, commandMenu, session.id]);

  const completeCommand = (name: string) => {
    const remainder = /^[$/][^\s]*\s*/.test(draft) ? draft.replace(/^[$/][^\s]*\s*/, "") : draft;
    setDraft(`${commandInvocation(name)} ${remainder}`.trimEnd());
    setCommandMenu(false);
    setBrowseAllCommands(false);
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
    if (event.kind === "turn_changes_unavailable") setError(String(event.data.message ?? "无法记录本轮改动"));
    if (event.snapshot) {
      applySnapshot(event.snapshot);
      if (event.kind === "error" || event.kind === "codex_goal_error") setError(String(event.data.message ?? "Codex Chat 执行失败"));
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
          const text = String(event.data.text ?? "");
          const reasoning = String(event.data.reasoning ?? "");
          const existing = current.history.findIndex((message) => message.id === id && message.role === "assistant");
          const history = current.history.slice();
          if (existing >= 0) history[existing] = { ...history[existing], text: history[existing].text + text, reasoning: (history[existing].reasoning ?? "") + reasoning };
          else history.push({ id, role: "assistant", text, reasoning, createdAt: Date.now() });
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
      case "context_usage":
        setSnapshot((current) => current ? { ...current, contextUsage: typeof event.data.used === "number" && typeof event.data.size === "number" ? { used: event.data.used, size: event.data.size } : null } : current);
        break;
      case "codex_goal_error":
        setError(String(event.data.message ?? "目标操作失败"));
        break;
      case "error":
        setPermission(null);
        setError(String(event.data.message ?? "Codex Chat 执行失败"));
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
      const next = await invoke<HarnessSessionSnapshot>("start_codex_chat_session", { sessionId: session.id });
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
    let unlistenChat: (() => void) | undefined;
    let unlistenCloseError: (() => void) | undefined;
    const setup = async () => {
      unlistenChat = await listen<HarnessEvent>("codex-chat-update", ({ payload }) => { if (!disposed) applyEvent(payload); });
      if (disposed) { unlistenChat(); return; }
      unlistenCloseError = await listen<string>("codex-chat-error", ({ payload }) => { if (!disposed) setError(payload); });
      if (disposed) {
        unlistenChat();
        unlistenCloseError();
        return;
      }
      try {
        const initial = await invoke<HarnessSessionSnapshot>("codex_chat_snapshot", { sessionId: session.id });
        if (!disposed) applySnapshot(initial);
      } catch (reason) {
        if (!disposed) setError(String(reason));
      }
      if (!disposed) await connect();
    };
    void setup().catch((reason) => {
      unlistenChat?.();
      unlistenCloseError?.();
      if (!disposed) { setError(String(reason)); setConnecting(false); }
    });
    return () => {
      disposed = true;
      unlistenChat?.();
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
    if ((!text && attachments.length === 0 && plugins.length === 0) || snapshot?.busy || !snapshot?.connected || goalActive) return;
    setError("");
    if (preview) {
      setSnapshot((current) => current ? {
        ...current,
        history: [...current.history,
          { id: `preview-${Date.now()}`, role: "user", text, createdAt: Date.now() },
          { id: `preview-${Date.now()}-reply`, role: "assistant", text: "这是 UI 预览回复；真实回复由本机 Codex 返回。", createdAt: Date.now() }],
      } : current);
      setDraft("");
      setAttachments([]);
      setPlugins([]);
      setCommandMenu(false);
      return;
    }
    if (text.startsWith("/") || text.startsWith("$")) {
      const invocation = /^[$/]\S+/.exec(text)?.[0];
      if (!invocation || !commands.some((command) => commandInvocation(command.name) === invocation)) {
        setError("当前会话不支持该命令或技能；没有发送给 Codex。");
        return;
      }
    }
    sendInFlight.current = true;
    setSending(true);
    try {
      const before = revision.current;
      await invoke("send_codex_chat_message", { sessionId: session.id, message: text, attachments, plugins: plugins.map(plugin => plugin.id) });
      if (revision.current === before) setSnapshot((current) => current ? { ...current, busy: true } : current);
      setDraft(current => current === draft ? "" : current);
      setAttachments(current => current.filter(path => !attachments.includes(path)));
      setPlugins(current => current.filter(plugin => !plugins.some(submitted => submitted.id === plugin.id)));
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
      const selected = await open({ multiple: true, directory: false, title: "添加 Codex 文件" });
      if (!selected) return;
      const paths = typeof selected === "string" ? [selected] : selected;
      setAttachments((current) => [...new Set([...current, ...paths])]);
    } catch (reason) { setError(String(reason)); }
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
      await invoke("answer_codex_chat_permission", {
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
      await invoke("cancel_codex_chat_turn", { sessionId: session.id });
    } catch (reason) {
      setError(String(reason));
    }
  };

  const changeConfig = async (value: string, configId = "model") => {
    if (configId === "model") setSelectedModel(value);
    if (preview) {
      setSnapshot((current) => current ? { ...current, configOptions: current.configOptions.map((raw) => (raw as ConfigOption).id === configId ? { ...(raw as ConfigOption), currentValue: value } : raw) } : current);
      return;
    }
    setSavingModel(true);
    setError("");
    try {
      const next = await invoke<HarnessSessionSnapshot>("set_codex_chat_config", { sessionId: session.id, value, configId });
      applySnapshot(next);
    } catch (reason) {
      setError(String(reason));
      if (snapshot) setSelectedModel(currentModel(snapshot.configOptions));
    } finally {
      setSavingModel(false);
    }
  };

  const controlGoal = async (action: "set" | "pause" | "resume" | "clear", objective?: string) => {
    setError("");
    if (preview) {
      setSnapshot((current) => current ? { ...current, codexGoal: action === "clear" ? null : action === "set" ? { objective: objective!, status: "active", tokensUsed: 0, timeUsedSeconds: 0, createdAt: Date.now(), updatedAt: Date.now() } : current.codexGoal ? { ...current.codexGoal, status: action === "pause" ? "paused" : "active" } : null } : current);
      return;
    }
    try { await invoke("control_codex_chat_goal", { sessionId: session.id, action, objective: objective ?? null }); }
    catch (reason) { setError(String(reason)); throw reason; }
  };

  return (
    <section className={`harness-chat native-agent-chat${empty ? " harness-chat-empty" : ""}`} aria-label="Codex Chat 会话">
      <header className="harness-chat-header">
        <div className="harness-chat-controls"><span className="harness-workspace" title={session.cwd}><FolderSimple size={17} />工作目录：{session.cwd.split(/[\\/]/).filter(Boolean).pop() || session.cwd}</span><span className={`harness-connection${snapshot?.connected ? " connected" : ""}`}><i />{connecting ? "连接中" : snapshot?.connected ? "已连接" : "未连接"}</span>
          {snapshot?.connected && !preview && <button type="button" onClick={() => void stop()}>停止会话</button>}
          {!snapshot?.connected && !connecting && <button type="button" onClick={() => void connect()}>恢复会话</button>}
        </div>
      </header>
      {preview && <div className="harness-chat-preview-banner">UI 预览：不会连接真实 Agent、调用模型或执行工具。可交互内容为演示数据。</div>}
      <NativeAgentSelector kind="codex_chat" {...switchProps} />

      <div className="harness-chat-messages" aria-live="polite">
        {empty && <NativeAgentWelcome kind="codex_chat" active={active} skillsDisabled={!snapshot?.connected} onSkills={() => { setBrowseAllCommands(true); setCommandMenu(true); composerInput.current?.focus(); }} />}

        {messages.map((message) => <article className={`harness-message ${message.role}`} key={`${message.role}-${message.id}`}>
          <strong>{message.role === "user" ? "你" : "Codex"}</strong>
          {message.reasoning && <details className="harness-reasoning" open={message.streamStatus === "streaming"}><summary>{message.streamStatus === "streaming" ? "正在思考" : "思考过程"}</summary><p>{message.reasoning}</p></details>}
          {(message.text || !message.reasoning) && <p>{message.text || "…"}</p>}
          {message.streamStatus && message.streamStatus !== "completed" && <small className="harness-stream-status">{({ streaming: "生成中…", interrupted: "已取消，显示部分内容", failed: "本次尝试失败", abandoned: "本次尝试未完成" } as Record<string, string>)[message.streamStatus]}</small>}
          {(message.attachments ?? []).map((file) => <div className="harness-attachment" key={file.uri} title={file.path}><Paperclip size={14} /> {file.name}</div>)}
          {(message.role === "assistant" || !replies.has(message.id)) && turnChanges.has(message.id) && (
            <ChatTurnChangesCard changes={turnChanges.get(message.id)!} onQuote={(quote) => {
              setDraft(current => current ? `${current.trimEnd()}\n\n${quote}` : quote);
              setCommandMenu(false);
              requestAnimationFrame(() => composerInput.current?.focus());
            }} />
          )}
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
      <CodexGoalPanel snapshot={snapshot} open={goalOpen} onClose={() => { setGoalOpen(false); composerInput.current?.focus(); }} onEdit={() => setGoalOpen(true)} onControl={controlGoal} />
      <CodexPluginPicker sessionId={session.id} preview={preview} open={pluginOpen} selected={plugins} onClose={() => { setPluginOpen(false); composerInput.current?.focus(); }} onSelect={plugin => setPlugins(current => current.some(item => item.id === plugin.id) ? current : [...current, plugin])} />
      <form className="harness-chat-composer" onSubmit={(event) => void send(event)}>
        {commandMenu && <div className="harness-command-menu codex-command-menu" role="menu" aria-label="Codex 命令">
          <button className="codex-menu-action" type="button" role="menuitem" onClick={() => { setCommandMenu(false); setGoalOpen(true); }}><Target size={20} /><span><strong>目标</strong><small>设置要持续追求的目标</small></span></button>
          <button className="codex-menu-action" type="button" role="menuitem" onClick={() => { setCommandMenu(false); setPluginOpen(true); }}><PuzzlePiece size={20} /><span><strong>插件</strong><small>添加已安装的 Codex 插件</small></span></button>
          <button className="codex-menu-action" type="button" role="menuitem" onClick={() => { setCommandMenu(false); void addFiles(); }}><Paperclip size={20} /><span><strong>文件</strong><small>添加本地文件引用</small></span></button>
          <div className="harness-skill-menu" aria-label="Codex 命令与技能"><strong>命令 <span className="harness-skill-key-hint">↑↓ 选择 · Tab 补全</span></strong>{matchingCommands.map((command, index) => <button type="button" role="menuitem" id={`codex-command-${session.id}-${index}`} className={index === activeCommandIndex ? "harness-skill-active" : ""} key={command.name} onMouseEnter={() => setActiveCommandIndex(index)} onClick={() => completeCommand(command.name)}><span className="harness-skill-name">{commandInvocation(command.name)}</span><small>{command.description}</small></button>)}{matchingCommands.length === 0 && <small>{commands.length ? "没有匹配命令" : "当前会话没有可用的命令或技能"}</small>}</div>
        </div>}
        {attachments.length > 0 && <div className="harness-attachments">{attachments.map((path) => <button type="button" key={path} title={path} aria-label={`移除附件 ${path.split(/[\\/]/).pop()}`} onClick={() => setAttachments((current) => current.filter((item) => item !== path))}><Paperclip size={14} /><span>{path.split(/[\\/]/).pop()}</span><X size={12} /></button>)}</div>}
        {plugins.length > 0 && <div className="codex-plugin-chips" aria-label="已选择插件">{plugins.map(plugin => <button type="button" key={plugin.id} aria-label={`移除插件 ${plugin.displayName}`} title={plugin.displayName} onClick={() => setPlugins(current => current.filter(item => item.id !== plugin.id))}><PuzzlePiece size={15} /><span>{plugin.displayName}</span><X size={12} /></button>)}</div>}
        <div className="harness-composer-input">
        <textarea ref={composerInput} aria-label="发送给 Codex" aria-controls={commandMenu ? `codex-command-${session.id}-${activeCommandIndex}` : undefined} value={draft} onChange={(event) => { setBrowseAllCommands(false); setDraft(event.target.value); setCommandMenu(/^[$/][^\s]*$/.test(event.target.value)); }} placeholder={goalActive ? "暂停或清除当前目标后可发送新任务" : "描述你想完成的任务，输入 / 或 $ 调用命令与技能…"} disabled={!snapshot?.connected || snapshot.busy || goalActive || connecting} onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.keyCode === 229) return;
          if (commandMenu && activeCommand && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setActiveCommandIndex((index) => (index + (event.key === "ArrowDown" ? 1 : -1) + matchingCommands.length) % matchingCommands.length);
              return;
            }
            if (event.key === "Tab") { event.preventDefault(); completeCommand(activeCommand.name); return; }
          }
          if (event.key === "Escape") { event.stopPropagation(); setCommandMenu(false); }
          if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); }
        }} />
        </div>
        <div className="harness-composer-toolbar"><div className="harness-composer-left">
          <button type="button" className="harness-icon-button" aria-label="命令菜单" title="命令菜单" aria-expanded={commandMenu} disabled={!snapshot?.connected} onClick={() => { setBrowseAllCommands(false); setCommandMenu((current) => !current); }}><Plus size={21} /></button>
          <button type="button" className="harness-icon-button" aria-label="添加文件" title="添加文件" disabled={!snapshot?.connected || snapshot.busy || connecting} onClick={() => void addFiles()}><Paperclip size={21} /></button>
        </div><div className="harness-composer-right">
          {otherConfigOptions.map((raw) => {
            const option = raw as ConfigOption;
            const options = modelChoices(snapshot?.configOptions ?? [], option.id!);
            return <label className="harness-picker harness-reasoning-picker" key={option.id}><span className="harness-reasoning-tooltip" role="tooltip" id={`codex-config-tooltip-${session.id}-${option.id}`}>{option.name ?? option.id}</span><Cpu size={17} /><select aria-describedby={`codex-config-tooltip-${session.id}-${option.id}`} aria-label={`Codex ${option.id}`} value={option.currentValue ?? ""} disabled={!snapshot?.connected || snapshot.busy || savingModel} onChange={(event) => void changeConfig(event.target.value, option.id!)}>{options.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label>;
          })}
          <CodexContextUsage usage={snapshot?.contextUsage} />
          {modelList.length > 0 && <label className="harness-picker harness-model-picker">
            <select aria-label="Codex 模型" value={selectedModel || currentModel(snapshot?.configOptions ?? [])} disabled={!snapshot?.connected || snapshot.busy || savingModel} onChange={(event) => void changeConfig(event.target.value)}>
              {modelList.map((choice) => <option key={choice.value} value={choice.value}>{choice.label}</option>)}
            </select>
          </label>}
          {snapshot?.busy && <button type="button" className="harness-cancel-button" onClick={() => void cancel()}>取消本轮</button>}
          <button type="submit" className="harness-send-button" aria-label="发送" title="发送" disabled={sending || (!draft.trim() && attachments.length === 0 && plugins.length === 0) || !snapshot?.connected || snapshot.busy || goalActive || connecting}><ArrowUp size={24} /></button>
        </div></div>
        <div className="harness-composer-hint">{snapshot?.busy ? "正在执行" : snapshot?.connected ? "Enter 发送 · Shift+Enter 换行" : "恢复会话后开始任务"}</div>
      </form>
    </section>
  );
}

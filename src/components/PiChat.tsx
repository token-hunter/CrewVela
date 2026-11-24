import { useTranslation } from "../useInterfaceLanguage";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowUp, Cpu, FolderSimple, Paperclip, Plus, X } from "@phosphor-icons/react";
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
  sessionId: "preview-pi-session",
  connected: true,
  busy: false,
  history: [
    { id: "preview-user-1", role: "user", text: "看看这个仓库的入口在哪里。", createdAt: 1 },
    { id: "preview-assistant-1", role: "assistant", streamStatus: "streaming", text: "我先浏览仓库结构和入口文件。当前是 CrewVela UI 预览，没有连接 pi CLI。", createdAt: 2 },
  ],
  pendingPermission: { requestId: "preview-permission", toolCall: { title: "bash 工具调用", kind: "bash", rawInput: { command: "npm test" } }, options: [{ optionId: "allow_once", name: "允许一次", kind: "allow_once" }, { optionId: "reject_once", name: "拒绝", kind: "reject_once" }] },
  toolCalls: [{ toolCallId: "preview-read", title: "read", status: "in_progress", rawInput: { path: "package.json" } }],
  sequence: 0,
  controlState: {},
  configOptions: [
    { id: "model", type: "select", currentValue: "faux/test-model", options: [{ name: "Pi Test Model", value: "faux/test-model" }, { name: "Pi Alt", value: "faux/alt-model" }] },
    { id: "thinking", type: "select", currentValue: "off", options: [{ name: "off", value: "off" }, { name: "low", value: "low" }, { name: "high", value: "high" }] },
  ],
  commands: [
    { name: "init", description: "初始化项目说明", inputHint: "" },
    { name: "custom", description: "自定义命令", inputHint: "[x]" },
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

export function PiChat({ session, preview = false, onSessionRecord, active = true, ...switchProps }: Props) {
  const t = useTranslation();
  const [snapshot, setSnapshot] = useState<HarnessSessionSnapshot | null>(preview ? new URLSearchParams(location.search).get("chat") === "empty" ? { ...previewSnapshot, history: [], toolCalls: [], pendingPermission: null } : previewSnapshot : null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const sendInFlight = useRef(false);
  const [attachments, setAttachments] = useState<string[]>([]);
  const [commandMenu, setCommandMenu] = useState(false);
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
  const empty = messages.length === 0 && toolCalls.length === 0 && !permission;
  const commands = snapshot?.commands ?? [];
  const matchingCommands = commands.filter((command) => `${command.name} ${command.description ?? ""}`.toLowerCase().includes(!browseAllCommands && draft.startsWith("/") ? draft.slice(1).toLowerCase() : ""));
  const activeCommand = matchingCommands[Math.min(activeCommandIndex, Math.max(0, matchingCommands.length - 1))];
  // 命令词后跟空格（Tab 补全会补上）即视为命令已确立：输入框内联渲染 ✨ 胶囊，textarea 只编辑参数部分。
  const invokedCommand = commands.find((command) => draft.startsWith(`/${command.name} `));
  const draftRemainder = invokedCommand ? draft.slice(invokedCommand.name.length + 2) : draft;
  const modelList = useMemo(() => modelChoices(snapshot?.configOptions ?? []), [snapshot?.configOptions]);
  const otherConfigOptions = (snapshot?.configOptions ?? []).filter((option) => {
    const id = (option as ConfigOption)?.id;
    return id && id !== "model" && modelChoices(snapshot?.configOptions ?? [], id).length > 0;
  });

  useEffect(() => { setActiveCommandIndex(0); }, [draft, browseAllCommands, commandMenu]);
  useEffect(() => {
    if (active && commandMenu) document.getElementById(`pi-command-${session.id}-${activeCommandIndex}`)?.scrollIntoView({ block: "nearest" });
  }, [active, activeCommandIndex, commandMenu, session.id]);

  const completeCommand = (name: string) => {
    const remainder = draft.startsWith("/") ? draft.replace(/^\/[^\s]*\s*/, "") : draft;
    setDraft(`/${name} ${remainder}`.trimEnd() + " ");
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
    if (event.snapshot) {
      applySnapshot(event.snapshot);
      if (event.kind === "error") setError(String(event.data.message ?? "Pi Chat 执行失败"));
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
        setError(String(event.data.message ?? "Pi Chat 执行失败"));
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
      const next = await invoke<HarnessSessionSnapshot>("start_pi_chat_session", { sessionId: session.id });
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
      unlistenChat = await listen<HarnessEvent>("pi-chat-update", ({ payload }) => { if (!disposed) applyEvent(payload); });
      if (disposed) { unlistenChat(); return; }
      unlistenCloseError = await listen<string>("pi-chat-error", ({ payload }) => { if (!disposed) setError(payload); });
      if (disposed) {
        unlistenChat();
        unlistenCloseError();
        return;
      }
      try {
        const initial = await invoke<HarnessSessionSnapshot>("pi_chat_snapshot", { sessionId: session.id });
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
    if ((!text && attachments.length === 0) || snapshot?.busy || !snapshot?.connected) return;
    setError("");
    if (preview) {
      setSnapshot((current) => current ? {
        ...current,
        history: [...current.history,
          { id: `preview-${Date.now()}`, role: "user", text, createdAt: Date.now() },
          { id: `preview-${Date.now()}-reply`, role: "assistant", text: "这是 UI 预览回复；真实回复由本机 Pi 返回。", createdAt: Date.now() }],
      } : current);
      setDraft("");
      setAttachments([]);
      setCommandMenu(false);
      return;
    }
    if (text.startsWith("/") && !commands.some((command) => command.name === text.slice(1).split(/\s/)[0])) {
      setError("当前会话不支持该斜杠命令；没有发送给 pi。");
      return;
    }
    sendInFlight.current = true;
    setSending(true);
    try {
      const before = revision.current;
      await invoke("send_pi_chat_message", { sessionId: session.id, message: text, attachments });
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
      const selected = await open({ multiple: true, directory: false, title: "添加 Pi Chat 文件" });
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
      await invoke("answer_pi_chat_permission", {
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
      await invoke("cancel_pi_chat_turn", { sessionId: session.id });
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
      const next = await invoke<HarnessSessionSnapshot>("set_pi_chat_config", { sessionId: session.id, value, configId });
      applySnapshot(next);
    } catch (reason) {
      setError(String(reason));
      if (snapshot) setSelectedModel(currentModel(snapshot.configOptions));
    } finally {
      setSavingModel(false);
    }
  };

  return (
    <section className={`harness-chat native-agent-chat${empty ? " harness-chat-empty" : ""}`} aria-label={t("Pi Chat 会话")}>
      <header className="harness-chat-header">
        <div className="harness-chat-controls"><span className="harness-workspace" title={session.cwd}><FolderSimple size={17} />{t("工作目录：")}{session.cwd.split(/[\\/]/).filter(Boolean).pop() || session.cwd}</span><span className={`harness-connection${snapshot?.connected ? " connected" : ""}`}><i />{connecting ? t("连接中") : snapshot?.connected ? t("已连接") : t("未连接")}</span>
          {snapshot?.connected && !preview && <button type="button" onClick={() => void stop()}>{t("停止会话")}</button>}
          {!snapshot?.connected && !connecting && <button type="button" onClick={() => void connect()}>{t("恢复会话")}</button>}
        </div>
      </header>
      {preview && <div className="harness-chat-preview-banner">UI 预览：不会连接真实 Agent、调用模型或执行工具。可交互内容为演示数据。</div>}
      <NativeAgentSelector kind="pi_chat" {...switchProps} />

      <div className="harness-chat-messages" aria-live="polite">
        {empty && <NativeAgentWelcome kind="pi_chat" active={active} skillsDisabled={!snapshot?.connected} onSkills={() => { setBrowseAllCommands(true); setCommandMenu(true); composerInput.current?.focus(); }} />}

        {messages.map((message) => <article className={`harness-message ${message.role}`} key={`${message.role}-${message.id}`}>
          <strong>{message.role === "user" ? "你" : "Pi"}</strong>
          {message.reasoning && <details className="harness-reasoning" open={message.streamStatus === "streaming"}><summary>{message.streamStatus === "streaming" ? "正在思考" : t("思考过程")}</summary><p>{message.reasoning}</p></details>}
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
          <strong>{t("需要确认：")}{permission.toolCall.title ?? "工具调用"}</strong>
          {permission.toolCall.rawInput !== undefined && <pre>{typeof permission.toolCall.rawInput === "string" ? permission.toolCall.rawInput : JSON.stringify(permission.toolCall.rawInput, null, 2)}</pre>}
          <div><button type="button" onClick={() => void answerPermission(true)}>{t("允许一次")}</button><button type="button" onClick={() => void answerPermission(false)}>{t("拒绝")}</button></div>
        </article>}
        {!connecting && !snapshot?.connected && <div className="harness-chat-info">{t("会话已停止或未连接。点击“恢复会话”继续原 pi 会话。")}</div>}
        <div ref={messagesEnd} />
      </div>
      {error && <pre className="harness-chat-error" role="alert">{error}</pre>}
      <form className="harness-chat-composer" onSubmit={(event) => void send(event)}>
        {commandMenu && <div className="harness-command-menu" role="menu" aria-label={t("Pi 命令")}>
          <button type="button" role="menuitem" onClick={() => { setCommandMenu(false); void addFiles(); }}>{t("文件 file")} <small>{t("添加本地文件引用")}</small></button>
          <div className="harness-skill-menu" aria-label={t("Pi 斜杠命令")}><strong>{t("命令")} <span className="harness-skill-key-hint">{t("↑↓ 选择 · Tab 补全")}</span></strong>{matchingCommands.map((command, index) => <button type="button" role="menuitem" id={`pi-command-${session.id}-${index}`} className={index === activeCommandIndex ? "harness-skill-active" : ""} key={command.name} onMouseEnter={() => setActiveCommandIndex(index)} onClick={() => completeCommand(command.name)}><span className="harness-skill-name">/{command.name}</span><small>{command.description}</small></button>)}{matchingCommands.length === 0 && <small>{commands.length ? t("没有匹配命令") : t("当前会话没有可用的斜杠命令")}</small>}</div>
        </div>}
        {attachments.length > 0 && <div className="harness-attachments">{attachments.map((path) => <button type="button" key={path} title={path} aria-label={`移除附件 ${path.split(/[\\/]/).pop()}`} onClick={() => setAttachments((current) => current.filter((item) => item !== path))}><Paperclip size={14} /><span>{path.split(/[\\/]/).pop()}</span><X size={12} /></button>)}</div>}
        <div className={invokedCommand ? "harness-composer-input harness-composer-input-command" : "harness-composer-input"}>
        {invokedCommand && <span className="harness-input-command-chip" title={invokedCommand.description || invokedCommand.name} aria-label={t("已识别命令")}><span aria-hidden="true">✨</span>/{invokedCommand.name}</span>}
        <textarea ref={composerInput} aria-label={t("发送给 Pi")} aria-controls={commandMenu ? `pi-command-${session.id}-${activeCommandIndex}` : undefined} value={invokedCommand ? draftRemainder : draft} onChange={(event) => { const value = event.target.value; setBrowseAllCommands(false); setDraft(invokedCommand ? `/${invokedCommand.name} ${value}` : value); setCommandMenu(!invokedCommand && /^\/[^\s]*$/.test(value)); }} placeholder={invokedCommand ? "" : t("描述你想完成的任务，输入 / 调用命令…")} disabled={!snapshot?.connected || snapshot.busy || connecting} onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.keyCode === 229) return;
          if (commandMenu && activeCommand && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setActiveCommandIndex((index) => (index + (event.key === "ArrowDown" ? 1 : -1) + matchingCommands.length) % matchingCommands.length);
              return;
            }
            if (event.key === "Tab") { event.preventDefault(); completeCommand(activeCommand.name); return; }
          }
          if (event.key === "Backspace" && invokedCommand && !draftRemainder) { event.preventDefault(); setDraft(""); return; }
          if (event.key === "Escape") { event.stopPropagation(); setCommandMenu(false); }
          if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); }
        }} />
        </div>
        <div className="harness-composer-toolbar"><div className="harness-composer-left">
          <button type="button" className="harness-icon-button" aria-label={t("命令菜单")} title={t("命令菜单")} aria-expanded={commandMenu} disabled={!snapshot?.connected} onClick={() => { setBrowseAllCommands(false); setCommandMenu((current) => !current); }}><Plus size={21} /></button>
          <button type="button" className="harness-icon-button" aria-label={t("添加文件")} title={t("添加文件")} disabled={!snapshot?.connected || snapshot.busy || connecting} onClick={() => void addFiles()}><Paperclip size={21} /></button>
        </div><div className="harness-composer-right">
          {otherConfigOptions.map((raw) => {
            const option = raw as ConfigOption;
            const options = modelChoices(snapshot?.configOptions ?? [], option.id!);
            return <label className="harness-picker harness-reasoning-picker" key={option.id}><span className="harness-reasoning-tooltip" role="tooltip" id={`pi-config-tooltip-${session.id}-${option.id}`}>{option.name ?? option.id}</span><Cpu size={17} /><select aria-describedby={`pi-config-tooltip-${session.id}-${option.id}`} aria-label={`Pi ${option.id}`} value={option.currentValue ?? ""} disabled={!snapshot?.connected || snapshot.busy || savingModel} onChange={(event) => void changeConfig(event.target.value, option.id!)}>{options.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select></label>;
          })}
          {modelList.length > 0 && <label className="harness-picker harness-model-picker">
            <select aria-label={t("Pi 模型")} value={selectedModel || currentModel(snapshot?.configOptions ?? [])} disabled={!snapshot?.connected || snapshot.busy || savingModel} onChange={(event) => void changeConfig(event.target.value)}>
              {modelList.map((choice) => <option key={choice.value} value={choice.value}>{choice.label}</option>)}
            </select>
          </label>}
          {snapshot?.busy && <button type="button" className="harness-cancel-button" onClick={() => void cancel()}>{t("取消本轮")}</button>}
          <button type="submit" className="harness-send-button" aria-label={t("发送")} title={t("发送")} disabled={sending || (!draft.trim() && attachments.length === 0) || !snapshot?.connected || snapshot.busy || connecting}><ArrowUp size={24} /></button>
        </div></div>
        <div className="harness-composer-hint">{snapshot?.busy ? t("正在执行") : snapshot?.connected ? "Enter 发送 · Shift+Enter 换行" : t("恢复会话后开始任务")}</div>
      </form>
    </section>
  );
}

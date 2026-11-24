import { useTranslation } from "../useInterfaceLanguage";
import "../styles/zcode-message-actions.css";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ArrowUp, FolderSimple, Plus, Paperclip, X, Shield, Check, CaretDown, Gauge, CircleNotch, Copy, PencilSimple, Square, ArrowsLeftRight } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import { appSideCommands, composerItems, composeMessage, normalizeCatalog, parseVisibleSlashCommand, rejectedSubmissionDisposition, skillSlashCommands, type ComposerCatalog, type ComposerItem } from "./zcodeComposer";
import { classifyComposerPaste, mergeAttachmentRefs } from "./zcodePaste";
import { pairedSubagent, type SubagentRow } from "./zcodeSubagents";
import type { SessionRecord } from "../types";
import { NativeAgentSelector, NativeAgentWelcome } from "./NativeAgentWelcome";
import type { NativeAgentSwitchProps } from "../nativeAgentSwitch";

import { addConversationQuote, canEditUserMessage, withConversationQuotes, type ConversationQuote } from "./zcodeMessageActions";

type Attachment = { ref: string; fileName: string; mime: string; bytes: number };
type ToolDisplay = { kind?: string; description?: string; serverName?: string; toolName?: string } | null;
type Row = SubagentRow & { rowId: number; entityId?: string; actions?: { canEdit?: boolean; editDisposition?: string }; kind: string; text?: string; attachments?: Attachment[]; state?: string; status?: string; toolName?: string; inputText?: string; sourceCommandId?: string; title?: string; durationMs?: number; display?: ToolDisplay; input?: unknown; output?: { text?: string } | null; outputPreview?: { text?: string } | null; progress?: { previewLine?: string } | null };
type Permission = { requestId: string; toolName?: string; input?: unknown };
type ModelOption = { id: string; name: string };
type QueueItem = { queueItemId: string; sourceCommandId: string; text: string; attachments?: Attachment[]; dispatch: { state: string } };
type PermissionMode = "plan" | "build" | "edit" | "yolo";
const permissionModes: { id: PermissionMode; label: string; description: string }[] = [
  { id: "plan", label: "计划模式", description: "先分析和规划任务。" },
  { id: "build", label: "变更前确认", description: "修改文件、执行命令前确认。" },
  { id: "edit", label: "自动编辑", description: "自动编辑文件，其他操作按原生规则确认。" },
  { id: "yolo", label: "完全访问", description: "授予 Agent 完全访问权限，不再确认。" },
];
type ChildDetail = { childSessionId: string; rows: Row[]; loading: boolean; error: string | null };
type ContextUsage = { used: number; size: number; cache?: { latestHitRate?: number | null; hitRate?: number | null } | null; breakdown?: { source: string; chars: number }[] | null };
type Snapshot = { childDetail?: ChildDetail | null; sideDetail?: ChildDetail | null; sessionId: string; connected: boolean; busy: boolean; rows: Row[]; pendingPermission: Permission | null; sequence: number; error: string | null; models: ModelOption[]; selectedModelId: string | null; permissionMode: PermissionMode | null; queuedInputs: QueueItem[]; recoverableInputs: QueueItem[]; queueAutoDrain: boolean; canQueue: boolean; inputRouting: string | null; contextUsage?: ContextUsage | null };
/** 上下文来源分解：排序与中文标签对齐 ZCode 原生面板。 */
const usageSourceMeta: { source: string; label: string; order: number }[] = [
  { source: "messages", label: "消息", order: 0 },
  { source: "system_prompt", label: "系统提示词", order: 1 },
  { source: "meta_user_context", label: "其他", order: 2 },
  { source: "skills", label: "技能", order: 3 },
  { source: "tool_prompt", label: "工具提示词", order: 4 },
  { source: "system_tool_schemas", label: "系统工具", order: 5 },
  { source: "mcp_tool_schemas", label: "MCP 工具", order: 6 },
];
/** 压缩 token 数（如 39.2万），与 ZCode 的紧凑格式观感一致。 */
function formatCompactTokens(value: number) {
  if (value >= 100_000_000) return `${(value / 100_000_000).toFixed(1)}亿`;
  if (value >= 10_000) return `${(value / 10_000).toFixed(1)}万`;
  return String(value);
}
const previewSubagents = new URLSearchParams(location.search).get("zcode-subagents") === "1";
const subagentPreviewRows: Row[] = [{ rowId: 1, kind: "toolCall", turnId: "t", toolCallId: "a", toolName: "Agent", status: "running" }, { rowId: 2, kind: "subagent", turnId: "t", parentToolCallId: "a", childSessionId: "preview-child", subagentType: "Explore", summaryText: "搜索 CLI/TUI 实现", status: "running" }, { rowId: 4, kind: "subagent", turnId: "t", parentToolCallId: "b", childSessionId: "preview-child-b", subagentType: "general-purpose", summaryText: "核对协议订阅", status: "running" }, { rowId: 3, kind: "subagent", turnId: "t", subagentType: "general-purpose", summaryText: "整理结果", status: "success" }];
const previewMessages = new URLSearchParams(location.search).get("zcode-messages") === "1";
const messagePreviewRows: Row[] = [{ rowId: 1, entityId: "preview-user", actions: { canEdit: true, editDisposition: "rewind" }, kind: "userInput", text: "请介绍 3D 生成与 3D 感知的区别。" }, { rowId: 2, kind: "assistantText", text: "混元3D 2.1：直接输出 3D 资产。\nDepth Anything 3：输出深度图和相机位姿。\nMoGe：输出 3D 点图。", state: "complete" }];
const previewStreaming = new URLSearchParams(location.search).get("zcode-output") === "streaming";
const previewRecovery = new URLSearchParams(location.search).get("zcode-queue") === "recoverable";
const previewModeParam = new URLSearchParams(location.search).get("zcode-mode");
const previewMode: PermissionMode = ["plan", "build", "edit", "yolo"].includes(previewModeParam ?? "") ? previewModeParam as PermissionMode : "build";
const previewModels: ModelOption[] = [{ id: "preview/personal", name: "Personal API · 演示模型" }];
const previewCatalog: ComposerCatalog = { skills: { items: [{ id: "preview-skill", name: "示例技能", description: "模拟技能目录，用于界面验收", path: "F:\\preview\\SKILL.md", scope: "workspace", enabled: true }] }, plugins: { items: [{ pluginId: "preview@fixture", name: "示例插件", description: "模拟插件目录，不连接模型", enabled: true, conflictingPluginIds: [] }] }, commands: { items: [{ name: "workflow", description: "创建工作流（模拟目录）" }] } };
type Update = { snapshot: Snapshot; record: SessionRecord | null };
function messageContent(text: string) {
  return text.split(/(\[(?:\\.|[^\]])*\]\((?:<[^>]*>|[^)]*)\))/g).map((part, index) => {
    const match = /^\[((?:\\.|[^\]])*)\]\((?:<[^>]*>|[^)]*)\)$/.exec(part);
    if (!match || !/^[$@]/.test(match[1])) return part;
    return <mark className="zcode-reference" key={index}>{match[1].startsWith("$") ? "✨ " : "🔌 "}{match[1].replace(/\\(.)/g, "$1")}</mark>;
  });
}
function toolSummary(input?: string) {
  if (!input) return "查看调用详情";
  try {
    const value: unknown = JSON.parse(input);
    if (value && typeof value === "object") {
      const fields = value as Record<string, unknown>;
      for (const key of ["description", "file_path", "path", "command", "query", "url"]) {
        if (typeof fields[key] === "string" && fields[key]) return fields[key] as string;
      }
      return "查看调用参数";
    }
  } catch { /* 部分工具参数为原始文本。 */ }
  return input;
}
function toolDetails(input: string) {
  try { return JSON.stringify(JSON.parse(input), null, 2); } catch { return input; }
}
function parseJsonObject(input?: string): Record<string, unknown> | null {
  if (!input) return null;
  try {
    const value: unknown = JSON.parse(input);
    return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
  } catch { return null; }
}
/** 待办步骤提取：TodoWrite/TODO 类工具的 input/output 里是 JSON 步骤数组。 */
function extractTodoSteps(row: Row): { content: string; status: string }[] | null {
  const candidates: unknown[] = [row.output?.text, (parseJsonObject(row.inputText) as { todos?: unknown; steps?: unknown } | null)?.todos, (parseJsonObject(row.inputText) as { todos?: unknown; steps?: unknown } | null)?.steps, parseJsonObject(row.inputText)?.plan];
  for (const candidate of candidates) {
    if (typeof candidate !== "string" && !Array.isArray(candidate)) continue;
    let value: unknown = candidate;
    if (typeof candidate === "string") {
      try { value = JSON.parse(candidate); } catch { continue; }
    }
    if (!Array.isArray(value)) continue;
    const steps = value.map(item => {
      const record = item as Record<string, unknown>;
      const content = typeof record?.content === "string" ? record.content : typeof record?.text === "string" ? record.text : null;
      const status = typeof record?.status === "string" ? record.status : "pending";
      return content ? { content, status } : null;
    }).filter(Boolean) as { content: string; status: string }[];
    if (steps.length > 0) return steps;
  }
  return null;
}
const toolStatusLabels: Record<string, string> = { success: "已完成", failed: "失败", error: "失败", cancelled: "已取消", pendingApproval: "等待确认", running: "执行中", inProgress: "执行中", pending: "等待执行" };
export function ZCodeChat({ session, preview, onSessionRecord, active = true, ...switchProps }: NativeAgentSwitchProps & { session: SessionRecord; preview?: boolean; onSessionRecord: (record: SessionRecord) => void }) {
  const t = useTranslation();
  const [snapshot, setSnapshot] = useState<Snapshot | null>(preview ? { sessionId: session.id, connected: true, busy: previewStreaming, rows: previewMessages ? messagePreviewRows : previewSubagents ? subagentPreviewRows : previewStreaming ? [{ rowId: 1, kind: "reasoning", text: "模拟持续输出，可以准备和排队下一条消息。", state: "streaming" }] : [], pendingPermission: null, sequence: 0, error: null, models: new URLSearchParams(location.search).get("zcode-models") === "empty" ? [] : previewModels, selectedModelId: previewStreaming || previewMessages ? previewModels[0].id : null, permissionMode: previewMode, queuedInputs: [], recoverableInputs: previewRecovery ? [{ queueItemId: "preview-recovery", sourceCommandId: "preview-recovery-command", text: "模拟待核对消息", dispatch: { state: "queued" } }] : [], queueAutoDrain: true, canQueue: true, inputRouting: previewStreaming ? "enqueue" : "startNow" } : null);
  const [childId, setChildId] = useState<string | null>(null);
  const [childError, setChildError] = useState("");
  const [childRetry, setChildRetry] = useState(0);
  const [childLoading, setChildLoading] = useState(false);
  const childGeneration = useRef(0);
  const childActions = useRef<Promise<unknown>>(Promise.resolve());
  useEffect(() => {
    if (!active) setChildId(null);
  }, [active]);
  useEffect(() => {
    const generation = ++childGeneration.current;
    if (!childId || !active) return;
    const requestToken = crypto.randomUUID();
    setChildError(""); setChildLoading(true);
    if (preview) { setChildLoading(false); return; }
    childActions.current = childActions.current.then(async () => {
      if (generation !== childGeneration.current) return;
      await invoke("zcode_child_action", { sessionId: session.id, childSessionId: childId, requestToken, open: true });
    }).catch(e => { if (generation === childGeneration.current) setChildError(String(e)); }).finally(() => { if (generation === childGeneration.current) setChildLoading(false); });
    return () => {
      ++childGeneration.current;
      // Serialize cleanup behind its open, and ahead of any replacement. Tauri
      // spawn_blocking order is not invocation order; an old open must not win.
      childActions.current = childActions.current.then(() => invoke("zcode_child_action", { sessionId: session.id, childSessionId: childId, requestToken, open: false })).catch(e => { if (mounted.current) setChildError(String(e)); });
    };
  }, [childId, childRetry, active, preview, session.id]);
  const subagentCard = (row: Row) => <button type="button" className="zcode-subagent" key={row.rowId} disabled={!row.childSessionId || !snapshot?.connected} onClick={() => setChildId(row.childSessionId || null)}><strong>子智能体 · {row.subagentType || "Agent"}</strong><span>{toolStatusLabels[row.status || ""] || row.status}</span><span>{row.summaryText}</span><small>{row.childSessionId ? "查看详情 →" : "详情暂不可用"}</small></button>;
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [sideDraft, setSideDraft] = useState("");
  const [quotes, setQuotes] = useState<ConversationQuote[]>([]);
  const [sideQuotes, setSideQuotes] = useState<ConversationQuote[]>([]);
  const [selection, setSelection] = useState<{ text: string; x: number; y: number } | null>(null);
  const [editing, setEditing] = useState<Row | null>(null);
  const [editText, setEditText] = useState("");
  const [editWorking, setEditWorking] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const cancelInFlight = useRef(false);
  const sideInput = useRef<HTMLTextAreaElement>(null);
  const [sideWorking, setSideWorking] = useState(false);
  const [sideError, setSideError] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [sending, setSending] = useState(false);
  const sendInFlight = useRef(false);
  const [answering, setAnswering] = useState(false);
  const [modeMenu, setModeMenu] = useState(false);
  const [selectingMode, setSelectingMode] = useState(false);
  const modePicker = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!modeMenu) return;
    const close = (event: PointerEvent) => { if (!modePicker.current?.contains(event.target as Node)) setModeMenu(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") setModeMenu(false); };
    document.addEventListener("pointerdown", close); document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", close); document.removeEventListener("keydown", escape); };
  }, [modeMenu]);
  const [usageOpen, setUsageOpen] = useState(false);
  const usagePicker = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!usageOpen) return;
    const close = (event: PointerEvent) => { if (!usagePicker.current?.contains(event.target as Node)) setUsageOpen(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") setUsageOpen(false); };
    document.addEventListener("pointerdown", close); document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", close); document.removeEventListener("keydown", escape); };
  }, [usageOpen]);
  const [selectingModel, setSelectingModel] = useState(false);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [addingAttachments, setAddingAttachments] = useState(false);
  // 粘贴/拖放附件在途标记：避免连击导致重复落盘/重复 invoke。
  const ingesting = useRef(false);
  const [dropActive, setDropActive] = useState(false);
  const [references, setReferences] = useState<ComposerItem[]>([]);
  const [catalog, setCatalog] = useState<ComposerCatalog | null>(preview ? previewCatalog : null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState("");
  const [menu, setMenu] = useState<"all" | "skills" | "plugins" | "commands" | null>(null);
  const [activeIndex, setActiveIndex] = useState(0);
  const [caret, setCaret] = useState(0);
  const input = useRef<HTMLTextAreaElement>(null);
  const [failedSubmission, setFailedSubmission] = useState<{ text: string; attachments: Attachment[]; references: ComposerItem[]; quotes: ConversationQuote[]; uncertain?: boolean } | null>(null);
  const [queueWorking, setQueueWorking] = useState(false);
  const currentComposer = useRef({ text: draft, attachments, references, quotes });
  currentComposer.current = { text: draft, attachments, references, quotes };
  const sequence = useRef(-1);
  const mounted = useRef(true);
  const end = useRef<HTMLDivElement>(null);
  const apply = useCallback((value: Snapshot) => {
    if (!mounted.current || value.sessionId !== session.id || value.sequence < sequence.current) return;
    sequence.current = value.sequence; setSnapshot(value);
  }, [session.id]);
  useEffect(() => { if (snapshot && !snapshot.connected) setChildId(null); }, [snapshot?.connected]);
  const connect = useCallback(async () => {
    if (preview) { setSnapshot(value => value && { ...value, connected: true, busy: false, inputRouting: "startNow", queueAutoDrain: true, recoverableInputs: [...value.recoverableInputs, ...value.queuedInputs], queuedInputs: [] }); return; }
    setConnecting(true); setError("");
    try { apply(await invoke<Snapshot>("start_zcode_session", { sessionId: session.id })); }
    catch (e) { if (mounted.current) setError(String(e)); }
    finally { if (mounted.current) setConnecting(false); }
  }, [apply, preview, session.id]);
  const reloadModels = async () => {
    if (connecting || sending || selectingModel || selectingMode || snapshot?.busy) return;
    setConnecting(true); setError("");
    try {
      if (preview) {
        setSnapshot(value => value && { ...value, sequence: value.sequence + 1, connected: true, models: previewModels });
      } else {
        onSessionRecord(await invoke<SessionRecord>("stop_session", { sessionId: session.id }));
        apply(await invoke<Snapshot>("start_zcode_session", { sessionId: session.id }));
      }
    } catch (e) { if (mounted.current) setError(String(e)); }
    finally { if (mounted.current) setConnecting(false); }
  };
  useEffect(() => {
    mounted.current = true;
    if (preview) return () => { mounted.current = false; };
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void (async () => {
      try {
        const stop = await listen<Update>("zcode-session-update", ({ payload }) => {
          if (disposed || !mounted.current || payload.snapshot.sessionId !== session.id || payload.snapshot.sequence < sequence.current) return;
          apply(payload.snapshot); if (payload.record) onSessionRecord(payload.record);
        });
        if (disposed || !mounted.current) { stop(); return; } unlisten = stop;
        const initial = await invoke<Snapshot>("zcode_snapshot", { sessionId: session.id });
        if (disposed) return;
        apply(initial);
        if (mounted.current && session.status === "pending" && !initial.connected) await connect();
      } catch (e) { if (!disposed && mounted.current) setError(String(e)); }
    })();
    return () => { disposed = true; mounted.current = false; unlisten?.(); };
  }, [apply, connect, onSessionRecord, preview, session.id]);
  useEffect(() => { if (active) end.current?.scrollIntoView({ block: "end", behavior: "smooth" }); }, [active, snapshot?.sequence, snapshot?.rows]);
  const loadCatalog = useCallback(async () => {
    if (preview) { setCatalog(previewCatalog); return; }
    setCatalogLoading(true); setCatalogError("");
    try { const value = normalizeCatalog(await invoke("zcode_composer_catalog", { sessionId: session.id })); if (mounted.current) setCatalog(value); }
    catch (e) { if (mounted.current) { setCatalog(null); setCatalogError(String(e)); } }
    finally { if (mounted.current) setCatalogLoading(false); }
  }, [preview, session.id]);
  useEffect(() => { if (snapshot?.connected) void loadCatalog(); else setCatalog(null); }, [snapshot?.connected, loadCatalog]);
  const allItems = catalog ? composerItems(catalog) : [];
  // / 面板补齐 ZCode 原生观感：技能以 /name 形态可见（选择即引用该技能），
  // 并合成 /side、/btw 辅助会话入口（选择触发新建同目录会话，见 appSideCommands）。
  const reservedNames = new Set(allItems.filter(item => item.kind === "commands").map(item => item.name.toLowerCase()));
  const slashMenuItems = catalog
    ? [...allItems, ...skillSlashCommands(catalog, reservedNames), ...appSideCommands(reservedNames)]
    : allItems;
  const token = /(?:^|\s)([$/@])([^\s]*)$/.exec(draft.slice(0, caret));
  const query = menu ? "" : token?.[2]?.toLowerCase() ?? "";
  const tokenKind = token?.[1] === "$" ? "skills" : token?.[1] === "/" ? "commands" : token?.[1] === "@" ? "plugins" : null;
  const shown = slashMenuItems.filter(item => (menu === "all" || item.kind === (menu || tokenKind)) && (!query || `${item.name} ${item.label} ${item.description}`.toLowerCase().includes(query)));
  const menuOpen = !!menu || !!tokenKind;
  useEffect(() => { setActiveIndex(0); }, [query, menu, tokenKind]);
  const choose = (item: ComposerItem) => {
    if (item.disabled) return;
    if (item.appAction === "new-side-session") {
      // App 合成命令：不进入消息正文，在当前 ZCode Desktop 进程内派生辅助对话并打开分栏
      // （原生 selection-side-session 同语义，不新开会话/窗口）。
      const start = token ? caret - token[1].length - token[2].length : caret;
      setDraft(value => token ? value.slice(0, start) + value.slice(caret) : value);
      setMenu(null);
      void openSideConversation();
      return;
    }
    const start = token ? caret - token[1].length - token[2].length : caret;
    setDraft(value => token ? value.slice(0, start) + value.slice(caret) : value);
    setReferences(value => item.kind === "commands" ? [item, ...value.filter(ref => ref.kind !== "commands")] : value.some(ref => ref.id === item.id && ref.kind === item.kind) ? value : [...value, item]);
    setMenu(null); setCaret(start);
    requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(start, start); });
  };
  const addAttachments = async () => {
    if (addingAttachments || sending) return;
    setMenu(null);
    if (preview) { setError("预览模式不打开本地文件；附件交互由测试模拟。"); return; }
    setAddingAttachments(true);
    try {
      const selected = await open({ multiple: true, directory: false }); if (!selected) return;
      const paths = [...attachments.map(item => item.ref), ...(Array.isArray(selected) ? selected : [selected])];
      setAttachments(await invoke<Attachment[]>("zcode_attachment_info", { sessionId: session.id, paths: [...new Set(paths)] }));
    } catch (e) { setError(String(e)); }
    finally { setAddingAttachments(false); }
  };
  // —— 粘贴/拖放附件：截图与文件不必绕道文件选择器，直接进附件芯片区 ——
  const attachPaths = async (paths: string[]) => {
    if (!paths.length) return;
    if (!preview && !snapshot?.connected) { setError("会话未连接，无法添加附件"); return; }
    try {
      setAttachments(await invoke<Attachment[]>("zcode_attachment_info", { sessionId: session.id, paths: mergeAttachmentRefs(attachments.map(item => item.ref), paths) }));
    } catch (e) { setError(String(e)); }
  };
  // 系统剪贴板直读（零字节拷贝）：资源管理器复制的文件直引原路径，位图由后端落临时 PNG。
  // 两者皆无或读取失败返回 false，调用方回落到 WebView File 字节兜底。
  const attachFromSystemClipboard = async (): Promise<boolean> => {
    try {
      const paths = await invoke<string[]>("clipboard_file_paths");
      if (paths.length) { await attachPaths(paths); return true; }
      const bitmap = await invoke<string | null>("paste_clipboard_image");
      if (bitmap) { await attachPaths([bitmap]); return true; }
    } catch { /* 系统剪贴板不可读时由字节兜底接管 */ }
    return false;
  };
  const ingestFiles = async (files: File[]) => {
    if (!files.length || ingesting.current) return;
    if (preview) {
      setAttachments(value => [...value, ...files.map((file, index) => ({ ref: `preview-paste-${Date.now()}-${index}`, fileName: file.name || "image.png", mime: file.type || "image/png", bytes: file.size }))]);
      return;
    }
    ingesting.current = true;
    try {
      if (await attachFromSystemClipboard()) return;
      // 兜底：把 WebView 给出的 File 字节写到临时目录（WebView 合成文件等场景）。
      const payload: { name: string; bytes: number[] }[] = [];
      for (const file of files) payload.push({ name: file.name || "image.png", bytes: Array.from(new Uint8Array(await file.arrayBuffer())) });
      await attachPaths(await invoke<string[]>("write_pasted_files_to_temp", { files: payload }));
    } catch (e) { setError(String(e)); }
    finally { ingesting.current = false; }
  };
  const selectModel = async (modelId: string) => {
    if (!modelId || selectingModel || snapshot?.busy) return;
    setSelectingModel(true); setError("");
    try {
      if (preview) setSnapshot(value => value && { ...value, sequence: value.sequence + 1, selectedModelId: modelId, error: null });
      else apply(await invoke<Snapshot>("set_zcode_model", { sessionId: session.id, modelId }));
    } catch (e) { setError(String(e)); }
    finally { setSelectingModel(false); }
  };
  const selectMode = async (mode: PermissionMode) => {
    if (!snapshot?.connected || snapshot.busy || sending || selectingModel || selectingMode) return;
    setSelectingMode(true); setError(""); setModeMenu(false);
    try {
      if (preview) setSnapshot(value => value && { ...value, sequence: value.sequence + 1, permissionMode: mode, error: null });
      else apply(await invoke<Snapshot>("set_zcode_permission_mode", { sessionId: session.id, mode }));
    } catch (e) { setError(String(e)); }
    finally { setSelectingMode(false); }
  };
  const send = async () => {
    if (sendInFlight.current) return;
    const message = withConversationQuotes(composeMessage(draft, references), quotes); if ((!message && !attachments.length) || addingAttachments || sending || selectingModel || selectingMode || !snapshot?.connected || !!snapshot.pendingPermission || !snapshot.selectedModelId || (snapshot.busy && (!snapshot.canQueue || !["enqueue", "guide"].includes(snapshot.inputRouting || ""))) || (!snapshot.busy && snapshot.canQueue && snapshot.inputRouting !== "startNow")) return;
    // 原生桌面同款发送门：goal/compact/resume 是类型化 v4 命令，/plan 是 planEnabled 快捷方式，
    // 其余 /xxx 保持普通 sendText 由 runtime 解析（技能、/init 等）。
    const slash = preview || quotes.length ? null : parseVisibleSlashCommand(draft, attachments.length + references.length);
    if (slash?.kind === "emptyGoal") { setError("/goal 需要目标文本，例如 /goal 完成登录重构"); return; }
    if (slash?.kind === "unsupportedGoal") { setError(`暂不支持 /goal ${slash.action}`); return; }
    if (slash?.kind === "unsupportedPlanShortcut") { setError("/plan 暂不支持携带附件或引用，请先移除后重试"); return; }
    if (slash?.kind === "planShortcut" && !slash.task) { await selectMode("plan"); return; }
    // /side|/btw 文本命令：带文字时作为辅助对话首条输入打开分栏（原生同款消费，不进主对话）。
    const sideCommand = preview || quotes.length ? null : /^\/(?:side|btw)(?:\s+([\s\S]+))?$/i.exec(draft.trim());
    if (sideCommand) { await openSideConversation(sideCommand[1]); return; }
    const submitted = { text: draft, attachments, references, quotes };
    sendInFlight.current = true;
    setSending(true); setError("");
    setDraft(""); setReferences([]); setQuotes([]); setAttachments([]); setMenu(null); setCaret(0);
    const refreshSnapshot = async () => { try { apply(await invoke<Snapshot>("zcode_snapshot", { sessionId: session.id })); } catch (e) { setError(String(e)); } };
    let slashDispatched = false;
    try {
      if (preview && snapshot.busy) {
        setSnapshot(value => value && { ...value, sequence: value.sequence + 1, queuedInputs: [...value.queuedInputs, { queueItemId: `preview-queue-${value.sequence}`, sourceCommandId: `preview-command-${value.sequence}`, text: message, attachments, dispatch: { state: "queued" } }] });
      } else if (preview) {
        setSnapshot(value => value && { ...value, sequence: value.sequence + 1, busy: true, inputRouting: "enqueue", rows: [...value.rows,
          { rowId: value.rows.length + 1, kind: "userInput", text: message, attachments },
          { rowId: value.rows.length + 2, kind: "reasoning", text: "模拟思考内容，用于检查实时显示。", state: "streaming" },
          { rowId: value.rows.length + 3, kind: "toolCall", toolName: "演示工具", inputText: "模拟操作，不执行命令", status: "pendingApproval" }],
          pendingPermission: { requestId: "preview-permission", toolName: "演示工具", input: "模拟操作，不执行命令" } });
      } else if (slash?.kind === "planShortcut") {
        slashDispatched = true;
        await invoke<string>("send_zcode_message", { sessionId: session.id, message: slash.task, attachments: [], planEnabled: true });
        await refreshSnapshot();
      } else if (slash) {
        slashDispatched = true;
        await invoke<void>("zcode_slash_command", { sessionId: session.id, kind: slash.kind, text: slash.kind === "sendGoalCommand" ? slash.objective : null, displayText: slash.displayText });
        await refreshSnapshot();
      } else {
        await invoke<string>("send_zcode_message", { sessionId: session.id, message, attachments: attachments.map(item => item.ref) });
        // Submission is already acknowledged; a later snapshot failure must not restore it as unsent.
        await refreshSnapshot();
      }
    } catch (e) {
      const current = currentComposer.current;
      const disposition = rejectedSubmissionDisposition(e, !!(current.text || current.references.length || current.quotes.length || current.attachments.length));
      if (slashDispatched && (current.text || current.references.length || current.quotes.length || current.attachments.length)) {
        setFailedSubmission(submitted);
      } else if (slashDispatched) {
        // 类型化命令被拒（如 plan/goal 互斥）不是排队输入，直接还原草稿供修改重发。
        setDraft(submitted.text); setReferences(submitted.references); setQuotes(submitted.quotes); setAttachments(submitted.attachments);
      } else if (disposition === "review") {
        try { apply(await invoke<Snapshot>("zcode_snapshot", { sessionId: session.id })); } catch { /* Backend persists every pending input before dispatch, including IPC loss. */ }
      } else if (disposition === "preserve") setFailedSubmission(submitted);
      else { setDraft(submitted.text); setReferences(submitted.references); setQuotes(submitted.quotes); setAttachments(submitted.attachments); }
      setError(String(e));
    }
    finally { sendInFlight.current = false; setSending(false); }
  };
  // /side、/btw 辅助对话：同一 ZCode Desktop 进程内的子会话分栏（原生 selection-side-session），
  // 不新建 CrewVela 会话、不新开窗口；父会话 busy 时同样可用（服务端独立 admission）。
  const openSideConversation = async (firstInput?: string) => {
    if (sideWorking || !snapshot?.connected || snapshot.pendingPermission || !snapshot.selectedModelId) return false;
    // 服务端 fork 从 sessionStore 读父会话；draft 会话在首条用户消息后才持久化（原生同款），
    // 空会话直接开辅助对话会得到 Session not found，这里先给出可行动的提示。
    if (!snapshot.rows.length) { setError("主对话还没有消息：先发送一条消息，再打开辅助对话"); return false; }
    setSideWorking(true); setError("");
    try {
      if (preview) setSnapshot(value => value && { ...value, sideDetail: { childSessionId: "preview-side", rows: [], loading: false, error: null } });
      else apply(await invoke<Snapshot>("zcode_side_session", { sessionId: session.id, firstInput: firstInput?.trim() || null }));
      return true;
    } catch (e) { setError(String(e)); return false; }
    finally { setSideWorking(false); }
  };
  const sendSide = async () => {
    const message = withConversationQuotes(sideDraft.trim(), sideQuotes);
    if (!message || sideWorking || !snapshot?.connected || snapshot.pendingPermission) return;
    setSideWorking(true); setSideError("");
    setSideDraft(""); setSideQuotes([]);
    try {
      if (preview) setSnapshot(value => value && value.sideDetail ? { ...value, sideDetail: { ...value.sideDetail, rows: [...value.sideDetail.rows, { rowId: Date.now(), kind: "userInput", text: message }, { rowId: Date.now() + 1, kind: "assistantText", text: "模拟辅助对话回复。" }] } } : value);
      else apply(await invoke<Snapshot>("zcode_side_send", { sessionId: session.id, message }));
    } catch (e) { setSideError(String(e)); setSideDraft(value => value ? `${message}\n${value}` : message); }
    finally { setSideWorking(false); }
  };
  const closeSide = async () => {
    if (sideWorking || !snapshot?.connected) return;
    setSideWorking(true); setSideError("");
    try {
      if (preview) setSnapshot(value => value && { ...value, sideDetail: null }); else apply(await invoke<Snapshot>("zcode_side_close", { sessionId: session.id }));
    } catch (e) { setError(String(e)); }
    finally { setSideWorking(false); }
  };

  const restoreText = (text: string, recovered: Attachment[] = []) => {
    setDraft(value => [value.trimEnd(), text].filter(Boolean).join("\n"));
    setAttachments(value => [...value, ...recovered.filter(item => !value.some(existing => existing.ref === item.ref))]);
    input.current?.focus();
  };
  const queueAction = async (itemId?: string, op?: "send_now" | "delete"): Promise<boolean> => {
    if (queueWorking || sending || !snapshot?.connected || snapshot.pendingPermission) return false;
    setQueueWorking(true); setError("");
    try {
      if (preview) setSnapshot(value => {
        if (!value) return value;
        const remaining = itemId ? value.queuedInputs.filter(item => item.queueItemId !== itemId) : [];
        const promoted = op === "send_now" && itemId ? value.queuedInputs.filter(item => item.queueItemId === itemId) : [];
        const resumedRows: Row[] = (op === "send_now" ? promoted : itemId ? [] : value.queuedInputs).flatMap((item, index) => [{ rowId: value.rows.length + index * 2 + 1, kind: "userInput", text: item.text, attachments: item.attachments, sourceCommandId: item.sourceCommandId }, { rowId: value.rows.length + index * 2 + 2, kind: "assistantText", text: "模拟队列回复。", state: "complete", sourceCommandId: item.sourceCommandId }]);
        return { ...value, sequence: value.sequence + 1, busy: itemId && op !== "send_now" ? value.busy : false, queuedInputs: remaining, rows: [...value.rows, ...resumedRows], inputRouting: itemId && value.busy ? "enqueue" : remaining.length && !value.queueAutoDrain ? "choice" : "startNow", queueAutoDrain: itemId ? value.queueAutoDrain : true };
      });
      else apply(await invoke<Snapshot>("zcode_queue_action", { sessionId: session.id, queueItemId: itemId ?? null, op: op ?? null }));
      return true;
    } catch (e) { setError(String(e)); return false; } finally { setQueueWorking(false); }
  };
  const recoverQueued = async (item: QueueItem, discard = false) => {
    if (queueWorking || sending) return;
    setQueueWorking(true); setError("");
    try {
      const restored = preview ? item : await invoke<QueueItem>("recover_zcode_queue_input", { sessionId: session.id, queueItemId: item.queueItemId, discard });
      if (!mounted.current) return;
      if (!discard) restoreText(restored.text, restored.attachments);
      if (preview && discard) setSnapshot(value => value && { ...value, recoverableInputs: value.recoverableInputs.filter(entry => entry.queueItemId !== item.queueItemId) });
      else if (!preview) apply(await invoke<Snapshot>("zcode_snapshot", { sessionId: session.id }));
    } catch (e) { setError(String(e)); } finally { setQueueWorking(false); }
  };
  const answer = async (allow: boolean) => {
    if (!snapshot?.pendingPermission || answering) return; setAnswering(true);
    try {
      if (preview) setSnapshot(value => value && { ...value, sequence: value.sequence + 1, busy: false, inputRouting: value.queuedInputs.length ? "choice" : "startNow", queueAutoDrain: false, pendingPermission: null,
        rows: [...value.rows.map(row => row.kind === "reasoning" ? { ...row, state: "complete" } : row.status === "pendingApproval" ? { ...row, status: allow ? "success" : "cancelled" } : row),
          { rowId: value.rows.length + 1, kind: "assistantText", state: "complete", text: allow ? "模拟回复：已允许本次演示操作。" : "模拟回复：已拒绝本次演示操作。" }] });
      else await invoke("answer_zcode_permission", { sessionId: session.id, requestId: snapshot.pendingPermission.requestId, allow });
    } catch (e) { setError(String(e)); } finally { setAnswering(false); }
  };
  const cancel = async () => {
    if (cancelInFlight.current || !snapshot?.busy) return;
    cancelInFlight.current = true; setCancelling(true); setError("");
    try {
      if (preview) setSnapshot(value => value && { ...value, sequence: value.sequence + 1, busy: false, inputRouting: value.queuedInputs.length ? "choice" : "startNow", queueAutoDrain: false, pendingPermission: null,
        rows: value.rows.map(row => row.state === "streaming" ? { ...row, state: "interrupted" } : row.status === "pendingApproval" ? { ...row, status: "cancelled" } : row) });
      else await invoke("cancel_zcode_turn", { sessionId: session.id });
    } catch (e) { cancelInFlight.current = false; setCancelling(false); setError(String(e)); }
  };
  useEffect(() => { if (!snapshot?.busy || !snapshot.connected || snapshot.error) { cancelInFlight.current = false; setCancelling(false); } }, [snapshot?.busy, snapshot?.connected, snapshot?.error]);
  const stop = async () => {
    try {
      if (preview) { await cancel(); setSnapshot(value => value && { ...value, connected: false }); }
      else onSessionRecord(await invoke<SessionRecord>("stop_session", { sessionId: session.id }));
    } catch (e) { setError(String(e)); }
  };
  const copyMessage = async (text: string) => {
    try { await navigator.clipboard.writeText(text); } catch (e) { setError(`复制失败：${String(e)}`); }
  };
  const editMessage = async () => {
    if (!editing?.entityId || editWorking || !snapshot?.connected) return;
    setEditWorking(true); setError("");
    try {
      if (preview) setSnapshot(value => value && { ...value, sequence: value.sequence + 1, busy: true, inputRouting: "enqueue", rows: [...value.rows.filter(row => row.rowId < editing.rowId), { ...editing, text: editText }] });
      else await invoke("edit_zcode_message", { sessionId: session.id, rowId: editing.rowId, entityId: editing.entityId, message: editText });
      setEditing(null);
    } catch (e) { setError(String(e)); } finally { setEditWorking(false); }
  };
  const captureSelection = (event: React.SyntheticEvent<HTMLParagraphElement>) => {
    const selected = window.getSelection();
    if (!selected || selected.isCollapsed || !selected.rangeCount) { setSelection(null); return; }
    const range = selected.getRangeAt(0);
    if (!event.currentTarget.contains(range.startContainer) || !event.currentTarget.contains(range.endContainer)) { setSelection(null); return; }
    const text = selected.toString().trim();
    const rect = range.getBoundingClientRect();
    setSelection(text ? { text, x: Math.max(8, Math.min(rect.left, window.innerWidth - 290)), y: Math.max(8, rect.top - 42) } : null);
  };
  const quoteSelection = async (side: boolean) => {
    if (!selection) return;
    try {
      const next = addConversationQuote(side ? sideQuotes : quotes, selection.text);
      if (side) {
        // 创建失败时保留选择，避免引用草稿无处可见。
        if (!snapshot?.sideDetail && !await openSideConversation()) return;
        setSideQuotes(next); requestAnimationFrame(() => sideInput.current?.focus());
      } else { setQuotes(next); input.current?.focus(); }
      setSelection(null); window.getSelection()?.removeAllRanges();
    } catch (e) { setError(String(e)); }
  };
  const quoteCards = (items: ConversationQuote[], update: (items: ConversationQuote[]) => void) => items.length > 0 && <div className="zcode-quote-cards" aria-label={t("对话引用")}>{items.map((quote, index) => <div key={index}><span>{quote.text}</span><button type="button" aria-label={`移除对话引用 ${index + 1}`} onClick={() => update(items.filter((_, i) => i !== index))}><X size={14} /></button></div>)}</div>;
  const empty = !snapshot?.rows.length && !snapshot?.pendingPermission;
  return <section className={`harness-chat native-agent-chat zcode-chat${empty ? " harness-chat-empty" : ""}${childId ? " zcode-child-open" : ""}${snapshot?.sideDetail ? " zcode-side-open" : ""}`} aria-label="ZCode Desktop 原生会话">
    <header className="harness-chat-header"><div className="harness-chat-controls">
      <span className="harness-workspace" title={session.cwd}><FolderSimple size={17} />{t("工作目录：")}{session.cwd.split(/[\\/]/).filter(Boolean).pop()}</span>
      <span className={`harness-connection${snapshot?.connected ? " connected" : ""}`}><i />{connecting ? t("连接中") : snapshot?.connected ? t("已连接") : t("未连接")}</span>
      {snapshot?.connected ? <button type="button" onClick={() => void stop()}>{t("停止会话")}</button> : <button type="button" disabled={connecting} onClick={() => void connect()}>{t("恢复会话")}</button>}
    </div></header>
    {preview && <div className="harness-chat-preview-banner">UI 预览：以下内容为模拟数据，不连接 ZCode Desktop、不调用模型、不执行工具。</div>}
    {session.agentRuntimeKind === "zcode_cli" ? <div className="native-agent-selector"><button type="button" className="btn secondary native-agent-handoff" title={t("把当前工作交接给其它Agent")} aria-label={t("把当前工作交接给其它Agent")} onClick={switchProps.onSwitchCli} disabled={!switchProps.onSwitchCli}><ArrowsLeftRight size={17} aria-hidden="true" /></button></div> : <NativeAgentSelector kind="zcode" {...switchProps} />}
    {selection && <div className="zcode-selection-menu" role="toolbar" aria-label="选中文本操作" style={{ left: selection.x, top: selection.y }} onPointerDown={e => e.preventDefault()}><button type="button" onClick={() => void quoteSelection(false)}>添加到当前任务</button><button type="button" disabled={sideWorking || !snapshot?.connected || !!snapshot.pendingPermission} onClick={() => void quoteSelection(true)}>在辅助对话中提问</button><button type="button" aria-label="关闭选中文本操作" onClick={() => setSelection(null)}><X size={14} /></button></div>}
    <div className="harness-chat-messages">
      {empty && <NativeAgentWelcome kind="zcode" active={active} skillsDisabled={!snapshot?.connected || sending} onSkills={() => { setMenu("all"); if (!catalog && !catalogLoading) void loadCatalog(); input.current?.focus(); }} />}
      <div className="zcode-message-column">
      {snapshot?.rows.map(row => {
        if (row.kind === "subagent") return subagentCard(row);
        if (row.kind === "toolCall" && pairedSubagent(snapshot.rows, row)) return null;
        if (row.kind === "reasoning") return <article className="harness-message assistant" key={row.rowId}><details className="harness-reasoning" open={row.state === "streaming"}><summary>{row.state === "streaming" ? "正在思考" : row.durationMs ? `思考 · 持续了 ${Math.max(1, Math.ceil(row.durationMs / 1000))} 秒` : t("思考过程")}</summary><p>{row.text}</p></details>{row.state === "interrupted" && <small>{t("已取消，显示部分内容")}</small>}</article>;
        if (row.kind === "toolCall") {
          const display = row.display ?? null;
          const mcp = display?.kind === "mcp_tool" ? display : null;
          const inputObj = parseJsonObject(row.inputText);
          const command = typeof inputObj?.command === "string" ? inputObj.command : Array.isArray(inputObj?.command) ? (inputObj.command as unknown[]).join(" ") : null;
          // 人话标题链：display.description → title → input.description → 工具名。
          const humanTitle = display?.description || row.title || (typeof inputObj?.description === "string" ? inputObj.description : "") || row.toolName || "工具调用";
          const isTodo = /todo/i.test(row.toolName || "") || display?.kind === "todo";
          const todoSteps = isTodo ? extractTodoSteps(row) : null;
          const livePreview = row.outputPreview?.text || row.progress?.previewLine || "";
          const running = row.status === "running" || row.status === "inputStreaming";
          const toolLabel = mcp ? `MCP · ${mcp.serverName ?? ""}` : row.toolName || "工具调用";
          return <details className="harness-tool zcode-tool" key={row.rowId}>
            <summary><strong>{humanTitle}</strong><span className="zcode-tool-status">{row.status ? toolStatusLabels[row.status] || row.status : "工具调用"}</span><span className="zcode-tool-description" title={toolLabel}>{toolLabel}</span></summary>
            <div className="zcode-tool-details">
              {isTodo && todoSteps ? <ul className="zcode-todo-list">{todoSteps.map((step, index) => <li key={index} data-status={step.status}><i>{step.status === "completed" ? "●" : step.status === "in_progress" ? "◐" : "○"}</i>{step.content}</li>)}</ul>
                : command ? <pre className="zcode-tool-cmd">$ {command}</pre>
                : row.inputText && <><small>{t("调用参数")}</small><pre>{toolDetails(row.inputText)}</pre></>}
              {livePreview && running && <pre className="zcode-tool-live" role="status">{livePreview}</pre>}
              {row.output?.text && <><small>{t("返回内容")}</small><pre>{row.output.text}</pre></>}
              {!row.inputText && !row.output?.text && !command && !todoSteps && <small>{t("暂无调用详情")}</small>}
            </div>
          </details>;
        }
        if (row.kind === "turnHeader") return row.state === "failed" || row.state === "completedInterrupted" ? <p className="harness-chat-info" key={row.rowId}>{row.state === "failed" ? "本轮执行失败" : "本轮已取消，部分内容保留"}</p> : null;
        if (row.kind !== "userInput" && row.kind !== "assistantText") return null;
        return <article className={`harness-message ${row.kind === "userInput" ? "user" : "assistant"}`} key={row.rowId}><strong>{row.kind === "userInput" ? "你" : "ZCode Desktop"}</strong>{editing?.rowId === row.rowId ? <form className="zcode-message-edit" onSubmit={e => { e.preventDefault(); void editMessage(); }}><textarea aria-label={t("编辑用户消息")} value={editText} onChange={e => setEditText(e.target.value)} disabled={editWorking} /><div><button type="button" disabled={editWorking} onClick={() => setEditing(null)}>{t("取消编辑")}</button><button type="submit" disabled={editWorking || (!editText.trim() && !row.attachments?.length)}>{editWorking ? "提交中…" : "保存并重新生成"}</button></div><small>重新生成此条消息后的对话，保留当前文件。</small></form> : row.text && <p onMouseUp={row.kind === "assistantText" ? captureSelection : undefined} onKeyUp={row.kind === "assistantText" ? captureSelection : undefined}>{row.kind === "userInput" ? messageContent(row.text) : row.text}</p>}{row.kind === "userInput" && <div className="zcode-message-actions"><button type="button" aria-label={t("复制用户消息")} title={t("复制")} onClick={() => void copyMessage(row.text || "")}><Copy size={16} /></button>{canEditUserMessage(row) && <button type="button" aria-label={t("编辑用户消息")} title={t("编辑")} disabled={editWorking || !snapshot.connected || !!snapshot.pendingPermission} onClick={() => { setEditing(row); setEditText(row.text || ""); }}><PencilSimple size={16} /></button>}</div>}{row.attachments?.length ? <div className="harness-attachments">{row.attachments.map((item, index) => <span key={index}><Paperclip size={15} />{item.fileName} · {Math.ceil(item.bytes / 1024)} KB</span>)}</div> : !row.text && <p>…</p>}{row.state === "streaming" && <small>{t("生成中…")}</small>}{row.state === "interrupted" && <small>{t("已取消，显示部分内容")}</small>}</article>;
      })}
      {snapshot?.pendingPermission && <article className="harness-permission"><strong>{t("需要确认：")}{snapshot.pendingPermission.toolName || "工具调用"}</strong><pre>{typeof snapshot.pendingPermission.input === "string" ? snapshot.pendingPermission.input : JSON.stringify(snapshot.pendingPermission.input, null, 2)}</pre><div><button type="button" disabled={answering} onClick={() => void answer(true)}>{t("允许一次")}</button><button type="button" disabled={answering} onClick={() => void answer(false)}>{t("拒绝")}</button></div></article>}
      {!snapshot?.connected && !connecting && <p className="harness-chat-info">{t("会话未连接。恢复后继续原 ZCode Desktop 会话，历史保留。")}</p>}
      <div ref={end} />
      </div>
    </div>
    {childId && <aside className="zcode-child-pane" aria-label={t("子智能体详情")}><header><strong>{snapshot?.rows.find(row => row.kind === "subagent" && row.childSessionId === childId)?.subagentType || "子智能体"} · {snapshot?.rows.find(row => row.kind === "subagent" && row.childSessionId === childId)?.summaryText || "详情"}</strong><span>{t("只读")}</span><button type="button" aria-label="关闭子智能体详情" onClick={() => setChildId(null)}><X size={18} /></button></header>
      <div className="zcode-child-content">{childLoading && <p>正在订阅子会话…</p>}{childError && <p role="alert">{childError}</p>}{(childError || snapshot?.childDetail?.error) && <button type="button" onClick={() => setChildRetry(value => value + 1)}>{t("重新加载详情")}</button>}{!preview && snapshot?.childDetail?.childSessionId === childId && <>{snapshot.childDetail.loading && <p>等待子会话快照…</p>}{snapshot.childDetail.error && <p role="alert">{snapshot.childDetail.error}</p>}</>}
      {(preview ? [{ rowId: 1, kind: "reasoning", text: "核对入口和命令实现。", state: "streaming" }, { rowId: 2, kind: "toolCall", toolName: "Read", text: "读取 cli/src/args.ts", status: "success" }, { rowId: 3, kind: "assistantText", text: childId === "preview-child-b" ? "已核对协议订阅。" : "已定位 CLI/TUI 的关键实现。" }] : snapshot?.childDetail?.childSessionId === childId ? snapshot.childDetail.rows : []).map(row => row.kind === "reasoning" ? <details className="harness-reasoning" key={row.rowId}><summary>{t("思考过程")}</summary><p>{row.text}</p></details> : row.kind === "toolCall" ? <details className="harness-tool" key={row.rowId}><summary>{row.toolName} · {toolStatusLabels[row.status || ""] || row.status}</summary><pre>{row.inputText && toolDetails(row.inputText)}</pre><pre>{row.text}</pre></details> : ["assistantText", "userInput"].includes(row.kind) ? <article className="harness-message" key={row.rowId}><p>{row.text}</p></article> : null)}
      </div></aside>}
    {(error || snapshot?.error) && <pre className="harness-chat-error" role="alert">{error || snapshot?.error}</pre>}
    {(snapshot?.connected && !!snapshot.queuedInputs?.length) && <div className="zcode-queue-panel"><header><strong>待发送 · {snapshot.queuedInputs.length}</strong>{!snapshot.queueAutoDrain && <button type="button" disabled={queueWorking || !!snapshot.pendingPermission || sending} onClick={() => void queueAction()}>{t("继续队列")}</button>}</header>{snapshot.queuedInputs.map(item => <div key={item.queueItemId}><span>{item.text || `${item.attachments?.length || 0} 个附件`}</span><div className="zcode-queue-item-actions">{item.dispatch.state === "queued" && <><button type="button" title="立即发送这一条" disabled={queueWorking || !!snapshot.pendingPermission || sending} onClick={() => void queueAction(item.queueItemId, "send_now")}>⏫ 立即</button><button type="button" title="放回输入框修改" disabled={queueWorking || !!snapshot.pendingPermission || sending} onClick={() => { const text = item.text || ""; void queueAction(item.queueItemId, "delete").then(removed => { if (removed) restoreText(text, item.attachments); }); }}>✎ 编辑</button></>}<button type="button" disabled={queueWorking || !!snapshot.pendingPermission || sending || item.dispatch.state !== "queued"} onClick={() => void queueAction(item.queueItemId, "delete")}>🗑 删除</button></div></div>)}</div>}
    {!!snapshot?.recoverableInputs?.length && <div className="zcode-queue-panel"><header><strong>{t("待核对内容")}</strong><small>重启后队列未保留，核对历史后恢复；不会自动重发</small></header>{snapshot.recoverableInputs.map(item => <div key={item.queueItemId}><span>{item.text || "附件消息"}</span><button type="button" disabled={queueWorking || sending} onClick={() => void recoverQueued(item)}>{t("恢复到输入框")}</button><button type="button" disabled={queueWorking || sending} onClick={() => void recoverQueued(item, true)}>{t("移除记录")}</button></div>)}</div>}
    {failedSubmission && <div className="zcode-queue-panel"><span>{failedSubmission.uncertain ? "上一条发送结果待核对，请先查看历史；不会自动重发" : "上一条未发送，新草稿已保留"}</span><button type="button" onClick={() => { restoreText(withConversationQuotes(composeMessage(failedSubmission.text, failedSubmission.references), failedSubmission.quotes), failedSubmission.attachments); setFailedSubmission(null); }}>{t("恢复到输入框")}</button></div>}
    <form className={`harness-chat-composer zcode-composer${dropActive ? " zcode-drop-active" : ""}`} onSubmit={e => { e.preventDefault(); void send(); }}
      onDragOver={e => { if (!e.dataTransfer.types.includes("Files")) return; e.preventDefault(); e.dataTransfer.dropEffect = "copy"; setDropActive(true); }}
      onDragLeave={e => { if (!(e.relatedTarget instanceof Node) || !e.currentTarget.contains(e.relatedTarget)) setDropActive(false); }}
      onDrop={e => { if (!e.dataTransfer.types.includes("Files")) return; e.preventDefault(); setDropActive(false); void ingestFiles(Array.from(e.dataTransfer.files)); }}>
      {menuOpen && <div className="harness-command-menu zcode-command-menu" aria-label="ZCode Desktop 能力菜单">
        <header><strong>{menu === "all" ? "添加" : menu === "skills" || tokenKind === "skills" ? t("技能") : menu === "plugins" || tokenKind === "plugins" ? t("插件") : "能力"}</strong><small>{t("↑↓ 选择 · Tab 补全")}</small><button type="button" aria-label="关闭能力菜单" onClick={() => { setMenu(null); input.current?.focus(); }}>×</button></header>
        {menu === "all" && <div className="zcode-menu-actions"><button type="button" onClick={() => void addAttachments()}><Paperclip size={17} />{t("附件")}</button>{allItems.some(item => item.kind === "commands" && item.name === "workflow") && <button type="button" onClick={() => choose(allItems.find(item => item.kind === "commands" && item.name === "workflow")!)}>↳ 工作流</button>}<button type="button" onClick={() => setMenu("skills")}>✨ 技能</button><button type="button" onClick={() => setMenu("commands")}>/ 能力</button><button type="button" onClick={() => setMenu("plugins")}>🔌 插件</button></div>}
        {catalogLoading && <small role="status">正在读取原生目录…</small>}
        {catalogError && <small role="alert">{catalogError}</small>}
        {catalog && Object.entries(catalog).filter(([kind]) => menu === "all" || kind === (menu || tokenKind)).map(([kind, group]) => group.error && <small role="alert" key={kind}>{kind}：{group.error}</small>)}
        {catalog?.commands.notice && (menu === "all" || menu === "commands" || tokenKind === "commands") && <small>{catalog.commands.notice}</small>}
        <div className="zcode-menu-items" role="listbox" aria-label="ZCode Desktop 可选能力">{shown.map((item, index) => <button type="button" role="option" aria-selected={index === activeIndex} disabled={item.disabled} className={index === activeIndex ? "harness-skill-active" : ""} key={`${item.kind}:${item.id}`} onMouseEnter={() => setActiveIndex(index)} onClick={() => choose(item)}><span>{item.kind === "skills" ? "✨" : item.kind === "plugins" ? "🔌" : "↳"} {item.label}</span><small>{item.disabled ? "插件名称冲突，无法引用" : item.description}</small></button>)}</div>
        {!catalogLoading && !shown.length && <small>{query ? "没有匹配项" : "当前目录没有可用项"}</small>}
        <button type="button" disabled={catalogLoading || !!snapshot?.pendingPermission} onClick={() => void loadCatalog()}>刷新原生目录</button>
      </div>}
      {(references.length > 0 || attachments.length > 0) && <div className="zcode-selected-references">{references.map(item => <span className="zcode-reference" key={`${item.kind}:${item.id}`}>{item.kind === "skills" ? "✨" : item.kind === "plugins" ? "🔌" : "↳"} {item.label}<button type="button" aria-label={`移除 ${item.label}`} disabled={sending} onClick={() => setReferences(value => value.filter(ref => ref !== item))}><X size={13} /></button></span>)}{attachments.map(item => <span key={item.ref}><Paperclip size={15} />{item.fileName}<button type="button" aria-label={`移除附件 ${item.fileName}`} disabled={sending || addingAttachments} onClick={() => setAttachments(value => value.filter(ref => ref.ref !== item.ref))}><X size={13} /></button></span>)}</div>}
      {quoteCards(quotes, setQuotes)}
      <textarea ref={input} aria-label={t("发送给 ZCode Desktop")} placeholder={t("描述任务，输入 $ 选择技能、/ 选择能力、@ 选择插件…")} value={draft} onChange={e => { setDraft(e.target.value); setCaret(e.target.selectionStart); setMenu(null); }} onSelect={e => setCaret(e.currentTarget.selectionStart)} disabled={!snapshot?.connected || connecting}
        onPaste={e => {
          // 粘贴分流：文件（截图/复制文件/浏览器图片）进附件区，纯文本走原生插入，
          // 空剪贴板时可能是 WebView 未暴露的位图/文件，交后端直读系统剪贴板。
          const files = Array.from(e.clipboardData?.files ?? []);
          const kind = classifyComposerPaste(files, e.clipboardData?.getData("text/plain") ?? "");
          if (kind === "text") return;
          e.preventDefault();
          if (kind === "system") { if (!preview) void attachFromSystemClipboard(); return; }
          void ingestFiles(files);
        }}
        onKeyDown={e => {
        if (e.nativeEvent.isComposing) return;
        if (menuOpen && (e.key === "ArrowDown" || e.key === "ArrowUp")) { e.preventDefault(); setActiveIndex(value => shown.length ? (value + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length : 0); return; }
        if (menuOpen && (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) && shown[activeIndex] && !shown[activeIndex].disabled) { e.preventDefault(); choose(shown[activeIndex]); return; }
        if (e.key === "Escape") { setMenu(null); setCaret(0); return; }
        if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); e.currentTarget.form?.requestSubmit(); }
      }} />
      <div className="harness-composer-toolbar"><div className="harness-composer-left">{snapshot?.busy && <span className="zcode-busy-dot" role="status" aria-label={t("任务进行中")} title={t("任务进行中")}><CircleNotch size={15} className="zcode-activity-spin" /></span>}<button type="button" className="harness-icon-button" aria-label={t("ZCode Desktop 功能菜单")} disabled={!snapshot?.connected || sending} onClick={() => { setMenu(value => value ? null : "all"); if (!catalog && !catalogLoading) void loadCatalog(); }}><Plus size={20} /></button><button type="button" className="harness-icon-button" aria-label={t("添加 ZCode Desktop 附件")} disabled={!snapshot?.connected || sending || addingAttachments} onClick={() => void addAttachments()}><Paperclip size={20} /></button><div className="zcode-mode-picker" ref={modePicker}><button type="button" className="harness-picker zcode-mode-trigger" data-mode={snapshot?.permissionMode ?? ""} aria-label={t("ZCode Desktop 权限模式")} aria-haspopup="menu" aria-expanded={modeMenu} disabled={!snapshot?.connected || snapshot.busy || sending || selectingModel || selectingMode || connecting} onClick={() => { setModeMenu(value => !value); setMenu(null); }}><Shield size={17} />{selectingMode ? "切换中…" : t(permissionModes.find(mode => mode.id === snapshot?.permissionMode)?.label || "权限模式")}<CaretDown size={13} /></button>{modeMenu && <div className="zcode-mode-menu" role="menu" aria-label={t("ZCode Desktop 权限模式选项")}>{permissionModes.map(mode => <button type="button" key={mode.id} role="menuitemradio" data-mode={mode.id} aria-checked={snapshot?.permissionMode === mode.id} onClick={() => void selectMode(mode.id)}><span><strong>{t(mode.label)}</strong><small>{t(mode.description)}</small></span>{snapshot?.permissionMode === mode.id && <Check size={18} />}</button>)}</div>}</div>{(() => { const usage = snapshot?.contextUsage; if (!usage || !(usage.size > 0)) return null; const pct = Math.min(100, Math.round((usage.used / usage.size) * 1000) / 10); const hitRate = usage.cache?.hitRate ?? usage.cache?.latestHitRate ?? null; const segments = (usage.breakdown ?? []).filter(item => item.chars > 0).map(item => ({ ...item, meta: usageSourceMeta.find(m => m.source === item.source) })).sort((a, b) => (a.meta?.order ?? 99) - (b.meta?.order ?? 99)); const totalChars = segments.reduce((sum, item) => sum + item.chars, 0); return <div className="zcode-usage-picker" ref={usagePicker}><button type="button" className="harness-picker zcode-usage-trigger" aria-label="ZCode Desktop 上下文容量" aria-haspopup="menu" aria-expanded={usageOpen} onClick={() => { setUsageOpen(value => !value); setMenu(null); }}><Gauge size={17} />{pct}%</button>{usageOpen && <div className="zcode-usage-menu" role="dialog" aria-label={t("上下文容量详情")}><header><strong>{t("上下文容量")}</strong><span>{formatCompactTokens(usage.used)} / {formatCompactTokens(usage.size)}（{pct}%）</span></header><div className="zcode-usage-bar" role="img" aria-label={`上下文已用 ${pct}%`}>{segments.length > 0 && segments.map((item, index) => <i key={item.source} data-tone={Math.min(index, 4)} style={{ width: `${Math.max(0.5, (item.chars / (totalChars || 1)) * 100)}%` }} />)}</div><dl>{segments.length > 0 && segments.map((item, index) => <div key={item.source}><i data-tone={Math.min(index, 4)} /><dt>{item.meta?.label ?? item.source}</dt><dd>{Math.round((item.chars / (totalChars || 1)) * 1000) / 10}%</dd></div>)}{hitRate !== null && hitRate >= 0.78 && <div className="zcode-usage-hitrate"><dt>{t("平均缓存命中率")}</dt><dd>{Math.round(hitRate * 100)}%</dd></div>}</dl><footer>{t("模型一次可处理的上下文容量，单位为 Token；请勿超过实际上限。")}</footer></div>}</div>; })()}</div><span>{snapshot?.connected && !snapshot.selectedModelId && ((snapshot.models?.length ?? 0) ? t("先选择 ZCode Desktop 模型") : "未读取到可用模型，可重新加载；仍为空时请检查 ZCode Desktop 账号权益或 API 配置")}</span><div className="harness-composer-right">{snapshot?.busy && <button type="button" className="harness-send-button zcode-stop-button" aria-label={t("停止生成")} title={cancelling ? "正在停止…" : t("停止生成")} disabled={cancelling || !snapshot.connected} onClick={() => void cancel()}>{cancelling ? <CircleNotch size={20} className="zcode-activity-spin" /> : <Square size={18} weight="fill" />}</button>}{snapshot?.connected && <button type="button" className="harness-picker" aria-label={t("重新加载 ZCode Desktop 模型")} title={t("重新读取原生账号的模型目录，保留会话历史")} disabled={connecting || snapshot.busy || sending || selectingModel || selectingMode} onClick={() => void reloadModels()}>{connecting ? "重新加载中…" : t("重新加载模型")}</button>}<label className="harness-picker" title={t("ZCode Desktop 模型")}><select aria-label={t("ZCode Desktop 模型")} value={snapshot?.selectedModelId || ""} disabled={!snapshot?.connected || snapshot.busy || selectingModel || selectingMode || sending || !(snapshot.models?.length)} onChange={e => void selectModel(e.target.value)}><option value="">{selectingModel ? "切换中…" : t("选择模型")}</option>{snapshot?.models?.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}</select></label><button className="harness-send-button" type="submit" aria-label={snapshot?.busy ? t("排队发送") : t("发送")} title={snapshot?.busy ? "排队发送，本轮结束后执行" : t("发送")} disabled={addingAttachments || (!draft.trim() && !references.length && !quotes.length && !attachments.length) || !snapshot?.connected || sending || selectingModel || selectingMode || !snapshot.selectedModelId || !!snapshot.pendingPermission || (snapshot.busy && (!snapshot.canQueue || !["enqueue", "guide"].includes(snapshot.inputRouting || ""))) || (!snapshot.busy && snapshot.canQueue && snapshot.inputRouting !== "startNow")}><ArrowUp size={22} /></button></div></div>
    </form>
    <div className="harness-composer-hint">{snapshot?.busy ? snapshot.pendingPermission ? "先处理工具审批，可继续编辑下一条消息" : "正在回复，发送的新消息将排队执行" : snapshot?.inputRouting === "choice" ? "队列已暂停，请先继续或撤销队列；草稿已保留" : "Enter 发送 · Shift+Enter 换行 · 可粘贴或拖入图片/文件"}</div>
    {!!snapshot?.sideDetail && <aside className="zcode-side-pane" aria-label="ZCode Desktop 辅助对话">
      <header><strong>{t("辅助对话")}</strong><small>同进程分栏 · 独立上下文</small><button type="button" disabled={sideWorking} onClick={() => void closeSide()}>{t("关闭")}</button></header>
      <div className="zcode-side-content">
        {snapshot.sideDetail.loading && <p>{t("等待辅助对话快照…")}</p>}
        {!!(sideError || snapshot.sideDetail.error) && <p role="alert">{sideError || snapshot.sideDetail.error}</p>}
        {(snapshot.sideDetail.rows as Row[]).map(row => {
          if (row.kind === "reasoning") return <details className="harness-reasoning" key={row.rowId}><summary>{row.durationMs ? `思考 · 持续了 ${Math.max(1, Math.ceil(row.durationMs / 1000))} 秒` : t("思考过程")}</summary><p>{row.text}</p></details>;
          if (row.kind === "toolCall") return <details className="harness-tool" key={row.rowId}><summary>{row.title || row.toolName} · {toolStatusLabels[row.status || ""] || row.status}</summary><pre>{row.inputText && toolDetails(row.inputText)}</pre>{typeof row.outputPreview?.text === "string" && <pre>{row.outputPreview.text}</pre>}<pre>{typeof row.output?.text === "string" ? row.output.text : row.text}</pre></details>;
          if (row.kind === "turnHeader") return row.state === "failed" || row.state === "completedInterrupted" ? <p className="harness-chat-info" key={row.rowId}>{row.state === "failed" ? "本轮执行失败" : "本轮已取消，部分内容保留"}</p> : null;
          if (!["assistantText", "userInput"].includes(row.kind)) return null;
          return <article className={`harness-message ${row.kind === "userInput" ? "user" : "assistant"}`} key={row.rowId}><strong>{row.kind === "userInput" ? "你" : "ZCode Desktop"}</strong>{row.text && <p>{row.kind === "userInput" ? messageContent(row.text) : row.text}</p>}{row.state === "streaming" && <small>{t("生成中…")}</small>}</article>;
        })}
        {!snapshot.sideDetail.loading && !(snapshot.sideDetail.rows as Row[]).length && <p className="harness-chat-info">{t("辅助对话已建立，输入后续修改要求。")}</p>}
      </div>
      <form className="zcode-side-composer" onSubmit={e => { e.preventDefault(); void sendSide(); }}>
        {quoteCards(sideQuotes, setSideQuotes)}
        <textarea ref={sideInput} aria-label={t("发送给辅助对话")} placeholder={t("提出后续修改要求…")} rows={2} value={sideDraft} onChange={e => setSideDraft(e.target.value)} onKeyDown={e => { if (e.nativeEvent.isComposing || e.keyCode === 229) return; if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void sendSide(); } }} disabled={!snapshot.connected} />
        <button className="harness-send-button" type="submit" aria-label={t("发送")} title={t("发送")} disabled={sideWorking || (!sideDraft.trim() && !sideQuotes.length) || !snapshot.connected}><ArrowUp size={20} /></button>
      </form>
    </aside>}
  </section>;
}

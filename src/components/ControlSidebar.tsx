import { FormEvent, KeyboardEvent, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { ArrowSquareInIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareIn";
import { ArrowSquareOutIcon } from "@phosphor-icons/react/dist/csr/ArrowSquareOut";
import { CaretDownIcon } from "@phosphor-icons/react/dist/csr/CaretDown";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { PaperPlaneTiltIcon } from "@phosphor-icons/react/dist/csr/PaperPlaneTilt";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { WarningCircleIcon } from "@phosphor-icons/react/dist/csr/WarningCircle";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { InterventionCenter } from "./InterventionCenter";
import { QuickCommandCenter } from "./QuickCommandCenter";
import type { QuickCommand, QuickContext } from "../quickCommands";
import { MultiAgentControl } from "./MultiAgentControl";
import {
  CONTROL_SURFACE_STORAGE_KEY,
  loadControlSurface,
  saveControlSurface,
  type ControlSurface,
} from "../controlSurface";
import type { ControlCommandResult } from "../controlCommandResult";
import { nextTrustPollDelay, sameTrustStatus } from "../controlTrustPolling";
import type {
  BroadcastReceipt,
  ControlHistoryMessage,
  ControlLaunchMode,
  ControlPlan,
  ControlWorkspaceTrustReceipt,
  ControlWorkspaceTrustStatus,
  PermissionMode,
  PermissionModeReceipt,
  ProviderConfig,
  SessionRecord,
} from "../types";

interface Props {
  quickContext?: QuickContext;
  onOpenQuickSettings?: () => void;
  open: boolean;
  active?: boolean;
  variant?: "docked" | "detached";
  providers: ProviderConfig[];
  sessions: SessionRecord[];
  defaultCwd: string | null;
  onClose: () => void;
  onDetach?: () => void | Promise<void>;
  onDock?: () => void | Promise<void>;
  onSessionsChange: (sessions: SessionRecord[]) => void;
  initialSurface?: ControlSurface;
  isolatedSurface?: boolean;
  onSurfaceChange?: (surface: ControlSurface) => void;
  onOpenMultiAgentWorkspace?: (projectId: string) => void;
}

interface ChatMessage extends ControlHistoryMessage {
  id: string;
  tone?: "normal" | "result" | "error";
}

const SUGGESTIONS = [
  "打开1个deepseek、1个glm、1个kimi",
  "打开1个glm、1个kimi",
  "把“检查当前项目并给出下一步建议”发送给所有会话",
];

const UNIFIED_MODES: Array<{ value: PermissionMode; label: string }> = [
  { value: "default", label: "Default" },
  { value: "acceptEdits", label: "Accept Edits" },
  { value: "plan", label: "Plan" },
  { value: "auto", label: "Auto" },
];
const CONTROL_MESSAGES_STORAGE_KEY = "crewvela-control-messages";
const CONTROL_DRAFT_STORAGE_KEY = "crewvela-control-draft";
const CONTROL_PROMPT_COMPLETION = "打开 1 个 GLM 和 2 个 Kimi 会话";
const TAURI_RUNTIME_AVAILABLE =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const EMPTY_TRUST_STATUS: ControlWorkspaceTrustStatus = {
  batchCount: 0,
  awaitingCount: 0,
  startingCount: 0,
  missingCount: 0,
};

function messageId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function controlLaunchModeLabel(mode: ControlLaunchMode): string {
  if (mode === "auto") return "Auto";
  if (mode === "yolo") return "YOLO";
  return "Default";
}

function initialMessage(): ChatMessage {
  return {
    id: "welcome",
    role: "assistant",
    content:
      "我是本地 Claude Code 总控。可以批量创建不同 Provider 的会话、广播 Prompt，也可以关闭或删除全部会话。",
  };
}

function loadMessages(): ChatMessage[] {
  try {
    const raw = window.localStorage.getItem(CONTROL_MESSAGES_STORAGE_KEY);
    if (!raw) return [initialMessage()];
    const parsed = JSON.parse(raw);
    if (
      Array.isArray(parsed) &&
      parsed.every(
        (item) =>
          typeof item?.id === "string" &&
          (item?.role === "user" || item?.role === "assistant") &&
          typeof item?.content === "string",
      )
    ) {
      return parsed;
    }
  } catch {
    // 存储损坏或不可用时回退到欢迎消息。
  }
  return [initialMessage()];
}

function loadDraft(): string {
  try {
    return window.localStorage.getItem(CONTROL_DRAFT_STORAGE_KEY) ?? "";
  } catch {
    return "";
  }
}

export function ControlSidebar({
  quickContext,
  onOpenQuickSettings,
  open,
  active,
  variant = "docked",
  providers,
  sessions,
  defaultCwd,
  onClose,
  onDetach,
  onDock,
  onSessionsChange,
  initialSurface,
  isolatedSurface = false,
  onSurfaceChange,
  onOpenMultiAgentWorkspace,
}: Props) {
  const isActive = active ?? open;
  const [quickCommand, setQuickCommand] = useState<Partial<QuickCommand> | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>(loadMessages);
  const [draft, setDraft] = useState(loadDraft);
  const [busy, setBusy] = useState(false);
  const commandInFlight = useRef(false);
  const modeInFlight = useRef(false);
  const trustInFlight = useRef(false);
  const [trustBusy, setTrustBusy] = useState(false);
  const [trustStatus, setTrustStatus] =
    useState<ControlWorkspaceTrustStatus>(EMPTY_TRUST_STATUS);
  const [modeBusy, setModeBusy] = useState(false);
  const [pendingMode, setPendingMode] = useState<PermissionMode | null>(null);
  const [composerFocused, setComposerFocused] = useState(false);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const [composerMenuOpen, setComposerMenuOpen] = useState(false);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [sendTarget, setSendTarget] = useState<"control" | "all">("control");
  const [controlSurface, setControlSurface] = useState<ControlSurface>(
    () => initialSurface ?? loadControlSurface(),
  );
  const [interventionCenterOpen, setInterventionCenterOpen] = useState(false);
  const [interventionCount, setInterventionCount] = useState(0);
  const [focusedCollaborationProject, setFocusedCollaborationProject] = useState<{
    id: string;
    request: number;
  } | null>(null);
  const messagesRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const liveSessions = sessions.filter(
    (session) => session.kind === "claude" && session.ptyId !== null,
  );
  const liveCount = liveSessions.length;
  const knownModeCount = liveSessions.filter(
    (session) => session.permissionMode !== null,
  ).length;
  const allModesKnown = knownModeCount === liveCount;
  const allModesSwitchable = liveSessions.every(
    (session) => session.permissionMode !== "dontAsk",
  );
  const firstMode = liveSessions[0]?.permissionMode ?? null;
  const uniformMode =
    firstMode !== null &&
    liveSessions.every((session) => session.permissionMode === firstMode) &&
    UNIFIED_MODES.some((mode) => mode.value === firstMode)
      ? firstMode
      : "mixed";
  const modeValue = pendingMode ?? uniformMode;
  const completionSuffix = CONTROL_PROMPT_COMPLETION.startsWith(draft)
    ? CONTROL_PROMPT_COMPLETION.slice(draft.length)
    : "";
  const canCompletePrompt =
    composerFocused &&
    draft.length > 0 &&
    !busy &&
    !trustBusy &&
    completionSuffix.length > 0;
  const canConfirmWorkspaceTrust =
    draft.trim().length === 0 &&
    trustStatus.awaitingCount > 0 &&
    !busy &&
    !trustBusy;

  useEffect(() => {
    if (initialSurface) setControlSurface(initialSurface);
  }, [initialSurface]);

  useEffect(() => {
    if (!isolatedSurface) saveControlSurface(controlSurface);
    onSurfaceChange?.(controlSurface);
  }, [controlSurface, onSurfaceChange, isolatedSurface]);

  useEffect(() => {
    if (isolatedSurface) return;
    const syncSurface = (event: StorageEvent) => {
      if (event.key === CONTROL_SURFACE_STORAGE_KEY) {
        setControlSurface(event.newValue === "multi_agent" ? "multi_agent" : "quick");
      }
    };
    window.addEventListener("storage", syncSurface);
    return () => window.removeEventListener("storage", syncSurface);
  }, [isolatedSurface]);

  useEffect(() => {
    if (pendingMode === null) return;
    if (liveCount === 0 || !allModesKnown || uniformMode === pendingMode) {
      setPendingMode(null);
    }
  }, [allModesKnown, liveCount, pendingMode, uniformMode]);

  useEffect(() => {
    if (!isActive) return;

    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = dialogRef.current;
    const frame = window.requestAnimationFrame(() => {
      const initialFocus =
        dialog?.querySelector<HTMLElement>(".multi-agent-composer textarea:not([disabled])") ??
        dialog?.querySelector<HTMLElement>(".control-close") ??
        dialog?.querySelector<HTMLElement>("button:not([disabled])") ??
        dialog;
      initialFocus?.focus();
    });
    const handleDialogKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (interventionCenterOpen) {
          setInterventionCenterOpen(false);
        } else if (clearConfirmOpen) {
          setClearConfirmOpen(false);
        } else if (modeMenuOpen) {
          setModeMenuOpen(false);
        } else if (composerMenuOpen) {
          setComposerMenuOpen(false);
        } else {
          onClose();
        }
      }
    };

    document.addEventListener("keydown", handleDialogKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handleDialogKeyDown);
      previousFocus?.focus();
    };
  }, [clearConfirmOpen, composerMenuOpen, interventionCenterOpen, isActive, modeMenuOpen, onClose]);

  useEffect(() => {
    try {
      window.localStorage.setItem(
        CONTROL_MESSAGES_STORAGE_KEY,
        JSON.stringify(messages),
      );
    } catch {
      // 持久化不可用时仍保留当前窗口内的状态。
    }
  }, [messages]);

  useEffect(() => {
    try {
      window.localStorage.setItem(CONTROL_DRAFT_STORAGE_KEY, draft);
    } catch {
      // 持久化不可用时仍保留当前窗口内的状态。
    }
  }, [draft]);

  useEffect(() => {
    if (!isActive) return;
    const container = messagesRef.current;
    if (container) container.scrollTop = container.scrollHeight;
  }, [messages, busy, isActive]);

  useEffect(() => {
    if (!isActive || !TAURI_RUNTIME_AVAILABLE) return;
    let stopped = false;
    let timer: number | undefined;
    // 串行自调度：上一次读取未返回不会再发起；窗口隐藏时不读取；
    // 有会话启动/等待信任确认时 250ms，其余时间 1s；内容未变不触发整个侧栏重渲染。
    const refresh = async () => {
      let status: ControlWorkspaceTrustStatus | null = null;
      if (!document.hidden) {
        try {
          status = await invoke<ControlWorkspaceTrustStatus>(
            "control_workspace_trust_status",
          );
          const latest = status;
          if (!stopped) {
            setTrustStatus((current) =>
              sameTrustStatus(current, latest) ? current : latest,
            );
          }
        } catch (error) {
          console.error("读取总控工作区信任状态失败:", error);
        }
      }
      if (!stopped) timer = window.setTimeout(refresh, nextTrustPollDelay(status));
    };
    void refresh();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [isActive]);

  const appendAssistant = (
    content: string,
    tone: ChatMessage["tone"] = "normal",
  ) => {
    setMessages((items) => [
      ...items,
      { id: messageId(), role: "assistant", content, tone },
    ]);
  };

  const clearConversation = () => {
    if (busy || modeBusy || commandInFlight.current || modeInFlight.current) return;
    setClearConfirmOpen(false);
    setMessages([]);
    setDraft("");
    try {
      window.localStorage.removeItem(CONTROL_MESSAGES_STORAGE_KEY);
      window.localStorage.removeItem(CONTROL_DRAFT_STORAGE_KEY);
    } catch {
      // 本次窗口内仍会清空会话；持久化不可用时无需额外提示。
    }
  };

  const refreshSessions = async () => {
    const snapshot = await invoke<SessionRecord[]>("list_sessions");
    onSessionsChange(snapshot);
  };

  const executePlan = async (plan: ControlPlan): Promise<string> => {
    if (plan.intent === "launch") {
      const outcomes = await Promise.allSettled(
        plan.launches.map(async (launch) => {
          const provider = providers.find((item) => item.id === launch.providerId);
          if (!provider) throw new Error(`未找到 Provider：${launch.providerId}`);
          const cwds = defaultCwd
            ? Array.from({ length: launch.count }, () => defaultCwd)
            : [];
          const launched = await invoke<SessionRecord[]>("launch_multi", {
            providerId: launch.providerId,
            cwds,
            count: launch.count,
            yolo: launch.mode === "yolo",
            autoMode: launch.mode === "auto",
            useWorktree: false,
            initialPrompt: null,
          });
          return {
            provider: provider.name || provider.id,
            count: launched.length,
            mode: launch.mode,
            sessionIds: launched.map((session) => session.id),
          };
        }),
      );
      const launchedSessionIds = outcomes.flatMap((outcome) =>
        outcome.status === "fulfilled" ? outcome.value.sessionIds : [],
      );
      const registeredStatus = await invoke<ControlWorkspaceTrustStatus>(
        "register_control_launch_batch",
        { sessionIds: launchedSessionIds },
      );
      setTrustStatus(registeredStatus);
      await refreshSessions();
      const succeeded = outcomes
        .filter(
          (
            outcome,
          ): outcome is PromiseFulfilledResult<{
            provider: string;
            count: number;
            mode: ControlLaunchMode;
            sessionIds: string[];
          }> =>
            outcome.status === "fulfilled",
        )
        .map(
          (outcome) =>
            `${outcome.value.provider} ×${outcome.value.count}（${controlLaunchModeLabel(outcome.value.mode)}）`,
        );
      const failed = outcomes
        .filter(
          (outcome): outcome is PromiseRejectedResult => outcome.status === "rejected",
        )
        .map((outcome) => String(outcome.reason));
      const result = succeeded.length > 0 ? `已创建：${succeeded.join("、")}。` : "";
      if (failed.length === 0) return result || "没有创建任何会话。";
      return `${result}${result ? "\n" : ""}未完成：${failed.join("；")}`;
    }

    if (plan.intent === "broadcast" && plan.prompt) {
      setQuickCommand({ action: "send", body: plan.prompt, scope: "all" });
      return "请在快捷指令中选择具体会话，核对后确认发送；尚未排队。";
    }
    if (plan.intent === "stop_all" || plan.intent === "delete_all") {
      const action = plan.intent === "stop_all" ? "stop" : "delete";
      setQuickCommand({ id: `builtin:${action}`, title: action === "stop" ? "批量停止会话" : "批量从列表删除会话", action, scope: "all", body: "" });
      return "请在快捷指令中核对目标并二次确认；尚未执行批量操作。";
    }

    return "";
  };

  const runUserAction = async (
    message: string,
    action: (content: string) => Promise<{
      content: string;
      tone: ChatMessage["tone"];
    }>,
    errorLabel: string,
  ) => {
    if (!message.trim() || busy || commandInFlight.current) return;
    commandInFlight.current = true;
    const userMessage: ChatMessage = {
      id: messageId(),
      role: "user",
      content: message,
    };
    setMessages((items) => [...items, userMessage]);
    setDraft("");
    setBusy(true);
    try {
      const result = await action(message);
      appendAssistant(result.content, result.tone);
    } catch (error) {
      appendAssistant(`${errorLabel}：${String(error)}`, "error");
    } finally {
      commandInFlight.current = false;
      setBusy(false);
    }
  };

  const submit = async (content: string) => {
    const message = content.trim();
    if (!message || busy) return;
    if (message.toLowerCase() === "/clear") {
      clearConversation();
      return;
    }
    const history = messages
      .filter((item) => item.tone !== "error")
      .slice(-8)
      .map(({ role, content: historyContent }) => ({
        role,
        content: historyContent,
      }));
    await runUserAction(
      message,
      async () => {
        const plan = await invoke<ControlPlan>("interpret_control_message", {
          message,
          history,
        });
        const result = await executePlan(plan);
        return {
          content: [plan.reply, result].filter(Boolean).join("\n"),
          tone: result.includes("未完成")
            ? "error"
            : result
              ? "result"
              : "normal",
        };
      },
      "执行失败",
    );
  };

  const executeQuickCommand = async (
    content: string,
  ): Promise<ControlCommandResult> => {
    const message = content.trim();
    if (!message || busy || commandInFlight.current) {
      return { ok: false, reply: "快速指挥正在忙，请稍后再试。" };
    }
    commandInFlight.current = true;
    const history = messages
      .filter((item) => item.tone !== "error")
      .slice(-8)
      .map(({ role, content: historyContent }) => ({
        role,
        content: historyContent,
      }));
    setMessages((items) => [
      ...items,
      { id: messageId(), role: "user", content: message },
    ]);
    setBusy(true);
    try {
      const plan = await invoke<ControlPlan>("interpret_control_message", {
        message,
        history,
      });
      const execution = await executePlan(plan);
      const reply = [plan.reply, execution].filter(Boolean).join("\n");
      setMessages((items) => [
        ...items,
        {
          id: messageId(),
          role: "assistant",
          content: reply,
          tone: execution.includes("未完成")
            ? "error"
            : execution
              ? "result"
              : "normal",
        },
      ]);
      return { ok: !execution.includes("未完成"), reply };
    } catch (reason) {
      const reply = `执行失败：${String(reason)}`;
      setMessages((items) => [
        ...items,
        { id: messageId(), role: "assistant", content: reply, tone: "error" },
      ]);
      return { ok: false, reply };
    } finally {
      commandInFlight.current = false;
      setBusy(false);
    }
  };

  const broadcast = async (content: string) => {
    if (content.trim()) setQuickCommand({ action: "send", body: content.trim(), scope: "all" });
  };

  const executeQuickBroadcast = async (content: string): Promise<ControlCommandResult> => {
    if (!content.trim()) return { ok: false, reply: "请填写消息内容。" };
    setQuickCommand({ action: "send", body: content.trim(), scope: "all" });
    return { ok: true, reply: "请在快捷指令中核对目标后确认发送；尚未排队。" };
  };

  const changePermissionMode = async (
    permissionMode: PermissionMode,
  ): Promise<ControlCommandResult> => {
    if (
      modeBusy || modeInFlight.current ||
      liveCount === 0 ||
      !allModesKnown ||
      !allModesSwitchable
    ) {
      return { ok: false, reply: "当前会话暂时不能统一切换权限模式。" };
    }
    modeInFlight.current = true;
    setPendingMode(permissionMode);
    setModeBusy(true);
    try {
      const receipt = await invoke<PermissionModeReceipt>(
        "set_all_permission_modes",
        { permissionMode },
      );
      await refreshSessions();
      const reply = `已请求将 ${receipt.targetCount} 个运行会话统一切换为 ${modeLabel(receipt.targetMode)}：${receipt.alreadyCount} 个已处于目标模式，${receipt.sentCount} 个已开始切换，剩余步骤会在后台完成；完成后可直接再次选择模式，无需发送消息。后续 Claude Code Hook 会自动纠正实际状态。`;
      appendAssistant(reply, "result");
      return { ok: true, reply };
    } catch (error) {
      setPendingMode(null);
      const reply = `统一模式切换失败：${String(error)}`;
      appendAssistant(reply, "error");
      return { ok: false, reply };
    } finally {
      modeInFlight.current = false;
      setModeBusy(false);
    }
  };

  const confirmWorkspaceTrust = async () => {
    if (
      !canConfirmWorkspaceTrust ||
      trustStatus.awaitingCount === 0 ||
      trustBusy || trustInFlight.current
    ) {
      return;
    }
    trustInFlight.current = true;
    setTrustBusy(true);
    try {
      const receipt = await invoke<ControlWorkspaceTrustReceipt>(
        "confirm_control_workspace_trust",
      );
      if (receipt.failedSessionIds.length > 0) {
        appendAssistant(
          `已统一确认 ${receipt.confirmedCount} 个会话的工作区信任，${receipt.failedSessionIds.length} 个未完成。`,
          "error",
        );
      } else {
        appendAssistant(
          `已统一确认 ${receipt.confirmedCount} 个会话的工作区信任。`,
          "result",
        );
      }
      const status = await invoke<ControlWorkspaceTrustStatus>(
        "control_workspace_trust_status",
      );
      setTrustStatus(status);
    } catch (error) {
      appendAssistant(`统一确认工作区信任失败：${String(error)}`, "error");
    } finally {
      trustInFlight.current = false;
      setTrustBusy(false);
    }
  };

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (sendTarget === "all") {
      broadcast(draft);
      return;
    }
    submit(draft);
  };

  const applyPromptCompletion = () => {
    if (!canCompletePrompt) return;
    setDraft(CONTROL_PROMPT_COMPLETION);
    window.requestAnimationFrame(() => {
      const textarea = composerRef.current;
      textarea?.focus();
      textarea?.setSelectionRange(
        CONTROL_PROMPT_COMPLETION.length,
        CONTROL_PROMPT_COMPLETION.length,
      );
    });
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      event.key === "Tab" &&
      !event.shiftKey &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey &&
      !event.nativeEvent.isComposing &&
      canCompletePrompt
    ) {
      event.preventDefault();
      applyPromptCompletion();
      return;
    }
    if (
      event.key === "Enter" &&
      event.shiftKey &&
      (event.ctrlKey || event.metaKey)
    ) {
      event.preventDefault();
      broadcast(draft);
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (canConfirmWorkspaceTrust) {
        confirmWorkspaceTrust();
        return;
      }
      if (sendTarget === "all") {
        broadcast(draft);
      } else {
        submit(draft);
      }
    }
  };

  if (!open) return null;

  return (
    <div className={`control-overlay control-${variant}`}>
      <aside
        ref={dialogRef}
        className="control-sidebar"
        role="complementary"
        aria-label="总控会话"
        tabIndex={-1}
      >
        <header className="control-header">
          <div className="control-header-right">
            <div className="control-header-actions">
              <button
                className={`control-intervention-button${interventionCount > 0 ? " has-items" : ""}`}
                type="button"
                aria-label={`待处理中心，${interventionCount} 项`}
                title="待处理中心"
                aria-expanded={interventionCenterOpen}
                onClick={() => setInterventionCenterOpen((visible) => !visible)}
              >
                <WarningCircleIcon size={17} />
                {interventionCount > 0 && (
                  <span>{Math.min(interventionCount, 99)}</span>
                )}
              </button>
            {variant === "docked" && onDetach && (
              <button
                className="control-detach-button"
                aria-label="分离总控窗口"
                title="分离为独立窗口"
                disabled={busy || modeBusy}
                onClick={() => { saveControlSurface(controlSurface); void onDetach(); }}
              >
                <ArrowSquareOutIcon size={16} />
              </button>
            )}
            {variant === "detached" && onDock && (
              <button
                className="control-dock-button"
                aria-label="重新停靠总控窗口"
                title="重新停靠"
                disabled={busy || modeBusy}
                onClick={onDock}
              >
                <ArrowSquareInIcon size={16} />
              </button>
            )}
            <button
              className="btn-icon control-close"
              aria-label="关闭总控会话"
              title="关闭总控会话"
              disabled={busy || modeBusy}
              onClick={onClose}
            >
              <XIcon size={18} />
            </button>
            </div>
          </div>
        </header>

        <InterventionCenter
          open={interventionCenterOpen}
          onClose={() => setInterventionCenterOpen(false)}
          onCountChange={setInterventionCount}
          onOpenProject={(projectId) => {
            setFocusedCollaborationProject((current) => ({
              id: projectId,
              request: (current?.request ?? 0) + 1,
            }));
          }}
        />

        <div className="control-multi-agent-surface">
          <MultiAgentControl
            sessions={sessions}
            active={isActive}
            defaultCwd={defaultCwd}
            onOpenWorkspace={onOpenMultiAgentWorkspace}
            focusedProjectId={focusedCollaborationProject?.id}
            focusedProjectRequest={focusedCollaborationProject?.request}
            onOpenQuickCommands={() => setQuickCommand({})}
            onQuickCommand={executeQuickCommand}
            onQuickBroadcast={executeQuickBroadcast}
            onQuickPermissionModeChange={changePermissionMode}
            onClearQuickHistory={async () => {
              clearConversation();
              return { ok: true, reply: "已清除对话记录。" };
            }}
            quickLiveSessionCount={liveCount}
            quickKnownModeCount={knownModeCount}
            quickPermissionMode={modeValue}
            quickModesKnown={allModesKnown}
            quickModesSwitchable={allModesSwitchable}
            quickPermissionModeBusy={modeBusy}
            workspaceTrustStatus={trustStatus}
            workspaceTrustBusy={trustBusy}
            onConfirmWorkspaceTrust={() => confirmWorkspaceTrust()}
          />
        </div>
      </aside>
      {quickCommand !== null && <QuickCommandCenter sessions={sessions} providers={providers} context={quickContext} initialCommand={quickCommand} onClose={() => setQuickCommand(null)} onSessionsChange={onSessionsChange} onOpenSettings={onOpenQuickSettings ? () => { setQuickCommand(null); onOpenQuickSettings(); } : undefined} />}
    </div>
  );
}

function modeLabel(mode: PermissionMode): string {
  return UNIFIED_MODES.find((item) => item.value === mode)?.label ?? mode;
}

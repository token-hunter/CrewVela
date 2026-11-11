import { useRef, useState } from "react";
import { DotsSixVertical } from "@phosphor-icons/react";
import { RUNTIME_LABELS, RUNTIME_LOGOS } from "../runtimeLogos";
import { runtimeCategory } from "../runtimeTaxonomy";
import { useRuntimeAvailability } from "../runtimeAvailability";
import type { AgentRuntimeKind, Project, ProviderConfig } from "../types";
import { PROVIDER_LOGOS, providerTone } from "../providerLogos";
import {
  NEW_SESSION_CHAT_ORDER_STORAGE_KEY,
  NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY,
  useRuntimeOrder,
} from "../runtimePreferences";
import { Modal } from "./ui/Modal";

const NEW_SESSION_AGENTS = [
  "claude_code",
  "codex",
  "kimi_code",
  "pi",
  "opencode",
  "deepseek_harness",
  "claude_chat",
  "codex_chat",
  "kimi_chat",
  "opencode_chat",
  "pi_chat",
  "zcode",
  "zcode_cli",
] as const satisfies readonly AgentRuntimeKind[];
type NewSessionAgent = (typeof NEW_SESSION_AGENTS)[number];
const TERMINAL_AGENTS = NEW_SESSION_AGENTS.filter(
  (runtime) => runtimeCategory(runtime) === "terminal",
);
const CHAT_AGENTS = NEW_SESSION_AGENTS.filter(
  (runtime) => runtimeCategory(runtime) === "chat",
);

interface Props {
  project?: Project | null;
  onRuntimeSelect?: (runtime: Exclude<AgentRuntimeKind, "claude_code" | "oh_my_pi" | "minimax_code" | "unsupported">) => void;
  providers: ProviderConfig[];
  keyStates: Record<string, boolean>;
  onSelect: (providerId: string) => void;
  onLocalClaudeSelect: () => void;
  onOpenSettings: () => void;
  onClose: () => void;
}

export function NewSessionDialog({
  project,
  onRuntimeSelect,
  providers,
  keyStates,
  onSelect,
  onLocalClaudeSelect,
  onOpenSettings,
  onClose,
}: Props) {
  const { isRuntimeEnabled, ready, error } = useRuntimeAvailability();
  const [chooseAgent, setChooseAgent] = useState(true);
  const [draggedRuntime, setDraggedRuntime] = useState<NewSessionAgent | null>(null);
  const [dropTarget, setDropTarget] = useState<{
    category: "terminal" | "chat";
    runtime: NewSessionAgent;
    position: "before" | "after";
  } | null>(null);
  const activeDragRef = useRef<{
    category: "terminal" | "chat";
    runtime: NewSessionAgent;
  } | null>(null);
  const suppressClickUntilRef = useRef(0);
  const terminalOrder = useRuntimeOrder(
    TERMINAL_AGENTS,
    NEW_SESSION_TERMINAL_ORDER_STORAGE_KEY,
  );
  const chatOrder = useRuntimeOrder(
    CHAT_AGENTS,
    NEW_SESSION_CHAT_ORDER_STORAGE_KEY,
  );
  // 四个原生聊天(claude/codex/kimi/pi chat)各持独立启停开关。
  const agents = NEW_SESSION_AGENTS.filter((runtime) => isRuntimeEnabled(runtime));
  const groups = [
    {
      category: "terminal" as const,
      title: "CLI 终端 Agent",
      order: terminalOrder.order,
      error: terminalOrder.error,
    },
    {
      category: "chat" as const,
      title: "原生聊天 Agent",
      order: chatOrder.order,
      error: chatOrder.error,
    },
  ];

  const selectRuntime = (runtime: NewSessionAgent) => {
    if (Date.now() < suppressClickUntilRef.current) return;
    if (!isRuntimeEnabled(runtime)) return;
    if (runtime === "claude_code") setChooseAgent(false);
    else onRuntimeSelect?.(runtime);
  };

  const resetAgentDrag = () => {
    activeDragRef.current = null;
    setDraggedRuntime(null);
    setDropTarget(null);
    suppressClickUntilRef.current = Date.now() + 250;
  };
  return (
    <Modal
      overlayClassName="dialog-overlay"
      className="new-session-dialog"
      onClose={onClose}
      closeOnOverlayClick
      labelledBy="new-session-title"
    >
        <div className="dialog-heading">
          <div>
            <span className="eyebrow">NEW TASK</span>
            <h2 id="new-session-title">{chooseAgent ? "选择 Agent" : "选择 Provider"}</h2>
            <p>{chooseAgent ? (project ? `在项目「${project.name}」中创建会话，使用项目工作目录。` : "选择本次会话使用的 Agent，下一步选择工作目录。") : "选择模型服务或本地配置，继续创建 CLI 会话。"}</p>
          </div>
          <button className="dialog-close" aria-label="关闭" onClick={onClose}>
            ×
          </button>
        </div>

        {chooseAgent ? <div className="provider-picker project-agent-picker">
          {!ready && <p role="status">{error ? `Agent 配置加载失败：${error}` : "正在加载 Agent 配置…"}</p>}
          {ready && agents.length === 0 && <p role="status">没有已启用的独立 Agent，请先在设置中启用。</p>}
          {groups.map(({ category, title, order, error: orderError }) => {
            const orderIndex = new Map(order.map((runtime, index) => [runtime, index]));
            const groupAgents = agents
              .filter((runtime) => runtimeCategory(runtime) === category)
              .sort((left, right) => (orderIndex.get(left) ?? 0) - (orderIndex.get(right) ?? 0));
            if (!groupAgents.length) return null;
            return <section
              key={category}
              className="new-session-agent-group"
              onDragOver={(event) => {
                const source = activeDragRef.current;
                if (!source) return;
                event.preventDefault();
                event.dataTransfer.dropEffect = source.category === category ? "move" : "none";
              }}
              onDrop={(event) => {
                // 空白处和组边界接收内部拖拽，避免浏览器默认打开文本，但不改变顺序。
                event.preventDefault();
                setDropTarget(null);
              }}
            >
              <h4 className="new-session-group-label">{title}</h4>
              {orderError && <p role="alert" className="new-session-order-error">{orderError}</p>}
              <div className="new-session-agent-grid">
                {groupAgents.map((runtime) => {
                  const targetPosition =
                    dropTarget?.category === category && dropTarget.runtime === runtime
                      ? dropTarget.position
                      : undefined;
                  return <div
                    key={runtime}
                    role="button"
                    tabIndex={0}
                    aria-roledescription="可拖动 Agent"
                    className={`provider-choice new-session-agent-choice${draggedRuntime === runtime ? " dragging" : ""}`}
                    data-drop-position={targetPosition}
                    data-new-session-agent={runtime}
                    draggable
                    title="拖动卡片调整本组顺序，单击选择 Agent"
                    onClick={() => selectRuntime(runtime)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        selectRuntime(runtime);
                      }
                    }}
                    onDragStart={(event) => {
                      activeDragRef.current = { runtime, category };
                      suppressClickUntilRef.current = Date.now() + 250;
                      setDraggedRuntime(runtime);
                      setDropTarget(null);
                      event.dataTransfer.effectAllowed = "move";
                      event.dataTransfer.setData("text/plain", runtime);
                    }}
                    onDragEnd={resetAgentDrag}
                    onDragOver={(event) => {
                      const source = activeDragRef.current;
                      if (!source) return;
                      event.preventDefault();
                      if (source.category !== category) {
                        event.dataTransfer.dropEffect = "none";
                        setDropTarget(null);
                        return;
                      }
                      event.dataTransfer.dropEffect = "move";
                      const bounds = event.currentTarget.getBoundingClientRect();
                      setDropTarget({
                        category,
                        runtime,
                        position:
                          event.clientX < bounds.left + bounds.width / 2 ? "before" : "after",
                      });
                    }}
                    onDragLeave={(event) => {
                      const relatedTarget = event.relatedTarget;
                      if (
                        !(relatedTarget instanceof Node) ||
                        !event.currentTarget.contains(relatedTarget)
                      ) {
                        setDropTarget((current) =>
                          current?.runtime === runtime ? null : current,
                        );
                      }
                    }}
                    onDrop={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      const source = activeDragRef.current;
                      if (!source || source.category !== category) return;
                      const bounds = event.currentTarget.getBoundingClientRect();
                      const position =
                        event.clientX < bounds.left + bounds.width / 2 ? "before" : "after";
                      if (category === "terminal") {
                        terminalOrder.reorder(source.runtime, runtime, position);
                      } else {
                        chatOrder.reorder(source.runtime, runtime, position);
                      }
                    }}
                  >
                    <span className="new-session-agent-grip" aria-hidden="true">
                      <DotsSixVertical size={16} weight="bold" aria-hidden="true" />
                    </span>
                    <span className="provider-choice-icon has-logo"><img src={RUNTIME_LOGOS[runtime]} data-runtime={runtime} alt="" aria-hidden="true" /></span>
                    <span className="provider-choice-copy"><strong>{RUNTIME_LABELS[runtime]}</strong><small>{runtime === "claude_code" ? "下一步选择 Provider、数量和目录" : runtime === "claude_chat" ? "本机 Claude Code CLI 账号 · ACP 原生聊天" : runtime === "codex_chat" ? "本机 Codex CLI 账号 · ACP 原生聊天" : runtime === "kimi_chat" ? "本机 Kimi Code CLI 账号 · ACP 原生聊天" : runtime === "opencode_chat" ? "本机 OpenCode CLI 账号 · ACP 原生聊天" : runtime === "pi_chat" ? "本机 pi CLI 账号 · 原生 RPC 聊天" : project ? "使用项目目录创建会话" : "选择工作目录创建会话"}</small></span>
                    <span className="provider-choice-arrow" aria-hidden="true">→</span>
                  </div>;
                })}
              </div>
            </section>;
          })}
        </div> : <div className="provider-picker">
          {!ready && <p role="status">{error ? `Agent 配置加载失败：${error}` : "正在加载 Agent 配置…"}</p>}
          {ready && !isRuntimeEnabled("claude_code") && <p role="status">Claude Code 未启用，请先在设置中启用。</p>}

          {isRuntimeEnabled("claude_code") && <button
            type="button"
            className="provider-choice local-claude-choice"
            onClick={onLocalClaudeSelect}
          >
            <span className="provider-choice-icon has-logo"><img src={RUNTIME_LOGOS.claude_code} alt="" aria-hidden="true" /></span>
            <span className="provider-choice-copy">
              <strong>使用本地 Claude Code 配置</strong>
              <small>沿用本机账号、服务地址和默认模型</small>
            </span>
            <span className="provider-choice-arrow" aria-hidden="true">→</span>
          </button>}

          {providers.filter(() => isRuntimeEnabled("claude_code")).map((provider) => {
            const hasKey = keyStates[provider.id] ?? false;
            const ready = hasKey && provider.baseUrl.length > 0;
            const tone = providerTone(provider);
            const logo = PROVIDER_LOGOS[tone];
            return (
              <button
                key={provider.id}
                className="provider-choice"
                onClick={() => onSelect(provider.id)}
              >
                <span
                  className={`provider-choice-icon${logo ? " has-logo" : ""}`}
                  data-provider-tone={tone}
                >
                  {logo ? (
                    <img src={logo} alt="" aria-hidden="true" />
                  ) : (
                    (provider.name || provider.id).slice(0, 1).toUpperCase()
                  )}
                </span>
                <span className="provider-choice-copy">
                  <strong>{provider.name || provider.id}</strong>
                  <small>{provider.baseUrl || "尚未配置 API 端点"}</small>
                </span>
                <span className={`readiness ${ready ? "ready" : "needs-config"}`}>
                  {ready ? "可用" : "需配置"}
                </span>
                <span className="provider-choice-arrow" aria-hidden="true">
                  →
                </span>
              </button>
            );
          })}
        </div>}

        <div className="dialog-footer">
          <span>{chooseAgent ? "Agent 启用状态可在设置中管理" : "Provider 配置与密钥可在设置中管理"}</span>
          {!chooseAgent && <button className="btn" onClick={() => setChooseAgent(true)}>返回选择 Agent</button>}
          <button className="btn" onClick={onOpenSettings}>
            打开设置
          </button>
        </div>
    </Modal>
  );
}

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { EmptyState } from "./ui/EmptyState";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import type {
  ApprovalCard,
  ApprovalDecision,
  ApprovalPolicy,
  InterventionAction,
  InterventionRecord,
  InterventionResolutionReceipt,
} from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
  onCountChange: (count: number) => void;
  onOpenProject: (projectId: string) => void;
}

const TAURI_RUNTIME_AVAILABLE =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** 卡片超时：等待超过该时长只做超时提示，不自动代答（自动代答走 Allow/Deny 策略）。 */
const CARD_EXPIRY_MS = 10 * 60 * 1000;

const POLICY_LABELS: Record<ApprovalPolicy, string> = {
  ask: "每次询问",
  allow: "总是允许",
  deny: "总是拒绝",
};

const KIND_LABELS: Record<InterventionRecord["kind"], string> = {
  approval_required: "待批准",
  agent_input_required: "等待回复",
  retry_exhausted: "重试耗尽",
  dependency_blocked: "依赖阻塞",
  semantic_review: "语义复核",
};

const PRIMARY_ACTION_LABELS: Record<
  InterventionRecord["recommendedAction"],
  string
> = {
  approve: "批准并继续",
  respond: "回复并继续",
  retry: "重试并继续",
  open_project: "打开项目",
  review: "查看并处理",
};

function formatWaitingAge(waitingSince: number): string {
  const minutes = Math.max(0, Math.floor((Date.now() - waitingSince) / 60_000));
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `等待 ${minutes} 分钟`;
  return `等待 ${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分钟`;
}

export function InterventionCenter({
  open,
  onClose,
  onCountChange,
  onOpenProject,
}: Props) {
  const [items, setItems] = useState<InterventionRecord[]>([]);
  const [cards, setCards] = useState<ApprovalCard[]>([]);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [busyCardId, setBusyCardId] = useState<string | null>(null);
  const [replyDrafts, setReplyDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const refreshSequence = useRef(0);
  const actionInFlight = useRef(false);

  const refresh = useCallback(async () => {
    const sequence = ++refreshSequence.current;
    if (!TAURI_RUNTIME_AVAILABLE) {
      setItems([]);
      setCards([]);
      onCountChange(0);
      return;
    }
    try {
      const [nextCards, nextItems] = await Promise.all([
        invoke<ApprovalCard[]>("list_approval_cards"),
        invoke<InterventionRecord[]>("ma_list_interventions"),
      ]);
      if (sequence !== refreshSequence.current) return;
      setCards(nextCards);
      setItems(nextItems);
      onCountChange(nextCards.length + nextItems.length);
      setError(null);
    } catch (reason) {
      if (sequence === refreshSequence.current) setError(String(reason));
    }
  }, [onCountChange]);

  useEffect(() => {
    void refresh();
    if (!TAURI_RUNTIME_AVAILABLE) return;
    const timer = window.setInterval(() => void refresh(), open ? 1_000 : 2_500);
    return () => { window.clearInterval(timer); refreshSequence.current += 1; };
  }, [open, refresh]);

  const resolve = async (
    item: InterventionRecord,
    action: InterventionAction,
    response?: string,
  ) => {
    if (actionInFlight.current) return;
    actionInFlight.current = true;
    setBusyId(item.id);
    setActionError(null);
    try {
      await invoke<InterventionResolutionReceipt>("ma_resolve_intervention", {
        input: {
          interventionId: item.id,
          action,
          response: response ?? null,
          agentId: null,
          snoozeMinutes: action === "snooze" ? 60 : null,
        },
      });
      setReplyDrafts((current) => {
        const next = { ...current };
        delete next[item.id];
        return next;
      });
      await refresh();
    } catch (reason) {
      setActionError(String(reason));
    } finally {
      actionInFlight.current = false;
      setBusyId(null);
    }
  };

  const runPrimaryAction = (item: InterventionRecord) => {
    if (
      item.recommendedAction === "open_project" ||
      item.recommendedAction === "review"
    ) {
      onOpenProject(item.projectId);
      onClose();
      return;
    }
    if (item.recommendedAction === "respond") {
      const response = replyDrafts[item.id]?.trim();
      if (!response) return;
      void resolve(item, "respond", response);
      return;
    }
    void resolve(item, item.recommendedAction);
  };

  const respondCard = async (card: ApprovalCard, action: ApprovalDecision) => {
    if (actionInFlight.current) return;
    actionInFlight.current = true;
    setBusyCardId(card.sessionId);
    setActionError(null);
    try {
      await invoke("respond_session_approval", {
        sessionId: card.sessionId,
        expectedPtyId: card.ptyId,
        expectedWaitingSince: card.waitingSince,
        action,
      });
      await refresh();
    } catch (reason) {
      setActionError(String(reason));
      await refresh();
    } finally {
      actionInFlight.current = false;
      setBusyCardId(null);
    }
  };

  const setCardPolicy = async (card: ApprovalCard, policy: ApprovalPolicy) => {
    if (actionInFlight.current) return;
    actionInFlight.current = true;
    setBusyCardId(card.sessionId);
    setActionError(null);
    try {
      await invoke("set_session_approval_policy", {
        sessionId: card.sessionId,
        policy,
      });
      await refresh();
    } catch (reason) {
      setActionError(String(reason));
    } finally {
      actionInFlight.current = false;
      setBusyCardId(null);
    }
  };

  const grouped = useMemo(() => {
    const result = new Map<
      string,
      { projectName: string; items: InterventionRecord[] }
    >();
    for (const item of items) {
      const group = result.get(item.projectId) ?? {
        projectName: item.projectName,
        items: [],
      };
      group.items.push(item);
      result.set(item.projectId, group);
    }
    return [...result.entries()];
  }, [items]);

  if (!open) return null;

  return (
    <section className="intervention-center" aria-label="统一人工干预中心">
      <header>
        <div>
          <strong>待处理</strong>
          <span>{cards.length + items.length} 项需要决定</span>
        </div>
        <button type="button" aria-label="关闭待处理中心" onClick={onClose}>
          <XIcon size={17} />
        </button>
      </header>

      {error && <p className="intervention-center-error">{error}</p>}
      {actionError && <p className="intervention-center-error" role="alert">{actionError}</p>}
      <div className="intervention-center-list">
        {cards.length > 0 && (
          <section className="intervention-project-group approval-group">
            <header>
              <strong>会话审批</strong>
              <span>{cards.length}</span>
            </header>
            {cards.map((card) => {
              const busy = busyCardId !== null || busyId !== null;
              const expired = Date.now() - card.waitingSince > CARD_EXPIRY_MS;
              return (
                <article
                  key={card.sessionId}
                  className={`intervention-item approval-item severity-high${
                    expired ? " approval-expired" : ""
                  }`}
                >
                  <div className="intervention-item-heading">
                    <span>权限确认{expired ? " · 已超时" : ""}</span>
                    <time>{formatWaitingAge(card.waitingSince)}</time>
                  </div>
                  <strong>{card.title}</strong>
                  <p className="approval-reason">
                    {card.waitingReason?.trim() || "Claude 正在等待权限确认"}
                  </p>
                  <p className="approval-meta">
                    {card.providerName} · {card.cwd}
                  </p>
                  {expired && (
                    <p className="approval-expired-hint">
                      已等待超过 10 分钟：请到终端确认，或直接在此决定。
                    </p>
                  )}
                  <footer>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void respondCard(card, "deny")}
                    >
                      拒绝
                    </button>
                    <button
                      className="primary"
                      type="button"
                      disabled={busy}
                      onClick={() => void respondCard(card, "allow")}
                    >
                      允许
                    </button>
                    <label className="approval-policy">
                      本会话
                      <select
                        value={card.approvalPolicy}
                        disabled={busy}
                        onChange={(event) =>
                          void setCardPolicy(
                            card,
                            event.target.value as ApprovalPolicy,
                          )
                        }
                      >
                        {(Object.keys(POLICY_LABELS) as ApprovalPolicy[]).map(
                          (policy) => (
                            <option key={policy} value={policy}>
                              {POLICY_LABELS[policy]}
                            </option>
                          ),
                        )}
                      </select>
                    </label>
                  </footer>
                </article>
              );
            })}
          </section>
        )}
        {grouped.map(([projectId, group]) => (
          <section key={projectId} className="intervention-project-group">
            <header>
              <strong>{group.projectName}</strong>
              <span>{group.items.length}</span>
            </header>
            {group.items.map((item) => {
              const response = replyDrafts[item.id] ?? "";
              const busy = busyId !== null || busyCardId !== null;
              return (
                <article
                  key={item.id}
                  className={`intervention-item severity-${item.severity}`}
                >
                  <div className="intervention-item-heading">
                    <span>{KIND_LABELS[item.kind]}</span>
                    <time>{new Date(item.updatedAt).toLocaleString()}</time>
                  </div>
                  <strong>{item.title}</strong>
                  <p>{item.summary}</p>
                  {item.recommendedAction === "respond" && (
                    <textarea
                      rows={2}
                      value={response}
                      disabled={busy}
                      placeholder="输入给 Agent 的补充信息"
                      onChange={(event) =>
                        setReplyDrafts((current) => ({
                          ...current,
                          [item.id]: event.target.value,
                        }))
                      }
                    />
                  )}
                  <footer>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void resolve(item, "snooze")}
                    >
                      暂缓 1 小时
                    </button>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void resolve(item, "dismiss")}
                    >
                      忽略本次
                    </button>
                    <button
                      className="primary"
                      type="button"
                      disabled={
                        busy ||
                        (item.recommendedAction === "respond" && !response.trim())
                      }
                      onClick={() => runPrimaryAction(item)}
                    >
                      {PRIMARY_ACTION_LABELS[item.recommendedAction]}
                    </button>
                  </footer>
                </article>
              );
            })}
          </section>
        ))}
        {!items.length && !cards.length && !error && (
          <EmptyState
            className="intervention-center-empty"
            title="目前没有待处理事项"
            description="需要审批、回复或人工决定的项目会集中出现在这里。"
            descriptionAs="p"
          />
        )}
      </div>
    </section>
  );
}

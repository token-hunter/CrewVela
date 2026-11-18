import { useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowRight, Check, CheckCircle, Clock, FileText, Robot, WarningCircle, X } from "@phosphor-icons/react";
import type { AgentProfile, ConversationDesignDelegation, ConversationDesignDelivery } from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { ConversationMarkdown } from "./ConversationMarkdown";
import "../styles/conversation-delegation.css";

const STATUS = {
  checkpointing: "正在保存检查点",
  waiting_design: "等待设计交付",
  accepted: "交付已验收",
  resumed: "原任务已续跑",
  failed: "委派未完成",
};

const time = (value: number) => new Date(value).toLocaleTimeString("zh-CN", {
  hour: "2-digit", minute: "2-digit", hour12: false,
});

export function ConversationDelegationDetails({ item, agents, projectId, conversationId, onClose }: {
  item: ConversationDesignDelegation;
  agents: AgentProfile[];
  projectId: string;
  conversationId: string;
  onClose: () => void;
}) {
  const headingId = useId();
  const deliveryId = useId();
  const close = useRef<HTMLButtonElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [delivery, setDelivery] = useState<ConversationDesignDelivery | null>(null);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => { close.current?.focus(); }, []);
  useEffect(() => {
    if (!expanded || delivery || !item.filename) return;
    let live = true;
    setError("");
    invoke<ConversationDesignDelivery>("ma_conversation_design_delivery", { projectId, conversationId, delegationId: item.id })
      .then(value => { if (live) setDelivery(value); })
      .catch(reason => { if (live) setError(String(reason)); });
    return () => { live = false; };
  }, [expanded, delivery, item.filename, item.id, projectId, conversationId, retry]);

  const participant = (id: string) => {
    const agent = agents.find(value => value.id === id);
    return <span className="delegation-participant">
      {agent ? <AgentAvatar avatarId={agent.avatarId} runtimeKind={agent.runtimeKind} decorative /> : <Robot size={24} />}
      <span>{agent?.name ?? "历史成员"}</span>
    </span>;
  };
  const StatusIcon = item.status === "resumed" || item.status === "accepted" ? CheckCircle : item.status === "failed" ? WarningCircle : Clock;
  const sourceName = agents.find(agent => agent.id === item.sourceAgentId)?.name.replace(/\s+Agent$/i, "");

  return <aside className="conversation-delegation-details" aria-labelledby={headingId} onKeyDown={event => {
    if (event.key === "Escape") { event.stopPropagation(); onClose(); }
  }}>
    <header className="delegation-drawer-header">
      <h2 id={headingId}>委派详情</h2>
      <button type="button" ref={close} className="delegation-close" aria-label="关闭委派详情" onClick={onClose}><X size={21} /></button>
    </header>
    <div className="delegation-drawer-body">
      <section className="delegation-intro">
        <h3>{item.title}</h3>
        <div className="delegation-route">{participant(item.sourceAgentId)}<ArrowRight size={22} aria-label="委派给" />{participant(item.targetAgentId)}</div>
        <span className={`delegation-status ${item.status}`} role="status"><StatusIcon size={22} />{item.status === "resumed" && sourceName ? `${sourceName}已续跑` : STATUS[item.status]}</span>
      </section>
      <ol className="delegation-timeline" aria-label="委派进度">
        <li data-state={item.checkpointAt ? "active" : "pending"}><span aria-hidden="true" className="delegation-step" /><span>{item.checkpointAt ? time(item.checkpointAt) : "待完成"}</span><strong>保存检查点</strong></li>
        <li data-state={item.submittedAt ? "active" : "pending"}><span aria-hidden="true" className="delegation-step" /><span>{item.submittedAt ? time(item.submittedAt) : "待交付"}</span><strong>提交设计交付</strong></li>
        <li data-state={item.resumedAt ? "done" : "pending"}><span aria-hidden="true" className="delegation-step">{item.resumedAt && <Check size={13} weight="bold" />}</span><span>{item.resumedAt ? time(item.resumedAt) : "待续跑"}</span><strong>验收通过并自动续跑</strong></li>
      </ol>
      {item.status === "failed" && <p role="alert" className="delegation-failure">{item.failure || "原任务或设计任务已结束，委派尚未完成。"}</p>}
      <section className="delegation-artifact" aria-label="交付物">
        <h3>交付物</h3>
        {item.filename ? <div className="delegation-artifact-row">
          <FileText size={28} /><strong title={item.filename}>{item.filename}</strong>
          <button type="button" aria-controls={expanded ? deliveryId : undefined} aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{expanded ? "收起交付" : "查看交付"}</button>
        </div> : <p className="delegation-note">等待架构设计交付</p>}
        {expanded && <div id={deliveryId} className="delegation-delivery">
          {delivery ? <><ConversationMarkdown body={delivery.markdown} /><h4>验收依据</h4><dl>{delivery.acceptanceEvidence.map(item =>
            <div key={item.criterion}><dt>{item.criterion}</dt><dd><ConversationMarkdown body={item.evidence} /></dd></div>)}</dl></>
            : error ? <p role="alert">{error} <button type="button" className="conversation-text-button" onClick={() => setRetry(value => value + 1)}>重试</button></p>
            : <p role="status">正在读取交付…</p>}
        </div>}
      </section>
      <p className="delegation-note">恢复原任务 · 延续原授权</p>
    </div>
  </aside>;
}

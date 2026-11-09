import { useEffect, useRef } from "react";
import { ArrowElbowDownLeftIcon } from "@phosphor-icons/react/dist/csr/ArrowElbowDownLeft";
import { RobotIcon } from "@phosphor-icons/react/dist/csr/Robot";
import { UsersThreeIcon } from "@phosphor-icons/react/dist/csr/UsersThree";
import type { AgentRuntimeKind } from "../types";
import { AgentAvatar } from "./AgentAvatar";

export interface AgentMentionOption {
  key: string;
  kind: "all" | "runtime" | "agent" | "session";
  token: string;
  description: string;
  section: "群组" | "具体 Agent" | "终端会话";
  runtimeKind?: AgentRuntimeKind;
  avatarId?: string | null;
  agentId?: string;
  sessionId?: string;
  targetCount?: number;
}

export function AgentMentionMenu({ id, options, activeIndex, onHighlight, onSelect }: {
  id: string;
  options: readonly AgentMentionOption[];
  activeIndex: number;
  onHighlight: (index: number) => void;
  onSelect: (option: AgentMentionOption) => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const list = listRef.current;
    const active = list?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (!list || !active) return;
    // Scroll only the results, keeping the composer and surrounding chat still.
    const bottom = active.offsetTop + active.offsetHeight;
    if (active.offsetTop < list.scrollTop) list.scrollTop = active.offsetTop;
    else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
  }, [activeIndex, options]);

  return <div className="multi-agent-mention-menu">
    <div className="multi-agent-mention-header">
      <strong>选择通信对象</strong><span>@ 提及 Agent</span>
    </div>
    <div ref={listRef} id={id} className="multi-agent-mention-results" role="listbox" aria-label="选择协作 Agent">
      {(["群组", "具体 Agent", "终端会话"] as const).map((section) => {
        const items = options.filter((option) => option.section === section);
        if (!items.length) return null;
        return <div className="multi-agent-mention-section" role="group" aria-label={section} key={section}>
          {section !== "群组" && <div className="multi-agent-mention-section-label">{section}</div>}
          {items.map((option) => {
            const index = options.indexOf(option);
            const description = option.kind === "all" ? "全部项目成员"
              : option.kind === "runtime" ? option.description.replace(/\s*·\s*\d+\s*个实例$/, "")
                : option.description;
            return <button
              key={option.key} id={`${id}-option-${index}`} type="button" role="option"
              aria-selected={index === activeIndex}
              className={`multi-agent-mention-option${index === activeIndex ? " active" : ""}`}
              title={`${option.token} · ${option.description}`}
              onMouseDown={(event) => event.preventDefault()}
              onMouseMove={() => onHighlight(index)} onClick={() => onSelect(option)}
            >
              {option.runtimeKind
                ? <AgentAvatar className="ma-mention-avatar" avatarId={option.avatarId} runtimeKind={option.runtimeKind} decorative />
                : option.kind === "all" ? <UsersThreeIcon className="ma-mention-avatar multi-agent-mention-group-icon" aria-hidden="true" />
                  : <RobotIcon className="ma-mention-avatar" aria-hidden="true" />}
              <strong className="multi-agent-mention-name">{option.token}</strong>
              <span className="multi-agent-mention-description">{description}</span>
              <em className="multi-agent-mention-count" aria-label={typeof option.targetCount === "number" ? `${option.targetCount} 个实例` : undefined}>
                {option.targetCount}
              </em>
              <ArrowElbowDownLeftIcon className="multi-agent-mention-enter" size={14} aria-hidden="true" />
            </button>;
          })}
        </div>;
      })}
    </div>
    <div className="multi-agent-mention-footer"><span><kbd>↑↓</kbd> 选择</span><span><kbd>Enter</kbd> 插入</span><span><kbd>Esc</kbd> 关闭</span></div>
  </div>;
}

import { useEffect, useRef } from "react";
import { ArrowElbowDownLeftIcon } from "@phosphor-icons/react/dist/csr/ArrowElbowDownLeft";
import { CheckIcon } from "@phosphor-icons/react/dist/csr/Check";
import { RobotIcon } from "@phosphor-icons/react/dist/csr/Robot";
import type { AgentRuntimeKind } from "../types";
import { AgentAvatar } from "./AgentAvatar";

export interface AgentMentionOption {
  key: string;
  kind: "all" | "runtime" | "agent" | "session";
  token: string;
  description: string;
  section: "群组" | "具体 Agent" | "终端会话";
  runtimeKind?: AgentRuntimeKind;
  runtimeLabel?: string;
  avatarId?: string | null;
  agentId?: string;
  sessionId?: string;
  targetCount?: number;
}

export function AgentMentionMenu({ id, options, activeIndex, onHighlight, onSelect, onSelectAll, allSelected, multiple = false, selectedKeys = [], selectedCount = 0, onMultipleChange, onConfirm }: {
  id: string;
  options: readonly AgentMentionOption[];
  activeIndex: number;
  onHighlight: (index: number) => void;
  onSelect: (option: AgentMentionOption) => void;
  onSelectAll: () => void;
  allSelected: boolean;
  multiple?: boolean;
  selectedKeys?: readonly string[];
  selectedCount?: number;
  onMultipleChange?: (enabled: boolean) => void;
  onConfirm?: () => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const list = listRef.current;
    const active = list?.querySelector<HTMLElement>('.multi-agent-mention-option.active');
    if (!list || !active) return;
    // Scroll only the results, keeping the composer and surrounding chat still.
    const bottom = active.offsetTop + active.offsetHeight;
    if (active.offsetTop < list.scrollTop) list.scrollTop = active.offsetTop;
    else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
  }, [activeIndex, options]);

  return <div className="multi-agent-mention-menu">
    <div className="multi-agent-mention-header">
      <strong>选择成员</strong>
      <div className="multi-agent-mention-controls">
        {onMultipleChange && <div className="multi-agent-mention-modes" role="group" aria-label="成员选择模式">
          <button type="button" aria-pressed={!multiple} onMouseDown={(event) => event.preventDefault()} onClick={() => onMultipleChange(false)}>单选</button>
          <button type="button" aria-pressed={multiple} onMouseDown={(event) => event.preventDefault()} onClick={() => onMultipleChange(true)}>多选</button>
        </div>}
        <button type="button" className="multi-agent-mention-select-all" disabled={!options.length} onMouseDown={(event) => event.preventDefault()} onClick={onSelectAll}>
          {allSelected ? "取消全选" : "全选"}
        </button>
      </div>
    </div>
    <div ref={listRef} id={id} className="multi-agent-mention-results" role="listbox" aria-label="选择协作 Agent" aria-multiselectable={multiple}>
      {!options.length && <p>没有匹配的成员</p>}
      {(["具体 Agent"] as const).map((section) => {
        const items = options.filter((option) => option.section === section);
        if (!items.length) return null;
        return <div className="multi-agent-mention-section" role="group" aria-label={section} key={section}>
          {items.map((option) => {
            const index = options.indexOf(option);
            const checked = selectedKeys.includes(option.key);
            return <button
              key={option.key} id={`${id}-option-${index}`} type="button" role="option"
              aria-selected={multiple ? checked : index === activeIndex}
              className={`multi-agent-mention-option${index === activeIndex ? " active" : ""}${multiple ? " multiple" : ""}`}
              title={`${option.token} · ${option.description}`}
              onMouseDown={(event) => event.preventDefault()}
              onMouseMove={() => onHighlight(index)} onClick={() => onSelect(option)}
            >
              {multiple && <span className="multi-agent-mention-check" aria-hidden="true">{checked && <CheckIcon size={13} weight="bold" />}</span>}
              {option.runtimeKind
                ? <AgentAvatar className="ma-mention-avatar" avatarId={option.avatarId} runtimeKind={option.runtimeKind} decorative />
                : <RobotIcon className="ma-mention-avatar" aria-hidden="true" />}
              <span className="multi-agent-mention-identity">
                <strong className="multi-agent-mention-name">{option.token}</strong>
                {option.runtimeLabel && <small className="multi-agent-mention-runtime">{option.runtimeLabel}</small>}
              </span>
              <span className="multi-agent-mention-description">{option.description}</span>
              {!multiple && <ArrowElbowDownLeftIcon className="multi-agent-mention-enter" size={14} aria-hidden="true" />}
            </button>;
          })}
        </div>;
      })}
    </div>
    <div className="multi-agent-mention-footer">
      <div className="multi-agent-mention-summary">
        {multiple && <strong role="status">已选 {selectedCount} 位成员</strong>}
        <div className="multi-agent-mention-shortcuts">
          <span><kbd>↑↓</kbd> 移动</span>
          {multiple && <span><kbd>Space</kbd> 勾选</span>}
          <span><kbd>Enter</kbd> 插入</span>
          <span><kbd>Esc</kbd> 关闭</span>
        </div>
      </div>
      {multiple && <button type="button" className="multi-agent-mention-confirm" disabled={!selectedCount} onMouseDown={(event) => event.preventDefault()} onClick={onConfirm}>插入所选</button>}
    </div>
  </div>;
}

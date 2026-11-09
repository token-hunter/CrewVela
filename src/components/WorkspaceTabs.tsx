import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MinusIcon } from "@phosphor-icons/react/dist/csr/Minus";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { SidebarSimpleIcon } from "@phosphor-icons/react/dist/csr/SidebarSimple";
import { SquareIcon } from "@phosphor-icons/react/dist/csr/Square";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import { CommunicationGuideButton } from "./CommunicationGuide";
import type { WorkspaceTab } from "../workspaceTabs";

interface Props {
  tabs: WorkspaceTab[];
  activeTabId: string;
  onSelect: (tabId: string) => void;
  onAdd: () => void;
  onClose: (tab: WorkspaceTab) => void;
  canClose?: boolean;
  onRename: (tab: WorkspaceTab, title: string) => void;
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  rightSidebarOpen: boolean;
  onToggleRightSidebar: () => void;
}

export function WorkspaceTabs({
  tabs,
  activeTabId,
  onSelect,
  onAdd,
  onClose,
  canClose = true,
  onRename,
  sidebarCollapsed,
  onToggleSidebar,
  rightSidebarOpen,
  onToggleRightSidebar,
}: Props) {
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (renamingId !== null) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [renamingId]);

  const startRename = (tab: WorkspaceTab) => {
    setDraft(tab.title);
    setRenamingId(tab.id);
  };

  const commitRename = (tab: WorkspaceTab) => {
    const title = draft.trim();
    if (title && title !== tab.title) onRename(tab, title);
    setRenamingId(null);
  };

  const appWindow =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
      ? getCurrentWindow()
      : null;
  const reportWindowError = (action: string) => (error: unknown) =>
    console.error(`窗口${action}失败:`, error);

  return (
    <div
      className="workspace-tabs"
      role="tablist"
      aria-label="工作区页签"
      data-tauri-drag-region
    >
      {sidebarCollapsed && (
        <button
          type="button"
          className="workspace-sidebar-expand"
          aria-label="展开侧栏"
          title="展开侧栏"
          onClick={onToggleSidebar}
        >
          <SidebarSimpleIcon size={18} />
        </button>
      )}
      {tabs.map((tab) => {
        const active = tab.id === activeTabId;
        const renaming = renamingId === tab.id;
        return (
          <div
            key={tab.id}
            className={`workspace-tab${active ? " active" : ""}`}
            role="tab"
            aria-selected={active}
          >
            {renaming ? (
              <input
                ref={inputRef}
                className="workspace-tab-input"
                value={draft}
                maxLength={24}
                onChange={(event) => setDraft(event.target.value)}
                onBlur={() => commitRename(tab)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === "Escape") {
                    event.preventDefault();
                    event.stopPropagation();
                  }
                  if (event.key === "Enter") commitRename(tab);
                  if (event.key === "Escape") setRenamingId(null);
                }}
              />
            ) : (
              <button
                type="button"
                className="workspace-tab-title"
                title="双击重命名"
                onClick={() => onSelect(tab.id)}
                onDoubleClick={() => startRename(tab)}
              >
                {tab.title}
              </button>
            )}
            {tabs.length > 1 && (
              <button
                type="button"
                className="workspace-tab-close"
                aria-label={`关闭页签 ${tab.title}`}
                title={canClose ? `关闭页签 ${tab.title}` : "会话数据正在加载"}
                disabled={!canClose}
                onClick={() => onClose(tab)}
              >
                <XIcon size={11} weight="bold" />
              </button>
            )}
          </div>
        );
      })}
      <button
        type="button"
        className="workspace-tab-add"
        aria-label="新增页签"
        title="新增页签"
        onClick={onAdd}
      >
        <PlusIcon size={13} weight="bold" />
      </button>
      <button
        type="button"
        className={`workspace-right-sidebar-toggle${rightSidebarOpen ? " active" : ""}`}
        aria-label="切换右侧栏"
        title="切换右侧栏"
        aria-pressed={rightSidebarOpen}
        onClick={onToggleRightSidebar}
      >
        <SidebarSimpleIcon size={18} />
      </button>
      <CommunicationGuideButton className="workspace-communication-button" />
      <div className="window-controls">
        <button
          type="button"
          className="window-control"
          aria-label="最小化"
          title="最小化"
          onClick={() =>
            void appWindow?.minimize().catch(reportWindowError("最小化"))
          }
        >
          <MinusIcon size={13} />
        </button>
        <button
          type="button"
          className="window-control"
          aria-label="最大化 / 还原"
          title="最大化 / 还原"
          onClick={() =>
            void appWindow
              ?.toggleMaximize()
              .catch(reportWindowError("最大化切换"))
          }
        >
          <SquareIcon size={12} />
        </button>
        <button
          type="button"
          className="window-control window-control-close"
          aria-label="关闭"
          title="关闭"
          onClick={() =>
            void appWindow?.close().catch(reportWindowError("关闭"))
          }
        >
          <XIcon size={13} weight="bold" />
        </button>
      </div>
    </div>
  );
}

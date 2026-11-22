import { terminalShells, shellLabel } from "../platform";
import { useRuntimeAvailability } from "../runtimeAvailability";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ComponentProps, ReactNode } from "react";
import { ArchiveIcon } from "@phosphor-icons/react/dist/csr/Archive";
import { BellIcon } from "@phosphor-icons/react/dist/csr/Bell";
import { BrainIcon } from "@phosphor-icons/react/dist/csr/Brain";
import { CornersOutIcon } from "@phosphor-icons/react/dist/csr/CornersOut";
import { CubeIcon } from "@phosphor-icons/react/dist/csr/Cube";
import { DotsThreeIcon } from "@phosphor-icons/react/dist/csr/DotsThree";
import { FolderPlusIcon } from "@phosphor-icons/react/dist/csr/FolderPlus";
import { GearSixIcon } from "@phosphor-icons/react/dist/csr/GearSix";
import { GitBranchIcon } from "@phosphor-icons/react/dist/csr/GitBranch";
import { GlobeIcon } from "@phosphor-icons/react/dist/csr/Globe";
import { HeartIcon } from "@phosphor-icons/react/dist/csr/Heart";
import { LightningIcon } from "@phosphor-icons/react/dist/csr/Lightning";
import { MonitorIcon } from "@phosphor-icons/react/dist/csr/Monitor";
import { ScrollIcon } from "@phosphor-icons/react/dist/csr/Scroll";
import { PackageIcon } from "@phosphor-icons/react/dist/csr/Package";
import { PaletteIcon } from "@phosphor-icons/react/dist/csr/Palette";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { RobotIcon } from "@phosphor-icons/react/dist/csr/Robot";
import { SidebarSimpleIcon } from "@phosphor-icons/react/dist/csr/SidebarSimple";
import { SlidersIcon } from "@phosphor-icons/react/dist/csr/Sliders";
import { TerminalWindowIcon } from "@phosphor-icons/react/dist/csr/TerminalWindow";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { UsersIcon } from "@phosphor-icons/react/dist/csr/Users";
import type {
  AgentRuntimeKind,
  LaunchPreset,
  Project,
  ProviderConfig,
  SessionRecord,
  SessionTypeFilter,
  SessionUsage,
  TerminalShell,
} from "../types";
import claudeIcon from "../assets/claude-color.png";
import { RUNTIME_LOGOS } from "../runtimeLogos";
import { SessionList } from "./SessionList";
import { CommunicationGuideButton } from "./CommunicationGuide";
import { AutomationIcon } from "./AutomationIcon";
import { AppToolsMenu } from "./AppToolsMenu";
import { AppHelpDialog } from "./AppHelpDialog";
import { Modal } from "./ui/Modal";
import type { AppHelpSection } from "../appHelp";
import type { AppViewMenuProps } from "../appView";
import { SidebarAttention } from "./SidebarAttention";
import type { AttentionKind } from "../sidebarAttention";
import type { SettingsSection } from "./SettingsDialog";
import {
  DEFAULT_SIDEBAR_SHORTCUTS,
  SIDEBAR_SHORTCUT_OPTIONS,
  loadSidebarShortcuts,
  saveSidebarShortcuts,
} from "../sidebarShortcuts";

// 与设置页导航保持一致的图标/字重复现；key 即 sidebarShortcuts 的 option id。
const SIDEBAR_SHORTCUT_ICONS: Record<string, () => ReactNode> = {
  generalSettings: () => <GearSixIcon size={17} />,
  git: () => <GitBranchIcon size={17} />,
  worktrees: () => <GitBranchIcon size={17} />,
  notifications: () => <BellIcon size={17} />,
  appearance: () => <PaletteIcon size={17} />,
  companionship: () => <HeartIcon size={17} />,
  collaboration: () => <UsersIcon size={17} />,
  sharedMemory: () => <BrainIcon size={17} />,
  automation: () => <AutomationIcon size={17} />,
  browserControl: () => <GlobeIcon size={17} />,
  skills: () => <ScrollIcon size={17} weight="regular" />,
  archivedSessions: () => <ArchiveIcon size={17} />,
};

interface Props extends Pick<ComponentProps<typeof SessionList>, "onOpenProject" | "onManageProjects" | "projectResumeIds" | "onContinueProject" | "projectServiceAction"> {
  workspaceId: string;
  workspaceName: string;
  sessions: SessionRecord[];
  projects: Project[];
  usages: Record<string, SessionUsage>;
  selectedSessionId: string | null;
  loadingSessionId: string | null;
  collapsed: boolean;
  onCollapse: () => void;
  rightSidebarMenuOpen: boolean;
  onRightSidebarMenuChange: (open: boolean) => void;
  sessionTypeFilter: SessionTypeFilter;
  onSessionTypeFilter: (filter: SessionTypeFilter) => void;
  onSelectSession: (id: string | null) => void;
  onRenameSession: (session: SessionRecord) => void;
  onStopSession: (session: SessionRecord) => void;
  onDeleteSession: (session: SessionRecord) => void;
  onArchiveSession: (session: SessionRecord) => void;
  onViewTranscript: (session: SessionRecord) => void;
  onSwitchAgent?: (session: SessionRecord) => void;
  sourceHistory?: (session: SessionRecord) => SessionRecord | null;
  onRelayDetails?: (session: SessionRecord) => void;
  logicalSessionId?: (id: string) => string;
  onNewClaudeSession: () => void;
  onOpenDeepSeekHarness: () => void;
  onOpenClaudeChat: () => void;
  onOpenCodexChat: () => void;
  onOpenKimiChat: () => void;
  onOpenOpenCodeChat: () => void;
  onOpenPiChat: () => void;
  onOpenZCode: () => void;
  onOpenZCodeCli: () => void;
  onNewTerminal: (shell: TerminalShell) => void;
  onNewRuntimeSession: (
    runtimeKind: Extract<
      AgentRuntimeKind,
      "codex" | "kimi_code" | "pi" | "opencode"
    >,
  ) => void;
  onAddProject: () => void;
  presets: LaunchPreset[];
  onLaunchPreset: (preset: LaunchPreset) => void;
  onDeletePreset: (preset: LaunchPreset) => void;
  onNewSessionInProject: (project: Project) => void;
  onRenameProject: (project: Project) => void;
  onDeleteProject: (project: Project) => void;
  onArchiveProjectSessions: (project: Project) => void;
  viewFocused: boolean;
  viewSwitchDisabled: boolean;
  onFocusView: () => void;
  onOverviewView: () => void;
  stopAllDisabled: boolean;
  onStopAll: () => void;
  deleteAllDisabled: boolean;
  onDeleteAll: () => void;
  onOpenSection: (section: SettingsSection) => void;
  onOpenSettings: () => void;
  onOpenCollaborationOverview?: () => void;
  collaborationOverviewOpen?: boolean;
  onOpenAttentionSession: (session: SessionRecord, kind: AttentionKind) => void;
  viewMenu?: AppViewMenuProps;
}

export function AppSidebar(props: Props) {
  const { isRuntimeEnabled } = useRuntimeAvailability();
  const [newMenuOpen, setNewMenuOpen] = useState(false);
  const [toolsMenuOpen, setToolsMenuOpen] = useState(false);
  const [helpSection, setHelpSection] = useState<AppHelpSection | null>(null);
  const toolsButtonRef = useRef<HTMLButtonElement>(null);
  const newSessionButtonRef = useRef<HTMLButtonElement>(null);
  const newSessionMenuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!newMenuOpen || toolsMenuOpen) return;
    newSessionMenuRef.current?.querySelector<HTMLButtonElement>('button[role="menuitem"]')?.focus();
    const onEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setNewMenuOpen(false);
      newSessionButtonRef.current?.focus();
    };
    window.addEventListener("keydown", onEscape);
    return () => window.removeEventListener("keydown", onEscape);
  }, [newMenuOpen, toolsMenuOpen]);
  const [deleteRequest, setDeleteRequest] = useState<{ workspaceId: string; sessionIds: string[] } | null>(null);
  const deleteCancelButton = useRef<HTMLButtonElement>(null);
  const deleteConfirmed = useRef(false);
  useEffect(() => { setDeleteRequest(null); }, [props.workspaceId]);
  const deleteScopeCurrent = deleteRequest?.workspaceId === props.workspaceId
    && deleteRequest.sessionIds.length === props.sessions.length
    && props.sessions.every(session => deleteRequest.sessionIds.includes(session.id));
  const requestDeleteAll = () => {
    if (props.deleteAllDisabled) return;
    deleteConfirmed.current = false;
    setDeleteRequest({ workspaceId: props.workspaceId, sessionIds: props.sessions.map(session => session.id) });
  };
  const [shortcuts, setShortcuts] = useState<SettingsSection[]>(() =>
    loadSidebarShortcuts(),
  );
  const [shortcutsMenuOpen, setShortcutsMenuOpen] = useState(false);

  const toggleShortcut = (id: SettingsSection) => {
    setShortcuts((current) => {
      const next = current.includes(id)
        ? current.filter((item) => item !== id)
        : [...current, id];
      saveSidebarShortcuts(next);
      return next;
    });
  };

  const resetShortcuts = () => {
    setShortcuts([...DEFAULT_SIDEBAR_SHORTCUTS]);
    saveSidebarShortcuts(DEFAULT_SIDEBAR_SHORTCUTS);
  };

  const runNewAction = (action: () => void) => {
    setNewMenuOpen(false);
    setToolsMenuOpen(false);
    action();
  };

  const closeToolsMenus = () => {
    setNewMenuOpen(false);
    setToolsMenuOpen(false);
  };

  if (props.collapsed) {
    return null;
  }

  return (
    <aside className="sidebar unified-sidebar">
      <header className="unified-sidebar-header" data-tauri-drag-region>
        <div className="unified-sidebar-header-actions">
          <button
            ref={toolsButtonRef}
            type="button"
            aria-label="更多工具"
            title="更多工具"
            aria-haspopup="menu"
            aria-expanded={toolsMenuOpen}
            onClick={() => {
              if (toolsMenuOpen) {
                closeToolsMenus();
              } else {
                // 新建入口只保留下方的「新建会话」按钮；打开本菜单时先收起它的列表。
                setNewMenuOpen(false);
                setToolsMenuOpen(true);
              }
            }}
          >
            <DotsThreeIcon size={18} weight="bold" />
          </button>
          <button
            type="button"
            aria-label="收起侧栏"
            title="收起侧栏"
            onClick={props.onCollapse}
          >
            <SidebarSimpleIcon size={18} />
          </button>
          <span className="unified-sidebar-view-switch" aria-label="视图切换">
            <button
              className={props.viewFocused ? "active" : ""}
              type="button"
              disabled={props.viewSwitchDisabled}
              aria-label="聚焦"
              title="聚焦"
              aria-pressed={props.viewFocused}
              onClick={props.onFocusView}
            >
              <CornersOutIcon size={17} />
            </button>
            <button
              className={props.viewFocused ? "" : "active"}
              type="button"
              aria-label="总览"
              title="总览"
              aria-pressed={!props.viewFocused}
              onClick={props.onOverviewView}
            >
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true" focusable="false">
                <rect x="3" y="3" width="18" height="18" rx="3" />
                <rect x="6.5" y="6.4" width="2.2" height="2.2" rx="0.5" fill="currentColor" stroke="none" />
                <rect x="6.5" y="10.9" width="2.2" height="2.2" rx="0.5" fill="currentColor" stroke="none" />
                <rect x="6.5" y="15.4" width="2.2" height="2.2" rx="0.5" fill="currentColor" stroke="none" />
                <path d="M11.5 7.5H17.5M11.5 12H17.5M11.5 16.5H17.5" strokeLinecap="round" />
              </svg>
            </button>
          </span>
        </div>
      </header>
      {toolsMenuOpen && <AppToolsMenu trigger={toolsButtonRef} reserveSidebarSpace onClose={closeToolsMenus} onHelp={setHelpSection} view={props.viewMenu} />}
      <div className="sidebar-new-session-row">
        <button
          ref={newSessionButtonRef}
          type="button"
          className="sidebar-new-session-button"
          data-switching-menu={toolsMenuOpen ? "tools" : props.rightSidebarMenuOpen ? "workspace" : undefined}
          aria-haspopup="menu"
          aria-expanded={newMenuOpen && !toolsMenuOpen}
          aria-controls={newMenuOpen ? "sidebar-new-session-menu" : undefined}
          onClick={() => {
            props.onRightSidebarMenuChange(false);
            setToolsMenuOpen(false);
            setNewMenuOpen(!newMenuOpen || toolsMenuOpen || props.rightSidebarMenuOpen);
          }}
        >
          <span className="sidebar-new-session-icon" aria-hidden="true"><PlusIcon size={12} weight="bold" /></span>
          <span>新建会话</span>
        </button>
      </div>
      {newMenuOpen && !toolsMenuOpen && <div className="menu-backdrop" onClick={closeToolsMenus} />}
      <nav className="unified-sidebar-nav" aria-label="主导航">
        <div className="unified-sidebar-nav-head">
          <span>快捷入口</span>
          <button
            type="button"
            aria-label="自定义快捷入口"
            title="自定义快捷入口"
            aria-expanded={shortcutsMenuOpen}
            onClick={() => setShortcutsMenuOpen((current) => !current)}
          >
            <SlidersIcon size={14} weight="bold" />
          </button>
        </div>
        <CommunicationGuideButton className="communication-sidebar-entry" />
        {props.onOpenCollaborationOverview && <button type="button"
          className={`collaboration-overview-entry${props.collaborationOverviewOpen ? " active" : ""}`}
          aria-label="Bot 模式" title="Bot 模式 · 协作项目总览"
          aria-pressed={props.collaborationOverviewOpen} onClick={props.onOpenCollaborationOverview}>
          <RobotIcon size={19} aria-hidden="true" /><span>Bot 模式</span>
        </button>}
        {SIDEBAR_SHORTCUT_OPTIONS.filter((option) =>
          shortcuts.includes(option.id),
        ).map((option) => {
          const renderIcon = SIDEBAR_SHORTCUT_ICONS[option.id];
          return (
            <button
              type="button"
              key={option.id}
              onClick={() => props.onOpenSection(option.id)}
            >
              {renderIcon ? (
                renderIcon()
              ) : (
                <GearSixIcon size={17} />
              )}
              <span>{option.label}</span>
            </button>
          );
        })}
        {shortcutsMenuOpen && (
          <>
            <div
              className="menu-backdrop"
              onClick={() => setShortcutsMenuOpen(false)}
            />
            <div
              className="sidebar-shortcut-picker"
              role="group"
              aria-label="自定义快捷入口"
            >
              <header className="sidebar-shortcut-picker-head">
                <strong>显示哪些设置入口</strong>
                <button type="button" onClick={resetShortcuts}>
                  恢复默认
                </button>
              </header>
              <p className="sidebar-shortcut-picker-hint">
                勾选后显示在左侧栏，全部取消即全部隐藏。
              </p>
              {(["通用", "智能体", "集成", "数据"] as const).map((group) => (
                <div className="sidebar-shortcut-picker-group" key={group}>
                  <span>{group}</span>
                  {SIDEBAR_SHORTCUT_OPTIONS.filter(
                    (option) => option.group === group,
                  ).map((option) => (
                    <label key={option.id}>
                      <input
                        type="checkbox"
                        checked={shortcuts.includes(option.id)}
                        onChange={() => toggleShortcut(option.id)}
                      />
                      <span>{option.label}</span>
                    </label>
                  ))}
                </div>
              ))}
            </div>
          </>
        )}
      </nav>

      <div className="session-drawer unified-session-drawer">
        <SessionList
          bulkActions={{ workspaceId: props.workspaceId, workspaceName: props.workspaceName, stopDisabled: props.stopAllDisabled, deleteDisabled: props.deleteAllDisabled, onStop: props.onStopAll, onDelete: requestDeleteAll }}
          sessions={props.sessions}
          projects={props.projects}
          usages={props.usages}
          selectedId={props.selectedSessionId}
          loadingId={props.loadingSessionId}
          typeFilter={props.sessionTypeFilter}
          onTypeFilter={props.onSessionTypeFilter}
          onSelect={props.onSelectSession}
          onRename={props.onRenameSession}
          onStop={props.onStopSession}
          onDelete={props.onDeleteSession}
          onArchive={props.onArchiveSession}
          onViewTranscript={props.onViewTranscript}
          onSwitchAgent={props.onSwitchAgent}
          sourceHistory={props.sourceHistory}
          onRelayDetails={props.onRelayDetails}
          logicalSessionId={props.logicalSessionId}
          onNewSessionInProject={props.onNewSessionInProject}
          onOpenProject={props.onOpenProject}
          onManageProjects={props.onManageProjects}
          projectResumeIds={props.projectResumeIds}
          onContinueProject={props.onContinueProject}
          projectServiceAction={props.projectServiceAction}
          onRenameProject={props.onRenameProject}
          onDeleteProject={props.onDeleteProject}
          onArchiveProjectSessions={props.onArchiveProjectSessions}
          onAddProject={props.onAddProject}
        />
      </div>

      <div className="unified-header-new-action">
        <div className="unified-new-action">
          {newMenuOpen && (
              <div ref={newSessionMenuRef} id="sidebar-new-session-menu" className={`rail-new-menu unified-new-menu runtime-submenu${toolsMenuOpen ? "" : " direct-new-menu"}`} role="menu" aria-label="新建 Runtime 会话">
                {isRuntimeEnabled("claude_code") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onNewClaudeSession)}>
                  <img src={claudeIcon} alt="" aria-hidden="true" />
                  <span><strong>Claude Code</strong></span>
                </button>}
                {isRuntimeEnabled("codex") && <button type="button" role="menuitem" onClick={() => runNewAction(() => props.onNewRuntimeSession("codex"))}>
                  <img src={RUNTIME_LOGOS.codex} data-runtime="codex" alt="" aria-hidden="true" />
                  <span><strong>Codex CLI</strong></span>
                </button>}
                {isRuntimeEnabled("kimi_code") && <button type="button" role="menuitem" onClick={() => runNewAction(() => props.onNewRuntimeSession("kimi_code"))}>
                  <img src={RUNTIME_LOGOS.kimi_code} data-runtime="kimi_code" alt="" aria-hidden="true" />
                  <span><strong>Kimi Code</strong></span>
                </button>}
                {isRuntimeEnabled("pi") && <button type="button" role="menuitem" onClick={() => runNewAction(() => props.onNewRuntimeSession("pi"))}>
                  <img src={RUNTIME_LOGOS.pi} data-runtime="pi" alt="" aria-hidden="true" />
                  <span><strong>Pi Agent</strong></span>
                </button>}
                {isRuntimeEnabled("opencode") && <button type="button" role="menuitem" onClick={() => runNewAction(() => props.onNewRuntimeSession("opencode"))}>
                  <img src={RUNTIME_LOGOS.opencode} data-runtime="opencode" alt="" aria-hidden="true" />
                  <span><strong>OpenCode</strong></span>
                </button>}

                {isRuntimeEnabled("deepseek_harness") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenDeepSeekHarness)}>
                  <img src={RUNTIME_LOGOS.deepseek_harness} data-runtime="deepseek_harness" alt="" aria-hidden="true" />
                  <span><strong>DeepSeek Harness</strong></span>
                </button>}
                {isRuntimeEnabled("claude_chat") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenClaudeChat)}>
                  <img src={RUNTIME_LOGOS.claude_chat} data-runtime="claude_chat" alt="" aria-hidden="true" />
                  <span><strong>Claude Code Chat</strong></span>
                </button>}
                {isRuntimeEnabled("codex_chat") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenCodexChat)}>
                  <img src={RUNTIME_LOGOS.codex_chat} data-runtime="codex_chat" alt="" aria-hidden="true" />
                  <span><strong>Codex Chat</strong></span>
                </button>}
                {isRuntimeEnabled("kimi_chat") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenKimiChat)}>
                  <img src={RUNTIME_LOGOS.kimi_chat} data-runtime="kimi_chat" alt="" aria-hidden="true" />
                  <span><strong>Kimi Code Chat</strong></span>
                </button>}
                {isRuntimeEnabled("opencode_chat") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenOpenCodeChat)}>
                  <img src={RUNTIME_LOGOS.opencode_chat} data-runtime="opencode_chat" alt="" aria-hidden="true" />
                  <span><strong>OpenCode Chat</strong></span>
                </button>}
                {isRuntimeEnabled("pi_chat") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenPiChat)}>
                  <img src={RUNTIME_LOGOS.pi_chat} data-runtime="pi_chat" alt="" aria-hidden="true" />
                  <span><strong>Pi Chat</strong></span>
                </button>}
                {isRuntimeEnabled("zcode") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenZCode)}><img src={RUNTIME_LOGOS.zcode} data-runtime="zcode" alt="" aria-hidden="true" />ZCode Desktop</button>}
                {isRuntimeEnabled("zcode_cli") && <button type="button" role="menuitem" onClick={() => runNewAction(props.onOpenZCodeCli)}><img src={RUNTIME_LOGOS.zcode_cli} data-runtime="zcode_cli" alt="" aria-hidden="true" />ZCode CLI</button>}
                {terminalShells().map(shell => <button key={shell} type="button" role="menuitem" onClick={() => runNewAction(() => props.onNewTerminal(shell))}>
                  <TerminalWindowIcon size={20} />
                  <span><strong>{shellLabel(shell)}</strong></span>
                </button>)}
                <button type="button" role="menuitem" onClick={() => runNewAction(props.onAddProject)}>
                  <FolderPlusIcon size={20} />
                  <span><strong>项目</strong></span>
                </button>
                {props.presets.length > 0 && (
                  <div className="rail-new-presets">
                    <div className="rail-new-presets-head">预设</div>
                    {props.presets.filter(() => isRuntimeEnabled("claude_code")).map((preset) => {
                      const providerName = "Claude Code（本地配置）";
                      return (
                        <div className="rail-new-preset" key={preset.id} role="none">
                          <button
                            type="button"
                            role="menuitem"
                            title={`${preset.cwds.length} 个会话 · ${providerName}`}
                            onClick={() => runNewAction(() => props.onLaunchPreset(preset))}
                          >
                            <LightningIcon size={18} />
                            <span>
                              <strong>{preset.name}</strong>
                              <small>
                                {preset.cwds.length} 个会话 · {providerName}
                                {preset.yolo ? " · YOLO" : preset.autoMode ? " · Auto" : ""}
                              </small>
                            </span>
                          </button>
                          <button
                            type="button"
                            className="rail-new-preset-delete"
                            aria-label={`删除预设 ${preset.name}`}
                            title="删除预设"
                            onClick={() => props.onDeletePreset(preset)}
                          >
                            <TrashIcon size={13} />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
          )}
        </div>
      </div>

      <footer className="unified-sidebar-footer">
        <button className="unified-sidebar-settings-button" type="button" onClick={props.onOpenSettings}>
          <GearSixIcon size={17} />
          <span>设置</span>
        </button>
        <SidebarAttention sessions={props.sessions} onOpenSession={props.onOpenAttentionSession} />
      </footer>

      {deleteRequest?.workspaceId === props.workspaceId && createPortal(
          <Modal overlayClassName="editor-overlay" className="confirm-dialog" ariaLabel="删除当前工作区全部会话" initialFocusRef={deleteCancelButton} closeOnOverlayClick onClose={() => setDeleteRequest(null)}>
            <div className="confirm-dialog-icon"><TrashIcon size={26} /></div>
            <h2>删除当前工作区全部会话</h2>
            <p>将删除「{props.workspaceName}」中的全部 {deleteRequest.sessionIds.length} 个会话，包含当前筛选中未显示的记录。此操作不可撤销。</p>
            {!deleteScopeCurrent && <p role="alert">会话列表已变化，请取消并重新选择删除操作。</p>}
            <div className="actions">
              <button ref={deleteCancelButton} className="btn" onClick={() => setDeleteRequest(null)}>取消</button>
              <button
                className="btn danger-solid"
                disabled={!deleteScopeCurrent || props.deleteAllDisabled}
                onClick={() => {
                  if (!deleteScopeCurrent || props.deleteAllDisabled || deleteConfirmed.current) return;
                  deleteConfirmed.current = true;
                  setDeleteRequest(null);
                  props.onDeleteAll();
                }}
              >
                确认删除
              </button>
            </div>
          </Modal>, document.body
      )}
      {helpSection && <AppHelpDialog key={helpSection} section={helpSection} onClose={() => setHelpSection(null)} />}
    </aside>
  );
}

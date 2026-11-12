import { sessionDisplayTitle, sessionDisplayProvider } from "../sessionCapabilities";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { sessionSwitchReason } from "../sessionSwitch";
import { createPortal } from "react-dom";
import { isSessionActive } from "../sessionCapabilities";
import { CaretDoubleLeftIcon } from "@phosphor-icons/react/dist/csr/CaretDoubleLeft";
import { ArchiveIcon } from "@phosphor-icons/react/dist/csr/Archive";
import { CaretDownIcon } from "@phosphor-icons/react/dist/csr/CaretDown";
import { CaretRightIcon } from "@phosphor-icons/react/dist/csr/CaretRight";
import { CircleIcon } from "@phosphor-icons/react/dist/csr/Circle";
import { DotsThreeIcon } from "@phosphor-icons/react/dist/csr/DotsThree";
import { FileTextIcon } from "@phosphor-icons/react/dist/csr/FileText";
import { FolderIcon } from "@phosphor-icons/react/dist/csr/Folder";
import { FolderOpenIcon } from "@phosphor-icons/react/dist/csr/FolderOpen";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { PencilSimpleIcon } from "@phosphor-icons/react/dist/csr/PencilSimple";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { PushPinIcon } from "@phosphor-icons/react/dist/csr/PushPin";
import { StarIcon } from "@phosphor-icons/react/dist/csr/Star";
import { StopIcon } from "@phosphor-icons/react/dist/csr/Stop";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { TerminalWindowIcon } from "@phosphor-icons/react/dist/csr/TerminalWindow";
import type {
  Project,
  SessionRecord,
  SessionStatus,
  SessionTypeFilter,
  SessionUsage,
} from "../types";
import { formatTokens } from "../format";
import { AgentAvatar } from "./AgentAvatar";
import { RUNTIME_LABELS } from "../runtimeLogos";
import { loadSidebarPins, pinnedFirst, toggleSidebarPin, PINNED_PROJECTS_STORAGE_KEY, PINNED_SESSIONS_STORAGE_KEY } from "../sidebarPins";

const FAVORITES_STORAGE_KEY = "crewvela-favorite-sessions";
const COLLAPSED_PROJECTS_STORAGE_KEY = "crewvela-collapsed-projects";

const STATUS_LABELS: Record<SessionStatus, string> = {
  pending: "未执行",
  running: "正在执行",
  waiting: "等待确认",
  completed: "执行完毕",
  error: "执行失败",
  stopped: "已停止",
};

type SessionFilter = "running" | "all" | "stopped" | "favorites";

interface Props {
  sessions: SessionRecord[];
  projects: Project[];
  usages: Record<string, SessionUsage>;
  selectedId: string | null;
  loadingId: string | null;
  typeFilter: SessionTypeFilter;
  onTypeFilter: (filter: SessionTypeFilter) => void;
  onSelect: (id: string | null) => void;
  onRename: (session: SessionRecord) => void;
  onStop: (session: SessionRecord) => void;
  onDelete: (session: SessionRecord) => void;
  onArchive: (session: SessionRecord) => void;
  onViewTranscript?: (session: SessionRecord) => void;
  onSwitchAgent?: (session: SessionRecord) => void;
  sourceHistory?: (session: SessionRecord) => SessionRecord | null;
  onRelayDetails?: (session: SessionRecord) => void;
  logicalSessionId?: (id: string) => string;
  onNewSessionInProject?: (project: Project) => void;
  onAddProject?: () => void;
  onRenameProject?: (project: Project) => void;
  onDeleteProject?: (project: Project) => void;
  onArchiveProjectSessions?: (project: Project) => void;
  onCollapse?: () => void;
}

function shortPath(path: string): string {
  const normalized = path.replace(/[\\/]+$/, "");
  return normalized.split(/[\\/]/).pop() || path;
}

function sessionType(session: SessionRecord): "claude" | "terminal" {
  return session.agentRuntimeKind ? "claude" : session.kind;
}

function loadFavorites(): Set<string> {
  try {
    const value = JSON.parse(window.localStorage.getItem(FAVORITES_STORAGE_KEY) ?? "[]");
    return new Set(Array.isArray(value) ? value.filter((item) => typeof item === "string") : []);
  } catch {
    return new Set();
  }
}

function loadCollapsedProjects(): Set<string> {
  try {
    const value = JSON.parse(
      window.localStorage.getItem(COLLAPSED_PROJECTS_STORAGE_KEY) ?? "[]",
    );
    return new Set(Array.isArray(value) ? value.filter((item) => typeof item === "string") : []);
  } catch {
    return new Set();
  }
}

function loadPins(key: string): Set<string> {
  try { return loadSidebarPins(key, window.localStorage); }
  catch { return new Set(); }
}

export function SessionList({
  sessions,
  projects,
  usages,
  selectedId,
  loadingId,
  typeFilter,
  onTypeFilter,
  onSelect,
  onRename,
  onStop,
  onDelete,
  onArchive,
  onViewTranscript,
  onSwitchAgent,
  sourceHistory,
  onRelayDetails,
  logicalSessionId = (id) => id,
  onNewSessionInProject,
  onAddProject,
  onRenameProject,
  onDeleteProject,
  onArchiveProjectSessions,
  onCollapse,
}: Props) {
  const [filter, setFilter] = useState<SessionFilter>("running");
  const [filterMenuOpen, setFilterMenuOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [favorites, setFavorites] = useState<Set<string>>(loadFavorites);
  const [pinnedSessions, setPinnedSessions] = useState(() => loadPins(PINNED_SESSIONS_STORAGE_KEY));
  const [pinnedProjects, setPinnedProjects] = useState(() => loadPins(PINNED_PROJECTS_STORAGE_KEY));
  const [pinSaveError, setPinSaveError] = useState("");
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(loadCollapsedProjects);
  const [actionMenuSessionId, setActionMenuSessionId] = useState<string | null>(null);
  const [actionMenuPosition, setActionMenuPosition] = useState({ left: 8, top: 8, maxHeight: 380 });
  const [actionMenuInitialFocusIndex, setActionMenuInitialFocusIndex] = useState(0);
  const filterSelectRef = useRef<HTMLDivElement>(null);
  const filterTriggerRef = useRef<HTMLButtonElement>(null);
  const actionMenuTriggerRef = useRef<HTMLButtonElement | null>(null);
  const actionMenuRef = useRef<HTMLDivElement | null>(null);
  const actionMenuItemsRef = useRef<HTMLButtonElement[]>([]);
  const liveCount = sessions.filter(isSessionActive).length;
  const stoppedCount = sessions.length - liveCount;
  const favoriteCount = sessions.filter((session) => favorites.has(logicalSessionId(session.id))).length;
  const claudeCount = sessions.filter((session) => sessionType(session) === "claude").length;
  const terminalCount = sessions.filter((session) => sessionType(session) === "terminal").length;

  useEffect(() => {
    try {
      window.localStorage.setItem(FAVORITES_STORAGE_KEY, JSON.stringify([...favorites]));
    } catch {
      // 当前窗口内仍可正常收藏。
    }
  }, [favorites]);

  useEffect(() => {
    try {
      window.localStorage.setItem(
        COLLAPSED_PROJECTS_STORAGE_KEY,
        JSON.stringify([...collapsedProjects]),
      );
    } catch {
      // 当前窗口内仍可正常折叠。
    }
  }, [collapsedProjects]);

  useEffect(() => {
    if (!filterMenuOpen) return;

    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!filterSelectRef.current?.contains(event.target as Node)) {
        setFilterMenuOpen(false);
      }
    };

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setFilterMenuOpen(false);
      filterTriggerRef.current?.focus();
    };

    document.addEventListener("pointerdown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [filterMenuOpen]);

  const closeActionMenu = (restoreFocus = false) => {
    const trigger = actionMenuTriggerRef.current;
    setActionMenuSessionId(null);
    if (restoreFocus && trigger) {
      window.requestAnimationFrame(() => {
        if (
          trigger.isConnected
          && (document.activeElement === document.body || !document.activeElement?.isConnected)
        ) trigger.focus();
      });
    }
  };

  const openActionMenu = (sessionId: string, trigger: HTMLButtonElement, focusLast = false) => {
    const row = trigger.closest(".session-item");
    const rect = row?.getBoundingClientRect() ?? trigger.getBoundingClientRect();
    const sidebarRect = trigger.closest(".sidebar, .session-section")?.getBoundingClientRect() ?? rect;
    actionMenuTriggerRef.current = trigger;
    actionMenuItemsRef.current = [];
    setActionMenuInitialFocusIndex(focusLast ? -1 : 0);
    setActionMenuPosition({
      left: sidebarRect.right + 8,
      top: Math.max(8, rect.top),
      maxHeight: Math.max(80, window.innerHeight - 16),
    });
    setActionMenuSessionId(sessionId);
  };

  const handleActionMenuTriggerKeyDown = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    sessionId: string,
  ) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp" && event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
    event.preventDefault();
    event.stopPropagation();
    openActionMenu(sessionId, event.currentTarget, event.key === "ArrowUp");
  };

  useLayoutEffect(() => {
    if (!actionMenuSessionId) return;
    const trigger = actionMenuTriggerRef.current;
    const menu = actionMenuRef.current;
    if (!trigger || !menu) {
      setActionMenuSessionId(null);
      return;
    }

    const placeMenu = () => {
      const rowRect = trigger.closest(".session-item")?.getBoundingClientRect() ?? trigger.getBoundingClientRect();
      const sidebarRect = trigger.closest(".sidebar")?.getBoundingClientRect()
        ?? trigger.closest(".session-section")?.getBoundingClientRect()
        ?? rowRect;
      const menuRect = menu.getBoundingClientRect();
      const edge = 8;
      const gap = 8;
      const maxHeight = Math.max(0, Math.min(380, window.innerHeight - edge * 2));
      const menuHeight = Math.min(menuRect.height, maxHeight);
      const maxLeft = window.innerWidth - edge - menuRect.width;
      const rightLeft = sidebarRect.right + gap;
      const fallbackLeft = rowRect.left - gap - menuRect.width;
      const left = Math.max(edge, Math.min(maxLeft, rightLeft <= maxLeft ? rightLeft : fallbackLeft));
      const top = Math.max(edge, Math.min(rowRect.top, window.innerHeight - edge - menuHeight));
      setActionMenuPosition((current) => (
        current.left === left && current.top === top && current.maxHeight === maxHeight
          ? current
          : { left, top, maxHeight }
      ));
    };

    placeMenu();
    const focusIndex = actionMenuInitialFocusIndex < 0
      ? actionMenuItemsRef.current.length - 1
      : actionMenuInitialFocusIndex;
    const focusFrame = window.requestAnimationFrame(() => actionMenuItemsRef.current[focusIndex]?.focus());
    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!(event.target instanceof Node)) return;
      if (!menu.contains(event.target) && !trigger.contains(event.target)) closeActionMenu();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeActionMenu(true);
    };

    window.addEventListener("resize", placeMenu);
    document.addEventListener("scroll", placeMenu, true);
    document.addEventListener("pointerdown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      window.cancelAnimationFrame(focusFrame);
      window.removeEventListener("resize", placeMenu);
      document.removeEventListener("scroll", placeMenu, true);
      document.removeEventListener("pointerdown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [actionMenuSessionId, actionMenuInitialFocusIndex]);

  const handleActionMenuKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("[role='menuitem']:not(:disabled)"));
    const currentIndex = items.indexOf(document.activeElement as HTMLButtonElement);
    let nextIndex = -1;
    if (items.length && event.key === "ArrowDown") nextIndex = (currentIndex + 1 + items.length) % items.length;
    else if (items.length && event.key === "ArrowUp") nextIndex = (currentIndex - 1 + items.length) % items.length;
    else if (items.length && event.key === "Home") nextIndex = 0;
    else if (items.length && event.key === "End") nextIndex = items.length - 1;
    else if (event.key === "Tab") {
      const trigger = actionMenuTriggerRef.current;
      closeActionMenu();
      trigger?.focus();
      return;
    }
    if (nextIndex >= 0) {
      event.preventDefault();
      items[nextIndex]?.focus();
    }
  };

  const runActionMenuItem = (event: ReactMouseEvent<HTMLButtonElement>, action: () => void) => {
    event.stopPropagation();
    closeActionMenu(true);
    action();
  };

  const visibleSessions = useMemo(() => {
    const typeSessions = sessions.filter(
      (session) => typeFilter === "all" || sessionType(session) === typeFilter,
    );
    let filtered: SessionRecord[];
    if (filter === "running") filtered = typeSessions.filter(isSessionActive);
    else if (filter === "stopped") filtered = typeSessions.filter((session) => !isSessionActive(session));
    else if (filter === "favorites") {
      filtered = typeSessions.filter((session) => favorites.has(logicalSessionId(session.id)));
    } else {
      filtered = typeSessions;
    }
    const keyword = query.trim().toLowerCase();
    if (keyword) {
      filtered = filtered.filter((session) =>
        sessionDisplayTitle(session).toLowerCase().includes(keyword) ||
        session.cwd.toLowerCase().includes(keyword) ||
        sessionDisplayProvider(session).toLowerCase().includes(keyword),
      );
    }
    return pinnedFirst(filtered, pinnedSessions, (session) => logicalSessionId(session.id));
  }, [favorites, filter, query, sessions, typeFilter, logicalSessionId, pinnedSessions]);

  const orderedProjects = useMemo(() => pinnedFirst(projects, pinnedProjects, (project) => project.id), [projects, pinnedProjects]);

  const togglePin = (id: string, kind: "session" | "project") => {
    const next = toggleSidebarPin(kind === "session" ? pinnedSessions : pinnedProjects, id);
    if (kind === "session") setPinnedSessions(next);
    else setPinnedProjects(next);
    try {
      window.localStorage.setItem(kind === "session" ? PINNED_SESSIONS_STORAGE_KEY : PINNED_PROJECTS_STORAGE_KEY, JSON.stringify([...next]));
      setPinSaveError("");
    } catch {
      setPinSaveError("置顶状态未能保存，重启后可能丢失。请检查本地存储空间或权限。");
    }
  };

  // 仅 AI 会话视图按项目分组；终端视图没有项目归属，平铺即可。
  const groupByProject = projects.length > 0 && typeFilter !== "terminal";
  const projectGroups = useMemo(() => {
    if (!groupByProject) return null;
    const knownProjectIds = new Set(projects.map((project) => project.id));
    const byProject = new Map<string, SessionRecord[]>();
    const ungrouped: SessionRecord[] = [];
    for (const session of visibleSessions) {
      const projectId = session.projectId;
      if (projectId && knownProjectIds.has(projectId)) {
        const bucket = byProject.get(projectId);
        if (bucket) bucket.push(session);
        else byProject.set(projectId, [session]);
      } else {
        ungrouped.push(session);
      }
    }
    return { byProject, ungrouped };
  }, [groupByProject, projects, visibleSessions]);

  const selectFilter = (nextFilter: SessionFilter) => {
    setFilter(nextFilter);
    setFilterMenuOpen(false);
    if (nextFilter === "running") onSelect(null);
  };

  const toggleFavorite = (sessionId: string) => {
    setFavorites((current) => {
      const next = new Set(current);
      if (next.has(sessionId)) next.delete(sessionId);
      else next.add(sessionId);
      return next;
    });
  };

  const toggleProjectCollapsed = (projectId: string) => {
    setCollapsedProjects((current) => {
      const next = new Set(current);
      if (next.has(projectId)) next.delete(projectId);
      else next.add(projectId);
      return next;
    });
  };

  const filters: Array<{
    id: SessionFilter;
    label: string;
    count: number;
    tone: string;
  }> = [
    { id: "running", label: "运行中", count: liveCount, tone: "running" },
    { id: "all", label: "全部会话", count: sessions.length, tone: "all" },
    { id: "stopped", label: "已停止", count: stoppedCount, tone: "stopped" },
    { id: "favorites", label: "收藏", count: favoriteCount, tone: "favorite" },
  ];
  const selectedFilter = filters.find((item) => item.id === filter) ?? filters[0];

  const handleFilterMenuKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const options = Array.from(
      filterSelectRef.current?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [],
    );
    if (options.length === 0) return;

    const currentIndex = options.indexOf(document.activeElement as HTMLButtonElement);
    let nextIndex = currentIndex;
    if (event.key === "ArrowDown") nextIndex = (currentIndex + 1) % options.length;
    else if (event.key === "ArrowUp") {
      nextIndex = currentIndex <= 0 ? options.length - 1 : currentIndex - 1;
    } else if (event.key === "Home") nextIndex = 0;
    else if (event.key === "End") nextIndex = options.length - 1;
    else return;

    event.preventDefault();
    options[nextIndex]?.focus();
  };

  const renderSessionItem = (session: SessionRecord) => {
    const isFavorite = favorites.has(logicalSessionId(session.id));
    const isPinned = pinnedSessions.has(logicalSessionId(session.id));
    const usage = usages[session.id];
    const usageTotal = usage
      ? usage.inputTokens + usage.outputTokens + usage.cacheCreationTokens + usage.cacheReadTokens
      : 0;
    return (
      <div
        key={session.id}
        className={`session-item${selectedId === session.id ? " active" : ""}`}
        onClick={() => onSelect(session.id)}
      >
        <div className="session-title-row">
          {session.agentRuntimeKind ? (
            <span
              className="session-kind-icon agent"
              aria-label={RUNTIME_LABELS[session.agentRuntimeKind] ?? RUNTIME_LABELS.unsupported}
              title={RUNTIME_LABELS[session.agentRuntimeKind] ?? RUNTIME_LABELS.unsupported}
            >
              <AgentAvatar avatarId={session.agentAvatarId} runtimeKind={session.agentRuntimeKind} decorative />
            </span>
          ) : session.kind === "terminal" && (
            <span className="session-kind-icon terminal" aria-hidden="true">
              <TerminalWindowIcon size={15} />
            </span>
          )}
          <span className={`status-dot ${session.status}`} />
          <span className="session-title">
            {loadingId === session.id ? "正在恢复…" : sessionDisplayTitle(session)}
          </span>
          {isPinned && <PushPinIcon className="session-pin-mark" size={12} weight="fill" aria-label="已置顶会话" />}
          {isFavorite && <StarIcon className="session-favorite-mark" size={12} weight="fill" />}
        </div>
        <div className="session-meta">
          <span>{STATUS_LABELS[session.status]}</span>
          <span>·</span>
          <span>{session.agentRuntimeKind === "oh_my_pi" ? RUNTIME_LABELS.oh_my_pi : sessionDisplayProvider(session)}</span>
          <span>·</span>
          <span title={session.cwd}>{shortPath(session.cwd)}</span>
          {usage !== undefined && usageTotal > 0 && (
            <>
              <span>·</span>
              <span
                className="session-usage"
                title={`token 用量\n输入 ${usage.inputTokens.toLocaleString()} · 输出 ${usage.outputTokens.toLocaleString()}\n缓存写入 ${usage.cacheCreationTokens.toLocaleString()} · 缓存命中 ${usage.cacheReadTokens.toLocaleString()}`}
              >
                {formatTokens(usageTotal)} tok
              </span>
            </>
          )}
          {session.worktree && (
            <>
              <span>·</span>
              <span
                title={`独立 Worktree\n分支：${session.worktree.branch}\n路径：${session.worktree.path}`}
              >
                ⎇ worktree
              </span>
            </>
          )}
        </div>
        <div className="session-actions">
          <button
            ref={actionMenuSessionId === session.id ? actionMenuTriggerRef : undefined}
            type="button"
            className="session-actions-trigger"
            aria-label={`${sessionDisplayTitle(session)}更多操作`}
            title="更多操作"
            aria-haspopup="menu"
            aria-expanded={actionMenuSessionId === session.id}
            aria-controls={actionMenuSessionId === session.id ? "session-action-menu" : undefined}
            onClick={(event) => {
              event.stopPropagation();
              if (actionMenuSessionId === session.id) closeActionMenu();
              else openActionMenu(session.id, event.currentTarget);
            }}
            onKeyDown={(event) => handleActionMenuTriggerKeyDown(event, session.id)}
          >
            <DotsThreeIcon size={18} weight="bold" aria-hidden="true" />
          </button>
        </div>
      </div>
    );
  };

  const renderProjectGroup = (project: Project, groupSessions: SessionRecord[]) => {
    const collapsed = collapsedProjects.has(project.id);
    return (
      <div className="session-project-group" key={project.id}>
        <div className="session-project-head">
          <button
            type="button"
            className="session-project-toggle"
            aria-expanded={!collapsed}
            title={collapsed ? "展开项目会话" : "折叠项目会话"}
            onClick={() => toggleProjectCollapsed(project.id)}
          >
            {collapsed ? <CaretRightIcon size={13} /> : <CaretDownIcon size={13} />}
            {collapsed ? (
              <FolderIcon
                className="session-project-folder"
                size={15}
               
              />
            ) : (
              <FolderOpenIcon
                className="session-project-folder"
                size={15}
               
              />
            )}
          </button>
          <span className="session-project-name" title={project.path}>
            {project.name}
          </span>
          {pinnedProjects.has(project.id) && <PushPinIcon className="session-pin-mark" size={12} weight="fill" aria-label="已置顶项目" />}
          <span className="section-count">{groupSessions.length}</span>
          <div className="session-project-actions">
            <button
              type="button"
              title={pinnedProjects.has(project.id) ? "取消置顶项目" : "置顶项目"}
              aria-label={`${pinnedProjects.has(project.id) ? "取消置顶项目" : "置顶项目"}「${project.name}」`}
              aria-pressed={pinnedProjects.has(project.id)}
              onClick={() => togglePin(project.id, "project")}
            >
              <PushPinIcon size={13} weight={pinnedProjects.has(project.id) ? "fill" : "regular"} />
            </button>
            {onNewSessionInProject && (
              <button
                type="button"
                title={`在「${project.name}」下新建会话`}
                onClick={() => onNewSessionInProject(project)}
              >
                <PlusIcon size={13} weight="bold" />
              </button>
            )}
            {onRenameProject && (
              <button
                type="button"
                title="重命名项目"
                onClick={() => onRenameProject(project)}
              >
                <PencilSimpleIcon size={12} />
              </button>
            )}
            {onDeleteProject && (
              <button
                type="button"
                title="删除项目（会话保留并回到未分组）"
                onClick={() => onDeleteProject(project)}
              >
                <TrashIcon size={12} />
              </button>
            )}
            {onArchiveProjectSessions && (
              <button
                type="button"
                title={`归档「${project.name}」下的全部会话`}
                onClick={() => onArchiveProjectSessions(project)}
              >
                <ArchiveIcon size={12} />
              </button>
            )}
          </div>
        </div>
        {!collapsed && groupSessions.map(renderSessionItem)}
        {!collapsed && groupSessions.length === 0 && (
          <div className="session-project-empty">暂无会话，点 + 在项目下新建</div>
        )}
      </div>
    );
  };

  const actionMenuSession = actionMenuSessionId
    ? sessions.find((session) => session.id === actionMenuSessionId) ?? null
    : null;
  const actionMenuItems: Array<{
    label: string;
    icon: ReactNode;
    action: () => void;
    danger?: boolean;
    dividerBefore?: boolean;
  }> = [];
  if (actionMenuSession) {
    const menuSessionIsPinned = pinnedSessions.has(logicalSessionId(actionMenuSession.id));
    actionMenuItems.push({
      label: menuSessionIsPinned ? "取消置顶会话" : "置顶会话",
      icon: <PushPinIcon size={15} weight={menuSessionIsPinned ? "fill" : "regular"} aria-hidden="true" />,
      action: () => togglePin(logicalSessionId(actionMenuSession.id), "session"),
    });
    const history = sourceHistory?.(actionMenuSession);
    if (onSwitchAgent && sessionSwitchReason(actionMenuSession) === null) {
      actionMenuItems.push({
        label: "将当前工作交接给其他 Agent",
        icon: <span className="session-action-symbol session-action-symbol--handoff" aria-hidden="true" />,
        action: () => onSwitchAgent(actionMenuSession),
      });
    }
    if (history && onViewTranscript) {
      actionMenuItems.push({
        label: "回看接力历史",
        icon: <FileTextIcon size={15} aria-hidden="true" />,
        action: () => onViewTranscript(history),
      });
    }
    if (history && onRelayDetails) {
      actionMenuItems.push({
        label: "接力详情与恢复原会话",
        icon: <span className="session-action-symbol session-action-symbol--handoff" aria-hidden="true" />,
        action: () => onRelayDetails(actionMenuSession),
      });
    }
    if (onViewTranscript && actionMenuSession.kind === "claude") {
      actionMenuItems.push({
        label: "查看历史记录",
        icon: <FileTextIcon size={15} aria-hidden="true" />,
        action: () => onViewTranscript(actionMenuSession),
      });
    }
    const menuSessionIsFavorite = favorites.has(logicalSessionId(actionMenuSession.id));
    actionMenuItems.push({
      label: menuSessionIsFavorite ? "取消收藏" : "收藏",
      icon: <StarIcon size={15} weight={menuSessionIsFavorite ? "fill" : "regular"} aria-hidden="true" />,
      action: () => toggleFavorite(logicalSessionId(actionMenuSession.id)),
    });
    actionMenuItems.push({
      label: "重命名",
      icon: <PencilSimpleIcon size={15} aria-hidden="true" />,
      action: () => onRename(actionMenuSession),
    });
    if (isSessionActive(actionMenuSession)) {
      actionMenuItems.push({
        label: "停止",
        icon: <StopIcon size={15} weight="fill" aria-hidden="true" />,
        action: () => onStop(actionMenuSession),
      });
    }
    actionMenuItems.push({
      label: "归档会话",
      icon: <ArchiveIcon size={15} aria-hidden="true" />,
      action: () => onArchive(actionMenuSession),
    });
    actionMenuItems.push({
      label: "从列表删除",
      icon: <TrashIcon size={15} aria-hidden="true" />,
      action: () => onDelete(actionMenuSession),
      danger: true,
      dividerBefore: true,
    });
  }

  return (
    <>
    <section className="session-section">
      <div className="sidebar-section-head">
        <span>会话</span>
        <span className="section-count">{sessions.length}</span>
        {onCollapse && (
          <button
            className="btn-icon sidebar-section-collapse"
            type="button"
            aria-label="收起会话列表"
            title="收起会话列表"
            onClick={onCollapse}
          >
            <CaretDoubleLeftIcon size={16} />
          </button>
        )}
      </div>

      <div className="session-search">
        <MagnifyingGlassIcon size={13} aria-hidden="true" />
        <input
          value={query}
          placeholder="搜索标题 / 目录 / provider"
          spellCheck={false}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape" && query) {
              event.stopPropagation();
              setQuery("");
            }
          }}
        />
        {query && (
          <button
            type="button"
            aria-label="清空搜索"
            title="清空搜索"
            onClick={() => setQuery("")}
          >
            ×
          </button>
        )}
      </div>

      <div className="session-type-filters" role="group" aria-label="会话类型">
        {([
          ["all", "全部", sessions.length],
          ["claude", "AI 会话", claudeCount],
          ["terminal", "终端", terminalCount],
        ] as const).map(([id, label, count]) => (
          <button
            key={id}
            type="button"
            className={typeFilter === id ? "active" : ""}
            aria-pressed={typeFilter === id}
            onClick={() => onTypeFilter(id)}
          >
            {label}
            <small>{count}</small>
          </button>
        ))}
      </div>

      <div
        ref={filterSelectRef}
        className={`session-filter-select${filterMenuOpen ? " open" : ""}`}
        onKeyDown={handleFilterMenuKeyDown}
      >
        <button
          ref={filterTriggerRef}
          className="session-filter-trigger"
          type="button"
          aria-label={`会话状态：${selectedFilter.label}`}
          aria-haspopup="listbox"
          aria-expanded={filterMenuOpen}
          onClick={() => {
            if (filterMenuOpen) setFilterMenuOpen(false);
            else setFilterMenuOpen(true);
          }}
        >
          {selectedFilter.id === "favorites" ? (
            <StarIcon size={15} weight="regular" />
          ) : (
            <CircleIcon
              size={11}
              weight={selectedFilter.id === "running" ? "fill" : "regular"}
            />
          )}
          <span>{selectedFilter.label}</span>
          <strong className={`filter-count ${selectedFilter.tone}`}>
            {selectedFilter.count}
          </strong>
          <CaretDownIcon className="session-filter-caret" size={14} weight="bold" />
        </button>

        {filterMenuOpen && (
          <div className="session-filter-menu" role="listbox" aria-label="会话状态筛选">
            {filters.map((item) => (
              <button
                key={item.id}
                className={`session-filter-option${filter === item.id ? " active" : ""}`}
                type="button"
                role="option"
                aria-selected={filter === item.id}
                data-filter={item.id}
                onClick={() => {
                  selectFilter(item.id);
                  filterTriggerRef.current?.focus();
                }}
              >
                {item.id === "favorites" ? (
                  <StarIcon size={15} weight={filter === item.id ? "fill" : "regular"} />
                ) : (
                  <CircleIcon size={10} weight={item.id === "running" ? "fill" : "regular"} />
                )}
                <span>{item.label}</span>
                <strong className={`filter-count ${item.tone}`}>{item.count}</strong>
              </button>
            ))}
          </div>
        )}
      </div>

      {!groupByProject && (
        <div className="session-list-heading">
          <span>{filters.find((item) => item.id === filter)?.label}</span>
          <small>{visibleSessions.length} 项</small>
        </div>
      )}

      {groupByProject && (
        <div className="session-projects-toolbar">
          <span>项目</span>
          {onAddProject && (
            <button
              type="button"
              aria-label="添加项目"
              title="添加项目"
              onClick={onAddProject}
            >
              <PlusIcon size={14} weight="bold" />
            </button>
          )}
        </div>
      )}

      <div className="session-list">
        {pinSaveError && <p className="session-pin-error" role="alert">{pinSaveError}</p>}
        {visibleSessions.length === 0 && projects.length === 0 && (
          <div className="session-empty">
            {filter === "favorites" ? "暂无收藏会话" : "暂无会话"}
          </div>
        )}
        {visibleSessions.length === 0 && projects.length > 0 && query.trim() && (
          <div className="session-empty">没有匹配「{query.trim()}」的会话</div>
        )}
        {groupByProject && projectGroups ? (
          <>
            {orderedProjects.map((project) => {
              const groupSessions = projectGroups.byProject.get(project.id) ?? [];
              // 搜索时隐藏空组，减少噪音。
              if (query.trim() && groupSessions.length === 0) return null;
              return renderProjectGroup(project, groupSessions);
            })}
            {projectGroups.ungrouped.length > 0 && (
              <div className="session-project-group">
                <div className="session-project-head session-project-head-plain">
                  <span className="session-project-name">未分组</span>
                  <span className="section-count">{projectGroups.ungrouped.length}</span>
                </div>
                {projectGroups.ungrouped.map(renderSessionItem)}
              </div>
            )}
          </>
        ) : (
          visibleSessions.map(renderSessionItem)
        )}
      </div>
    </section>
    {actionMenuSession && createPortal(
      <div
        id="session-action-menu"
        ref={actionMenuRef}
        className="session-action-menu"
        role="menu"
        aria-label={`${sessionDisplayTitle(actionMenuSession)}操作`}
        style={{
          left: actionMenuPosition.left,
          top: actionMenuPosition.top,
          maxHeight: actionMenuPosition.maxHeight,
        }}
        onKeyDown={handleActionMenuKeyDown}
      >
        {actionMenuItems.map((item, index) => (
          <button
            key={item.label}
            ref={(node) => {
              if (node) actionMenuItemsRef.current[index] = node;
            }}
            type="button"
            role="menuitem"
            tabIndex={-1}
            className={`session-action-menu-item${item.danger ? " danger" : ""}${item.dividerBefore ? " has-divider-before" : ""}`}
            onClick={(event) => runActionMenuItem(event, item.action)}
          >
            {item.icon}
            <span>{item.label}</span>
          </button>
        ))}
      </div>,
      document.body,
    )}
    </>
  );
}

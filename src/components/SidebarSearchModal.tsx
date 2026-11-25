import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { ArrowElbowDownLeftIcon } from "@phosphor-icons/react/dist/csr/ArrowElbowDownLeft";
import { ArchiveIcon } from "@phosphor-icons/react/dist/csr/Archive";
import { ChatCircleTextIcon } from "@phosphor-icons/react/dist/csr/ChatCircleText";
import { DotsThreeIcon } from "@phosphor-icons/react/dist/csr/DotsThree";
import { FolderIcon } from "@phosphor-icons/react/dist/csr/Folder";
import { FolderPlusIcon } from "@phosphor-icons/react/dist/csr/FolderPlus";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { PencilSimpleIcon } from "@phosphor-icons/react/dist/csr/PencilSimple";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { StopIcon } from "@phosphor-icons/react/dist/csr/Stop";
import { TerminalIcon } from "@phosphor-icons/react/dist/csr/Terminal";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { XIcon } from "@phosphor-icons/react/dist/csr/X";
import type { Project, SessionRecord } from "../types";
import { useTranslation } from "../useInterfaceLanguage";
import { isSessionActive, sessionDisplayProvider, sessionDisplayTitle } from "../sessionCapabilities";
import { AgentAvatar } from "./AgentAvatar";
import "../styles/sidebar-search-modal.css";

type SearchTab = "all" | "sessions" | "terminals" | "projects" | "actions";

interface SearchResultItem {
  id: string;
  section: "attention" | "recent" | "projects" | "actions";
  kind: "session" | "terminal" | "project" | "action";
  title: string;
  subtitle?: string;
  session?: SessionRecord;
  project?: Project;
  statusTone?: "running" | "waiting" | "error" | "idle";
  onExecute: () => void;
}

interface Props {
  open: boolean;
  onClose: () => void;
  sessions: SessionRecord[];
  projects: Project[];
  selectedSessionId: string | null;
  onSelectSession: (id: string) => void;
  onRenameSession: (session: SessionRecord) => void;
  onStopSession: (session: SessionRecord) => void;
  onDeleteSession: (session: SessionRecord) => void;
  onArchiveSession: (session: SessionRecord) => void;
  onOpenProject?: (project: Project) => void;
  onNewSessionInProject?: (project: Project) => void;
  onNewSession: () => void;
  onNewTerminal: () => void;
  onAddProject: () => void;
}

const TABS: readonly { id: SearchTab; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "sessions", label: "会话" },
  { id: "terminals", label: "终端" },
  { id: "projects", label: "项目" },
  { id: "actions", label: "操作" },
];

function shortPath(path: string): string {
  const normalized = path.replace(/[\\/]+$/, "");
  return normalized.split(/[\\/]/).pop() || path;
}

function isAiSession(session: SessionRecord): boolean {
  return Boolean(session.agentRuntimeKind) || session.kind === "claude";
}

function sessionTone(session: SessionRecord): "running" | "waiting" | "error" | "idle" {
  if (session.status === "waiting") return "waiting";
  if (session.status === "error") return "error";
  if (session.status === "running") return "running";
  return "idle";
}

export function SidebarSearchModal({
  open,
  onClose,
  sessions,
  projects,
  selectedSessionId,
  onSelectSession,
  onRenameSession,
  onStopSession,
  onDeleteSession,
  onArchiveSession,
  onOpenProject,
  onNewSessionInProject,
  onNewSession,
  onNewTerminal,
  onAddProject,
}: Props) {
  const t = useTranslation();
  const [query, setQuery] = useState("");
  const [activeTab, setActiveTab] = useState<SearchTab>("all");
  const [activeIndex, setActiveIndex] = useState(0);
  const [menuItemId, setMenuItemId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActiveTab("all");
    setActiveIndex(0);
    setMenuItemId(null);
    const frame = window.requestAnimationFrame(() => inputRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [open]);

  const projectById = useMemo(() => {
    const map = new Map<string, Project>();
    for (const p of projects) map.set(p.id, p);
    return map;
  }, [projects]);

  const sessionCountByProject = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of sessions) {
      if (s.projectId) counts.set(s.projectId, (counts.get(s.projectId) ?? 0) + 1);
    }
    return counts;
  }, [sessions]);

  const grouped = useMemo(() => {
    const kw = query.trim().toLocaleLowerCase();
    const matchesSession = (session: SessionRecord) => {
      if (!kw) return true;
      const title = sessionDisplayTitle(session).toLocaleLowerCase();
      const provider = sessionDisplayProvider(session).toLocaleLowerCase();
      const cwd = session.cwd.toLocaleLowerCase();
      const proj = session.projectId ? (projectById.get(session.projectId)?.name.toLocaleLowerCase() ?? "") : "";
      return title.includes(kw) || provider.includes(kw) || cwd.includes(kw) || proj.includes(kw);
    };

    const sortedSessions = [...sessions].sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
    const filteredSessions = sortedSessions.filter((s) => {
      if (activeTab === "sessions" && !isAiSession(s)) return false;
      if (activeTab === "terminals" && isAiSession(s)) return false;
      if (activeTab === "projects" || activeTab === "actions") return false;
      return matchesSession(s);
    });

    const toSessionItem = (
      session: SessionRecord,
      section: "attention" | "recent",
    ): SearchResultItem => {
      const ai = isAiSession(session);
      const title = sessionDisplayTitle(session);
      const projName = session.projectId ? projectById.get(session.projectId)?.name : undefined;
      const provider = sessionDisplayProvider(session);
      const location = projName || shortPath(session.cwd);
      const metaParts = [
        provider && provider.toLocaleLowerCase() !== title.toLocaleLowerCase() ? provider : "",
        location,
      ].filter(Boolean);
      return {
        id: `${section}:${session.id}`,
        section,
        kind: ai ? "session" : "terminal",
        title,
        subtitle: metaParts.join(" · "),
        session,
        statusTone: sessionTone(session),
        onExecute: () => {
          onSelectSession(session.id);
          onClose();
        },
      };
    };

    const attention: SearchResultItem[] = [];
    const recent: SearchResultItem[] = [];
    for (const s of filteredSessions) {
      if (s.status === "waiting" || s.status === "error") {
        attention.push(toSessionItem(s, "attention"));
      } else {
        recent.push(toSessionItem(s, "recent"));
      }
    }

    const projectItems: SearchResultItem[] =
      activeTab === "all" || activeTab === "projects"
        ? projects
            .filter((p) => !kw || p.name.toLocaleLowerCase().includes(kw) || p.path.toLocaleLowerCase().includes(kw))
            .map((project) => {
              const count = sessionCountByProject.get(project.id) ?? 0;
              const folder = shortPath(project.path);
              const subParts = [
                folder.toLocaleLowerCase() !== project.name.toLocaleLowerCase() ? folder : "",
                `${count} ${t("会话")}`,
              ].filter(Boolean);
              return {
                id: `project:${project.id}`,
                section: "projects" as const,
                kind: "project" as const,
                title: project.name,
                subtitle: subParts.join(" · "),
                project,
                onExecute: () => {
                  if (onOpenProject) onOpenProject(project);
                  else onNewSessionInProject?.(project);
                  onClose();
                },
              };
            })
        : [];

    const allActions: SearchResultItem[] = [
      {
        id: "action:new-session",
        section: "actions",
        kind: "action",
        title: t("新建会话"),
        subtitle: "Agent / CLI",
        onExecute: () => {
          onClose();
          onNewSession();
        },
      },
      {
        id: "action:new-terminal",
        section: "actions",
        kind: "action",
        title: t("新建终端"),
        subtitle: "Shell",
        onExecute: () => {
          onClose();
          onNewTerminal();
        },
      },
      {
        id: "action:add-project",
        section: "actions",
        kind: "action",
        title: t("添加项目"),
        subtitle: t("选择其他目录..."),
        onExecute: () => {
          onClose();
          onAddProject();
        },
      },
    ];

    const actionItems =
      activeTab === "all" || activeTab === "actions"
        ? allActions.filter(
            (a) =>
              !kw ||
              a.title.toLocaleLowerCase().includes(kw) ||
              (a.subtitle?.toLocaleLowerCase().includes(kw) ?? false),
          )
        : [];

    const recentSlice = activeTab === "all" && !kw ? recent.slice(0, 8) : recent;
    const projectSlice = activeTab === "all" && !kw ? projectItems.slice(0, 5) : projectItems;

    const sections: { key: SearchResultItem["section"]; label: string; items: SearchResultItem[] }[] = [];
    if (attention.length) sections.push({ key: "attention", label: t("需要关注"), items: attention });
    if (recentSlice.length) {
      sections.push({
        key: "recent",
        label: activeTab === "terminals" ? t("终端") : activeTab === "sessions" ? t("会话") : t("最近使用"),
        items: recentSlice,
      });
    }
    if (projectSlice.length) sections.push({ key: "projects", label: t("项目"), items: projectSlice });
    if (actionItems.length) sections.push({ key: "actions", label: t("操作"), items: actionItems });

    const flat = sections.flatMap((sec) => sec.items);
    return { sections, flat };
  }, [
    query,
    activeTab,
    sessions,
    projects,
    projectById,
    sessionCountByProject,
    onSelectSession,
    onOpenProject,
    onNewSessionInProject,
    onNewSession,
    onNewTerminal,
    onAddProject,
    onClose,
    t,
  ]);

  useEffect(() => {
    setActiveIndex(0);
    setMenuItemId(null);
  }, [query, activeTab]);

  useEffect(() => {
    if (activeIndex >= grouped.flat.length) {
      setActiveIndex(Math.max(0, grouped.flat.length - 1));
    }
  }, [activeIndex, grouped.flat.length]);

  useEffect(() => {
    if (!open) return;
    const activeEl = listRef.current?.querySelector<HTMLElement>("[data-active='true']");
    activeEl?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, open]);

  if (!open) return null;

  const stepTab = (delta: number) => {
    const idx = TABS.findIndex((tab) => tab.id === activeTab);
    const next = TABS[(idx + delta + TABS.length) % TABS.length];
    if (next) setActiveTab(next.id);
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      if (menuItemId) setMenuItemId(null);
      else onClose();
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setMenuItemId(null);
      if (grouped.flat.length) setActiveIndex((i) => (i + 1) % grouped.flat.length);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setMenuItemId(null);
      if (grouped.flat.length) setActiveIndex((i) => (i - 1 + grouped.flat.length) % grouped.flat.length);
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      stepTab(event.shiftKey ? -1 : 1);
      return;
    }
    if ((event.key === "ArrowLeft" || event.key === "ArrowRight") && !query) {
      event.preventDefault();
      stepTab(event.key === "ArrowRight" ? 1 : -1);
      return;
    }
    if (event.key === "Enter") {
      const current = grouped.flat[activeIndex];
      if (!current) return;
      event.preventDefault();
      if (event.altKey && current.session) {
        setMenuItemId((prev) => (prev === current.id ? null : current.id));
      } else {
        current.onExecute();
      }
    }
  };

  let flatCursor = -1;

  return createPortal(
    <div
      className="sidebar-search-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="sidebar-search-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t("搜索")}
        onKeyDown={handleKeyDown}
      >
        <div className="ssm-header">
          <MagnifyingGlassIcon size={17} className="ssm-search-icon" aria-hidden="true" />
          <input
            ref={inputRef}
            type="text"
            className="ssm-input"
            value={query}
            placeholder={t("搜索会话、终端或项目…")}
            aria-label={t("搜索")}
            spellCheck={false}
            onChange={(event) => setQuery(event.target.value)}
          />
          {query && (
            <button
              type="button"
              className="ssm-clear-btn"
              aria-label={t("清空搜索")}
              onClick={() => {
                setQuery("");
                inputRef.current?.focus();
              }}
            >
              {t("清空")}
            </button>
          )}
          <button
            type="button"
            className="ssm-close-btn"
            aria-label={t("关闭搜索")}
            onClick={onClose}
          >
            <XIcon size={16} aria-hidden="true" />
          </button>
        </div>

        <div className="ssm-tabs" role="tablist" aria-label={t("搜索分类")}>
          {TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={activeTab === tab.id}
              className={activeTab === tab.id ? "active" : ""}
              onClick={() => {
                setActiveTab(tab.id);
                inputRef.current?.focus();
              }}
            >
              {t(tab.label)}
            </button>
          ))}
        </div>

        <div className="ssm-body" ref={listRef}>
          {grouped.sections.length === 0 ? (
            <div className="ssm-empty">
              <MagnifyingGlassIcon size={24} aria-hidden="true" />
              <span>{t("没有找到匹配结果")}</span>
            </div>
          ) : (
            grouped.sections.map((section) => (
              <section className="ssm-group" key={section.key}>
                <div className="ssm-group-label">{section.label}</div>
                {section.items.map((item) => {
                  flatCursor += 1;
                  const idx = flatCursor;
                  const isActive = idx === activeIndex;
                  const isSelectedSession = Boolean(item.session && item.session.id === selectedSessionId);
                  const menuOpen = menuItemId === item.id && Boolean(item.session);
                  return (
                    <div
                      key={item.id}
                      className={`ssm-row${isActive ? " is-active" : ""}${isSelectedSession ? " is-current" : ""}`}
                      data-active={isActive ? "true" : undefined}
                      onMouseEnter={() => setActiveIndex(idx)}
                    >
                      <button
                        type="button"
                        className="ssm-row-main"
                        onClick={item.onExecute}
                      >
                        <span className="ssm-row-icon" aria-hidden="true">
                          {item.session ? (
                            item.kind === "terminal" ? (
                              <TerminalIcon size={16} />
                            ) : item.session.agentRuntimeKind ? (
                              <AgentAvatar
                                avatarId={item.session.agentAvatarId ?? undefined}
                                runtimeKind={item.session.agentRuntimeKind}
                                decorative
                              />
                            ) : (
                              <ChatCircleTextIcon size={16} />
                            )
                          ) : item.kind === "project" ? (
                            <FolderIcon size={16} />
                          ) : item.id === "action:add-project" ? (
                            <FolderPlusIcon size={16} />
                          ) : item.id === "action:new-terminal" ? (
                            <TerminalIcon size={16} />
                          ) : (
                            <PlusIcon size={16} />
                          )}
                          {item.statusTone && item.statusTone !== "idle" && (
                            <span className={`ssm-status-dot is-${item.statusTone}`} />
                          )}
                        </span>
                        <span className="ssm-row-copy">
                          <span className="ssm-row-title">{item.title}</span>
                          {item.subtitle && <span className="ssm-row-sub">{item.subtitle}</span>}
                        </span>
                      </button>

                      <span className="ssm-row-trail">
                        {item.session && (
                          <button
                            type="button"
                            className={`ssm-row-more${menuOpen ? " is-open" : ""}`}
                            aria-label={t("更多操作")}
                            aria-expanded={menuOpen}
                            onClick={(event) => {
                              event.stopPropagation();
                              setMenuItemId((prev) => (prev === item.id ? null : item.id));
                            }}
                          >
                            <DotsThreeIcon size={16} weight="bold" aria-hidden="true" />
                          </button>
                        )}
                        <span className="ssm-row-enter" aria-hidden="true">
                          <ArrowElbowDownLeftIcon size={13} />
                        </span>
                      </span>

                      {menuOpen && item.session && (
                        <div className="ssm-row-menu" role="menu">
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              onClose();
                              onRenameSession(item.session!);
                            }}
                          >
                            <PencilSimpleIcon size={13} aria-hidden="true" />
                            <span>{t("重命名")}</span>
                          </button>
                          {isSessionActive(item.session) && (
                            <button
                              type="button"
                              role="menuitem"
                              onClick={() => {
                                onClose();
                                onStopSession(item.session!);
                              }}
                            >
                              <StopIcon size={13} aria-hidden="true" />
                              <span>{t("停止")}</span>
                            </button>
                          )}
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              onClose();
                              onArchiveSession(item.session!);
                            }}
                          >
                            <ArchiveIcon size={13} aria-hidden="true" />
                            <span>{t("归档")}</span>
                          </button>
                          <button
                            type="button"
                            role="menuitem"
                            className="is-danger"
                            onClick={() => {
                              onClose();
                              onDeleteSession(item.session!);
                            }}
                          >
                            <TrashIcon size={13} aria-hidden="true" />
                            <span>{t("删除")}</span>
                          </button>
                        </div>
                      )}
                    </div>
                  );
                })}
              </section>
            ))
          )}
        </div>

        <footer className="ssm-footer" aria-hidden="true">
          <div className="ssm-footer-group">
            <span>{t("关闭")} <kbd>Esc</kbd></span>
            <span>{t("切换类型")} <kbd>←</kbd> <kbd>→</kbd></span>
            <span>{t("移动")} <kbd>↑</kbd> <kbd>↓</kbd></span>
            <span>{t("打开")} <kbd>↵</kbd></span>
          </div>
          <div className="ssm-footer-group">
            <span>{t("切换")} <kbd>Tab</kbd></span>
          </div>
        </footer>
      </div>
    </div>,
    document.body,
  );
}

import { useEffect, useId, useRef, useState, type CSSProperties, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { CaretDown, DotsThree, FolderSimple, MagnifyingGlass, PencilSimple, Plus, SidebarSimple } from "@phosphor-icons/react";
import { loadMultiAgentRailCollapsed, saveMultiAgentRailCollapsed } from "../appearance";
import type { MultiAgentProject } from "../types";
import { Modal } from "./ui/Modal";
import "../styles/collaboration-project-navigation.css";

type Activity = MultiAgentProject["status"] | "pending_approval" | "needs_attention" | "running" | "delivered";
interface ProjectActivity { project: MultiAgentProject; activity: Activity }

const labels: Record<Activity, string> = {
  pending_approval: "待审批", needs_attention: "需处理", running: "运行中", delivered: "已交付",
  draft: "待启动", active: "空闲", paused: "已暂停", completed: "已完成", cancelled: "已取消",
};
const groups: { id: string; label: string; activities: Activity[] }[] = [
  { id: "pending_approval", label: "待审批", activities: ["pending_approval"] },
  { id: "needs_attention", label: "需处理", activities: ["needs_attention"] },
  { id: "running", label: "运行中", activities: ["running"] },
  { id: "delivered", label: "已交付", activities: ["delivered"] },
  { id: "other", label: "其他项目", activities: ["draft", "active", "paused", "completed", "cancelled"] },
];

function actionMenuPosition(bounds: DOMRect): CSSProperties {
  return {
    right: Math.max(8, window.innerWidth - bounds.right),
    ...(window.innerHeight - bounds.bottom >= 48
      ? { top: bounds.bottom + 4 } : { bottom: window.innerHeight - bounds.top + 4 }),
  };
}

interface Props {
  active: boolean;
  projects: MultiAgentProject[];
  projectId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string, name: string) => Promise<MultiAgentProject>;
}

export function CollaborationProjectNavigation({ active, projects, projectId, onSelect, onCreate, onRename }: Props) {
  const [railCollapsed, setRailCollapsed] = useState<boolean>(loadMultiAgentRailCollapsed);
  const toggleRailCollapsed = () => {
    setRailCollapsed(current => {
      const next = !current;
      saveMultiAgentRailCollapsed(next);
      return next;
    });
  };
  const id = useId();
  const menuId = useId();
  const [activity, setActivity] = useState<ProjectActivity[] | null>(null);
  const [error, setError] = useState(false);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const drawerTrigger = useRef<HTMLButtonElement>(null);
  const projectGroups = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<{ project: MultiAgentProject; anchor: HTMLButtonElement; position: CSSProperties } | null>(null);
  const renameAction = useRef<HTMLButtonElement>(null);
  const [rename, setRename] = useState<{ project: MultiAgentProject; name: string } | null>(null);
  const [renameError, setRenameError] = useState("");
  const [saving, setSaving] = useState(false);
  const inFlight = useRef(false);

  const menuAnchor = menu?.anchor;
  useEffect(() => {
    if (!menuAnchor) return;
    renameAction.current?.focus({ preventScroll: true });
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && (menuAnchor.contains(event.target) || renameAction.current?.contains(event.target))) return;
      setMenu(null);
    };
    const reposition = () => {
      const bounds = menuAnchor.getBoundingClientRect();
      const list = projectGroups.current?.getBoundingClientRect();
      const position = list && bounds.height && bounds.bottom > list.top && bounds.top < list.bottom
        ? actionMenuPosition(bounds) : null;
      setMenu(current => current?.anchor === menuAnchor ? position && { ...current, position } : current);
    };
    window.addEventListener("pointerdown", outside);
    window.addEventListener("scroll", reposition, true);
    window.addEventListener("resize", reposition);
    return () => {
      window.removeEventListener("pointerdown", outside);
      window.removeEventListener("scroll", reposition, true);
      window.removeEventListener("resize", reposition);
    };
  }, [menuAnchor]);

  useEffect(() => {
    if (!active) return;
    let disposed = false;
    let timer: number;
    const refresh = async () => {
      try {
        const next = await invoke<ProjectActivity[]>("ma_list_project_activity");
        if (disposed) return;
        setActivity(next);
        setError(false);
      } catch {
        if (!disposed) setError(true);
      } finally {
        if (!disposed) timer = window.setTimeout(refresh, 4000);
      }
    };
    void refresh();
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [active, projects]);

  const entries = activity ?? projects.map(project => ({ project, activity: project.status }));
  const trimmed = query.trim().toLocaleLowerCase();
  const results = entries.filter(({ project }) =>
    project.name.toLocaleLowerCase().includes(trimmed) || project.rootPath.toLocaleLowerCase().includes(trimmed),
  );
  const saveRename = async (event: FormEvent) => {
    event.preventDefault();
    const name = rename?.name.trim();
    if (inFlight.current || !rename || !name || name === rename.project.name) return;
    inFlight.current = true;
    setSaving(true);
    setRenameError("");
    try {
      const updated = await onRename(rename.project.id, name);
      setActivity(current => current?.map(entry => entry.project.id === updated.id
        ? { ...entry, project: { ...entry.project, name: updated.name } } : entry) ?? null);
      setRename(null);
    } catch (reason) {
      setRenameError(String(reason));
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  };

  return <nav className={`collaboration-project-navigation${railCollapsed ? " is-rail-collapsed" : ""}`}
    data-multi-agent-style="grok_noir"
    data-rail-collapsed={railCollapsed ? "true" : "false"}
    aria-label="协作项目列表">
    <button ref={drawerTrigger} type="button" className="collaboration-project-drawer" aria-controls={id} aria-expanded={drawerOpen}
      onClick={() => setDrawerOpen(value => !value)}>
      <FolderSimple size={17} aria-hidden="true" />协作项目<CaretDown size={13} aria-hidden="true" />
    </button>
    {railCollapsed && (
      <div className="collaboration-slim-rail" aria-label="极简协作项目图标轨">
        <button type="button" className="collaboration-rail-icon-btn" aria-label="展开协作项目列表" title="展开协作项目列表" onClick={toggleRailCollapsed}>
          <SidebarSimple size={16} aria-hidden="true" />
        </button>
        <button type="button" className="collaboration-rail-icon-btn is-create" aria-label="新建协作" title="新建协作" onClick={onCreate}>
          <Plus size={16} weight="bold" aria-hidden="true" />
        </button>
        <span className="collaboration-rail-divider" aria-hidden="true" />
        <div className="collaboration-rail-projects">
          {entries.map(({ project, activity: status }) => {
            const abbr = Array.from(project.name.trim())[0] ?? "协";
            return (
              <button
                key={project.id}
                type="button"
                className="collaboration-rail-proj-btn"
                aria-current={project.id === projectId ? "true" : undefined}
                aria-label={`${project.name}（${labels[status]}）`}
                title={`${project.name} · ${labels[status]}\n${project.rootPath}`}
                onClick={() => onSelect(project.id)}
              >
                <span>{abbr}</span>
                <span className="collaboration-project-status-dot" data-activity={status} aria-hidden="true" />
              </button>
            );
          })}
        </div>
      </div>
    )}
    <div id={id} className={`collaboration-project-navigation-body${drawerOpen ? " is-open" : ""}`}>
      <div className="collaboration-project-navigation-heading">
        <h2>协作项目</h2>
        <div className="collaboration-project-heading-actions">
          <button type="button" className="collaboration-rail-toggle" aria-label="收起为极简图标轨" title="收起为极简图标轨" onClick={toggleRailCollapsed}>
            <SidebarSimple size={16} aria-hidden="true" />
          </button>
          <button type="button" className="collaboration-project-add" aria-label="新建协作" title="新建协作" onClick={onCreate}>
            <Plus size={16} aria-hidden="true" />新建
          </button>
        </div>
      </div>
      <label className="collaboration-project-search">
        <MagnifyingGlass size={16} aria-hidden="true" />
        <input type="search" aria-label="搜索协作项目" placeholder="搜索协作项目" value={query} onChange={event => setQuery(event.target.value)} />
      </label>
      <div ref={projectGroups} className="collaboration-project-groups">
        {error && <p role="status">项目状态暂不可用，正在重试。</p>}
        {!results.length && <p>{trimmed ? "没有匹配的协作项目" : activity || error ? "暂无协作项目" : "正在读取协作项目…"}</p>}
        {groups.map(group => {
          const rows = results.filter(item => group.activities.includes(item.activity));
          if (!rows.length) return null;
          const open = Boolean(trimmed) || !collapsed.includes(group.id);
          const groupId = `${id}-${group.id}`;
          return <section key={group.id}>
            <button type="button" className="collaboration-project-group-toggle" data-activity={group.id}
              aria-expanded={open} aria-controls={groupId} disabled={Boolean(trimmed)}
              onClick={() => setCollapsed(current => open ? [...current, group.id] : current.filter(item => item !== group.id))}>
              <span className="collaboration-project-status-dot" aria-hidden="true" />
              <span>{group.label} {rows.length}</span><CaretDown size={15} aria-hidden="true" />
            </button>
            <ul id={groupId} hidden={!open}>
              {rows.map(({ project, activity: status }) => <li className="collaboration-project-list-item" key={project.id}>
                <button type="button" className="collaboration-project-row" aria-current={project.id === projectId ? "true" : undefined}
                  title={`${project.name}\n${project.rootPath}`} onClick={() => {
                    onSelect(project.id);
                    if (drawerOpen) drawerTrigger.current?.focus();
                    setDrawerOpen(false);
                  }}>
                  <FolderSimple size={20} aria-hidden="true" />
                  <span className="collaboration-project-name">{project.name}</span>
                  <span className="collaboration-project-status" data-activity={status}>
                    <span className="collaboration-project-status-dot" aria-hidden="true" />{labels[status]}
                  </span>
                </button>
                <button type="button" className="collaboration-project-actions" title="项目操作" aria-label={`${project.name}的项目操作`}
                  aria-haspopup="menu" aria-expanded={menu?.project.id === project.id} aria-controls={menu?.project.id === project.id ? menuId : undefined}
                  onClick={event => {
                    const anchor = event.currentTarget;
                    const position = actionMenuPosition(anchor.getBoundingClientRect());
                    setMenu(current => current?.project.id === project.id ? null : { project, anchor, position });
                  }}><DotsThree size={18} aria-hidden="true" /></button>
              </li>)}
            </ul>
          </section>;
        })}
      </div>
    </div>
    {menu && createPortal(<div id={menuId} className="conversation-history-menu collaboration-project-actions-menu"
      role="menu" aria-label={`${menu.project.name}的项目操作`} style={menu.position} onKeyDown={event => {
        if (event.key !== "Escape" && event.key !== "Tab") return;
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
        }
        menu.anchor.focus();
        setMenu(null);
      }}>
      <button ref={renameAction} type="button" role="menuitem" onClick={() => {
        menu.anchor.focus();
        setMenu(null);
        setRenameError("");
        setRename({ project: menu.project, name: menu.project.name });
      }}><PencilSimple size={16} aria-hidden="true" />重命名项目</button>
    </div>, document.body)}
    {rename && createPortal(<Modal className="conversation-clear-dialog conversation-session-dialog" ariaLabel="重命名项目"
      onClose={() => { if (!inFlight.current) setRename(null); }}>
      <h2>重命名项目</h2>
      <form onSubmit={event => void saveRename(event)}>
        <label>项目名称<input value={rename.name} required disabled={saving} onFocus={event => event.currentTarget.select()}
          onChange={event => setRename({ ...rename, name: event.target.value })} /></label>
        {renameError && <p className="conversation-clear-error" role="alert">{renameError}</p>}
        <div className="conversation-session-dialog-actions">
          <button type="button" disabled={saving} onClick={() => setRename(null)}>取消</button>
          <button type="submit" className="conversation-clear-confirm" disabled={saving || !rename.name.trim() || rename.name.trim() === rename.project.name}>{saving ? "正在保存…" : "保存"}</button>
        </div>
      </form>
    </Modal>, document.body)}
  </nav>;
}

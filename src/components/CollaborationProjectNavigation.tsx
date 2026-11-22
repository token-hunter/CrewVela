import { useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CaretDown, FolderSimple, MagnifyingGlass, Plus } from "@phosphor-icons/react";
import type { MultiAgentProject } from "../types";
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

interface Props {
  active: boolean;
  projects: MultiAgentProject[];
  projectId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
}

export function CollaborationProjectNavigation({ active, projects, projectId, onSelect, onCreate }: Props) {
  const id = useId();
  const [activity, setActivity] = useState<ProjectActivity[] | null>(null);
  const [error, setError] = useState(false);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const drawerTrigger = useRef<HTMLButtonElement>(null);

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

  return <nav className="collaboration-project-navigation" aria-label="协作项目列表">
    <button ref={drawerTrigger} type="button" className="collaboration-project-drawer" aria-controls={id} aria-expanded={drawerOpen}
      onClick={() => setDrawerOpen(value => !value)}>
      <FolderSimple size={17} aria-hidden="true" />协作项目<CaretDown size={13} aria-hidden="true" />
    </button>
    <div id={id} className={`collaboration-project-navigation-body${drawerOpen ? " is-open" : ""}`}>
      <div className="collaboration-project-navigation-heading">
        <h2>协作项目</h2>
        <button type="button" className="collaboration-project-add" onClick={onCreate}>
          <Plus size={16} aria-hidden="true" />新建协作
        </button>
      </div>
      <label className="collaboration-project-search">
        <MagnifyingGlass size={16} aria-hidden="true" />
        <input type="search" aria-label="搜索协作项目" placeholder="搜索协作项目" value={query} onChange={event => setQuery(event.target.value)} />
      </label>
      <div className="collaboration-project-groups">
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
              {rows.map(({ project, activity: status }) => <li key={project.id}>
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
              </li>)}
            </ul>
          </section>;
        })}
      </div>
    </div>
  </nav>;
}

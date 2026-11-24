import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Archive, ArrowSquareOut, CaretDown, CaretRight, DotsThree, FileText, FolderSimple, LinkSimple, MagnifyingGlass, PencilSimple, Play, Plus, Stop, X } from "@phosphor-icons/react";
import { projectServiceActive, type LocalProjectServices } from "../hooks/useLocalProjectServices";
import type { MultiAgentProject, Project, LocalProjectServiceStatus, ProjectManagementStyle } from "../types";
import {
  ProjectManagementCreativeViews,
  ProjectManagementStyleSwitcher,
} from "./ProjectManagementCreativeViews";
import { Modal } from "./ui/Modal";
import "../styles/project-management.css";

interface Props {
  projects: Project[];
  activeProjectId?: string;
  services?: LocalProjectServices;
  onAdd?: () => void;
  onChanged?: (project: Project) => void;
  onOpen?: (project: Project) => Promise<void>;
  onOpenCollaboration?: (id: string) => void;
  onBrowse?: (url: string) => void;
  viewStyle?: ProjectManagementStyle;
  onViewStyleChange?: (style: ProjectManagementStyle) => void;
  onOpenAppearanceSettings?: () => void;
}

type EditKind = "rename" | "directory" | "archive" | "collaboration";
const titles: Record<EditKind, string> = {
  rename: "重命名项目", directory: "修改项目目录", archive: "归档项目",
  collaboration: "关联协作项目",
};
const labels: Record<LocalProjectServiceStatus["state"], string> = {
  idle: "未运行", starting: "正在启动", running: "运行中", completed: "已完成", failed: "启动失败", stopFailed: "停止待确认",
};

function ProjectEditDialog({ project, kind, onSaved, onClose }: {
  project: Project; kind: EditKind; onSaved: (project: Project) => void; onClose: () => void;
}) {
  const t = useTranslation();
  const [name, setName] = useState(project.name);
  const [path, setPath] = useState(project.path);
  const [linked, setLinked] = useState(project.collaborationProjectIds ?? []);
  const [available, setAvailable] = useState<MultiAgentProject[] | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const inFlight = useRef(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const title = kind === "archive" && project.archivedAt != null ? "恢复项目" : titles[kind];

  useEffect(() => {
    if (kind !== "collaboration") return;
    let active = true;
    void invoke<MultiAgentProject[]>("ma_list_projects").then(next => { if (active) setAvailable(next); },
      reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [kind]);

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (inFlight.current) return;
    inFlight.current = true;
    setSaving(true); setError("");
    try {
      if (kind === "rename") {
        onSaved(await invoke<Project>("update_project", { id: project.id, name: name.trim(), badgeColor: null }));
        return;
      }
      const edit = kind === "directory" ? { kind, path: path.trim() }
        : kind === "archive" ? { kind, archived: project.archivedAt == null }
        : { kind, ids: linked.filter(id => available?.some(item => item.id === id)) };
      onSaved(await invoke<Project>("edit_local_project", { id: project.id, edit }));
    } catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSaving(false); }
  };

  const pickDirectory = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setSaving(true); setError("");
    try {
      const selected = await open({ directory: true, multiple: false, title: "选择项目目录", defaultPath: path });
      if (typeof selected === "string") setPath(selected);
    } catch (reason) { setError(String(reason)); }
    finally { inFlight.current = false; setSaving(false); }
  };

  return <Modal className={kind === "collaboration" ? "project-management-dialog project-management-link-dialog" : "project-management-dialog"}
    ariaLabel={title} initialFocusRef={kind === "archive" ? cancel : undefined}
    onClose={() => { if (!inFlight.current) onClose(); }}>
    <form onSubmit={event => void save(event)}>
      <h3>{title}</h3>
      {kind === "rename" && <label>{t("项目名称")}<input required maxLength={120} disabled={saving} value={name} onChange={event => setName(event.target.value)} /></label>}
      {kind === "directory" && <>
        <label>{t("项目目录")}<input required disabled={saving} value={path} onChange={event => setPath(event.target.value)} /></label>
        <button className="btn secondary" type="button" disabled={saving} onClick={() => void pickDirectory()}><FolderSimple size={16} />{t("选择目录")}</button>
        <p>新会话使用新目录，已有会话保留原工作目录。</p>
      </>}
      {kind === "archive" && <p>{project.archivedAt == null ? `归档「${project.name}」并从主界面项目列表收起。已有会话和项目文件会保留。` : `将「${project.name}」恢复到主界面项目列表。`}</p>}
      {kind === "collaboration" && <div className="project-management-links">
        {!available && !error && <p role="status">正在读取协作项目…</p>}
        {available?.length === 0 && <p>可以先在协作中心创建协作项目。</p>}
        {available?.map(item => <label key={item.id}><input type="checkbox" disabled={saving} checked={linked.includes(item.id)}
          onChange={event => setLinked(event.target.checked ? [...linked, item.id] : linked.filter(id => id !== item.id))} />
          <span>{item.name}<small>{item.rootPath}</small></span>
        </label>)}
      </div>}
      {error && <p className="project-management-error" role="alert">{error}</p>}
      <footer>
        <button ref={cancel} className="btn secondary" type="button" disabled={saving} onClick={onClose}>{t("取消")}</button>
        <button className="btn" type="submit" disabled={saving || (kind === "rename" && !name.trim()) || (kind === "directory" && !path.trim()) || (kind === "collaboration" && !available)}>{saving ? "处理中…" : kind === "archive" ? title : t("保存")}</button>
      </footer>
    </form>
  </Modal>;
}

function ServiceLog({ project, onClose }: { project: Project; onClose: () => void }) {
  const t = useTranslation();
  const [log, setLog] = useState<string | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const status = await invoke<LocalProjectServiceStatus>("local_project_service_logs", { id: project.id });
        if (active) { setLog(status.logs ?? ""); setError(""); }
      } catch (reason) { if (active) setError(String(reason)); }
      finally { pending = false; }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1500);
    return () => { active = false; window.clearInterval(timer); };
  }, [project.id]);
  return <Modal className="project-management-dialog project-management-log" ariaLabel={`${project.name}的开发服务日志`} onClose={onClose}>
    <header><h3>{project.name} · 服务日志</h3><button type="button" className="icon-button" aria-label={t("关闭日志")} onClick={onClose}><X size={18} /></button></header>
    {error && <p className="project-management-error" role="alert">{error}</p>}
    <pre tabIndex={0}>{log === null ? "正在读取日志…" : log || "本次运行暂无日志。"}</pre>
  </Modal>;
}

export function ProjectManagementPanel({
  projects,
  activeProjectId,
  services,
  onAdd,
  onChanged,
  onOpen,
  onOpenCollaboration,
  onBrowse,
  viewStyle = "classic",
  onViewStyleChange,
  onOpenAppearanceSettings,
}: Props) {
  const t = useTranslation();
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [dialog, setDialog] = useState<{ project: Project; kind: EditKind } | null>(null);
  const [logs, setLogs] = useState<Project | null>(null);
  const [error, setError] = useState("");
  const [opening, setOpening] = useState<string | null>(null);
  const [collaboration, setCollaboration] = useState<MultiAgentProject[]>([]);
  const listId = useId();
  const search = query.trim().toLocaleLowerCase();
  const matches = [...projects].filter(project => `${project.name} ${project.path}`.toLocaleLowerCase().includes(search))
    .sort((left, right) => (right.lastOpenedAt ?? 0) - (left.lastOpenedAt ?? 0) || right.createdAt - left.createdAt);
  const active = matches.filter(project => project.archivedAt == null);
  const archived = matches.filter(project => project.archivedAt != null);
  const hasLinks = projects.some(project => !!project.collaborationProjectIds?.length);
  useEffect(() => {
    if (!hasLinks) return;
    let active = true;
    void invoke<MultiAgentProject[]>("ma_list_projects").then(items => { if (active) setCollaboration(items); }, () => {});
    return () => { active = false; };
  }, [hasLinks, dialog]);

  const openProject = async (project: Project) => {
    if (!onOpen || opening) return;
    setOpening(project.id); setError("");
    try { await onOpen(project); } catch (reason) { setError(String(reason)); }
    finally { setOpening(null); }
  };
  const changeService = async (id: string, action: "start" | "stop" | "recheck") => {
    setError("");
    try { await services?.change(id, action); } catch { /* The project-specific error is shown on its card. */ }
  };
  const renderProject = (project: Project) => {
    const isOpen = expanded === project.id;
    const archived = project.archivedAt != null;
    const status = services?.statuses[project.id];
    const running = projectServiceActive(status);
    const working = services?.pending.has(project.id);
    const serviceError = services?.startErrors[project.id] || status?.error;
    const details = `${listId}-${project.id}`;
    return <article key={project.id} className="project-management-card">
      <header>
        <button type="button" className="project-management-card-toggle" aria-label={project.name} aria-expanded={isOpen} aria-controls={details} onClick={() => setExpanded(isOpen ? null : project.id)}>
          <span className="project-management-folder"><FolderSimple size={23} /></span>
          <span><strong>{project.name}</strong><small title={project.path}>{project.path}</small></span>
          {status?.state === "running" && !isOpen && <span className="project-management-dot" title={t("运行中")} />}
        </button>
        {!archived && onOpen && <button className={`btn${isOpen ? " primary" : ""}`} type="button" disabled={opening !== null} onClick={() => void openProject(project)}><ArrowSquareOut size={17} />{t("打开工作区")}</button>}
        <details className="project-management-menu">
          <summary aria-label={`${project.name}的项目操作`}><DotsThree size={21} /></summary>
          <div>
            {([
              ["rename", PencilSimple, "改名"],
              ["directory", FolderSimple, "修改目录"],
              ["archive", Archive, archived ? "恢复项目" : "归档"],
            ] as const).map(([kind, Icon, label]) => (
              <button type="button" key={kind} disabled={(kind === "directory" || kind === "archive") && (running || working)} onClick={event => {
                const menu = event.currentTarget.closest("details");
                if (menu) { menu.open = false; menu.querySelector("summary")?.focus(); }
                setDialog({ project, kind });
              }}><Icon size={16} />{label}</button>
            ))}
          </div>
        </details>
        <button className="project-management-chevron icon-button" type="button" aria-label={isOpen ? `收起${project.name}` : `展开${project.name}`}
          aria-expanded={isOpen} aria-controls={details} onClick={() => setExpanded(isOpen ? null : project.id)}>{isOpen ? <CaretDown size={17} /> : <CaretRight size={17} />}</button>
      </header>
      <div id={details} hidden={!isOpen}>
        <div className="project-management-service">
          <strong>{t("本地开发服务")}</strong>
          <div className="project-management-actions">
            {!archived && <span className="project-management-status" data-state={status?.state ?? "idle"}>{status?.state === "running" && <span className="project-management-dot" />}{working && !running ? "正在准备启动" : labels[status?.state ?? "idle"]}</span>}
            {status?.state === "running" && status.url && onBrowse && <button type="button" className="btn secondary" onClick={() => { if (status.url) onBrowse(status.url); }}><ArrowSquareOut size={16} />{t("打开页面")}</button>}
            {status && <button type="button" className="btn secondary" onClick={() => setLogs(project)}><FileText size={16} />{t("日志")}</button>}
            {!archived && services && <button type="button" className="btn secondary project-management-primary" disabled={working} onClick={() => void changeService(project.id, running ? "stop" : "start")}>
              {running ? <Stop size={16} /> : <Play size={16} />}{running ? t("停止项目") : t("启动项目")}
            </button>}
          </div>
        </div>
        {serviceError && <div>
          <p className="project-management-error" role="alert">{serviceError}</p>
          {!archived && !running && services && <button className="btn secondary" type="button" disabled={working} onClick={() => void changeService(project.id, "recheck")}>{t("重新识别启动方式")}</button>}
        </div>}
        <div className="project-management-association">
          <button type="button" onClick={() => setDialog({ project, kind: "collaboration" })}><LinkSimple size={17} />{t("关联协作")}<CaretRight size={14} /></button>
          {collaboration.filter(item => project.collaborationProjectIds?.includes(item.id)).map(item =>
            <button key={item.id} type="button" onClick={() => onOpenCollaboration?.(item.id)}>{item.name}<ArrowSquareOut size={14} /></button>)}
        </div>
      </div>
    </article>;
  };

  return <div className="project-management" onPointerDown={event => {
    for (const menu of event.currentTarget.querySelectorAll<HTMLDetailsElement>(".project-management-menu[open]")) {
      if (event.target instanceof Node && !menu.contains(event.target)) menu.open = false;
    }
  }}>
    <div className={`project-management-toolbar${onViewStyleChange ? " has-mode-switcher" : ""}`}>
      <label className="project-management-search"><MagnifyingGlass size={17} aria-hidden="true" />
        <input type="search" aria-label={t("搜索项目名或目录")} placeholder={t("搜索项目名或目录")} value={query} onChange={event => setQuery(event.target.value)} />
      </label>
      {onViewStyleChange && (
        <ProjectManagementStyleSwitcher
          style={viewStyle}
          onChange={onViewStyleChange}
          onOpenAppearanceSettings={onOpenAppearanceSettings}
        />
      )}
      {onAdd && <button type="button" className="btn primary" onClick={onAdd}><Plus size={18} />{t("添加项目")}</button>}
    </div>
    {(error || services?.readError) && <p className="project-management-error" role="alert">{error || `读取服务状态失败：${services?.readError}`}</p>}
    {!matches.length && <p className="project-management-empty">{search ? "没有匹配的项目。" : "添加本地目录后即可管理项目。"}</p>}
    {matches.length > 0 && viewStyle === "classic" && (
      <>
        {active.map(renderProject)}
        {archived.length > 0 && <details className="project-management-archived" open={search ? true : undefined}>
          <summary>{t("已归档")}</summary>{archived.map(renderProject)}
        </details>}
      </>
    )}
    {matches.length > 0 && viewStyle !== "classic" && (
      <ProjectManagementCreativeViews
        style={viewStyle}
        active={active}
        archived={archived}
        search={search}
        activeProjectId={activeProjectId}
        services={services}
        collaboration={collaboration}
        opening={opening}
        onOpenProject={project => void openProject(project)}
        onChangeService={(id, action) => void changeService(id, action)}
        onBrowse={onBrowse}
        onOpenLogs={project => setLogs(project)}
        onOpenDialog={(project, kind) => setDialog({ project, kind })}
        onOpenCollaboration={onOpenCollaboration}
      />
    )}
    {dialog && <ProjectEditDialog project={dialog.project} kind={dialog.kind} onClose={() => setDialog(null)} onSaved={project => {
      onChanged?.(project); void services?.refresh(); setDialog(null);
    }} />}
    {logs && <ServiceLog project={logs} onClose={() => setLogs(null)} />}
  </div>;
}

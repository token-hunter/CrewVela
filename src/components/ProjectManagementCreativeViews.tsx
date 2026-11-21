import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  Archive,
  ArrowCounterClockwise,
  ArrowSquareOut,
  Check,
  DotsThree,
  FileText,
  FolderSimple,
  GridFour,
  Lightbulb,
  LinkSimple,
  Moon,
  Palette,
  PencilSimple,
  Play,
  Sparkle,
  Stop,
  X,
} from "@phosphor-icons/react";
import {
  loadProjectGalleryDensity,
  loadProjectPinboardNeat,
  saveProjectGalleryDensity,
  saveProjectPinboardNeat,
  type ProjectGalleryDensity,
} from "../appearance";
import { projectServiceActive, type LocalProjectServices } from "../hooks/useLocalProjectServices";
import type {
  LocalProjectServiceStatus,
  MultiAgentProject,
  Project,
  ProjectManagementStyle,
} from "../types";
import { Modal } from "./ui/Modal";
import "../styles/project-management-creative.css";

export type ProjectEditKind = "rename" | "directory" | "archive" | "collaboration";

export const PROJECT_STYLE_MENU: ReadonlyArray<{ id: ProjectManagementStyle; label: string }> = [
  { id: "classic", label: "经典列表" },
  { id: "gallery", label: "精选画廊" },
  { id: "vinyl", label: "黑胶唱片架" },
  { id: "pinboard", label: "相纸灵感墙" },
  { id: "studio", label: "亮灯工作室" },
];

const STATUS_LABELS: Record<LocalProjectServiceStatus["state"], string> = {
  idle: "未运行",
  starting: "正在启动",
  running: "运行中",
  completed: "已完成",
  failed: "启动失败",
  stopFailed: "停止待确认",
};

const COVER_PALETTES: ReadonlyArray<readonly [string, string]> = [
  ["#d97757", "#7c3aed"],
  ["#3ebac6", "#2563eb"],
  ["#9b7af0", "#ec4899"],
  ["#47be72", "#0d9488"],
  ["#e2a336", "#ea580c"],
  ["#5c8df6", "#6366f1"],
];

function hashString(input: string): number {
  let hash = 0;
  for (let i = 0; i < input.length; i += 1) {
    hash = ((hash << 5) - hash + input.charCodeAt(i)) | 0;
  }
  return Math.abs(hash);
}

function resolveProjectColors(project: Project): readonly [string, string] {
  const fallback = COVER_PALETTES[hashString(project.id || project.name) % COVER_PALETTES.length];
  const primary = project.badgeColor?.trim() || fallback[0];
  const secondary = fallback[1];
  return [primary, secondary];
}

function buildCoverGradient(project: Project): CSSProperties {
  const [c1, c2] = resolveProjectColors(project);
  return {
    background: [
      `radial-gradient(ellipse 78% 74% at 88% 14%, ${c2}e6 0%, transparent 62%)`,
      `radial-gradient(ellipse 84% 78% at 12% 86%, ${c1}f2 0%, transparent 66%)`,
      `radial-gradient(ellipse 52% 46% at 52% 40%, rgba(255,255,255,0.14) 0%, transparent 62%)`,
      `linear-gradient(135deg, ${c1} 0%, #151519 100%)`,
    ].join(", "),
  };
}

function shortPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : path;
}

function formatRecency(project: Project): string {
  const ts = project.lastOpenedAt ?? project.updatedAt ?? project.createdAt;
  if (!ts) return "本地项目";
  const diffMs = Math.max(0, Date.now() - ts);
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 2) return "刚刚";
  if (mins < 60) return `${mins} 分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days} 天前`;
  return new Date(ts).toLocaleDateString();
}

function formatShortUrl(url: string | null | undefined): string {
  if (!url) return "";
  return url.replace(/^https?:\/\/(localhost|127\.0\.0\.1)/, "");
}

export function ProjectManagementStyleSwitcher({
  style,
  onChange,
  onOpenAppearanceSettings,
}: {
  style: ProjectManagementStyle;
  onChange: (next: ProjectManagementStyle) => void;
  onOpenAppearanceSettings?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="pm-style-switcher" ref={rootRef}>
      <button
        type="button"
        className="pm-style-trigger"
        aria-label="切换项目管理模式"
        title="切换项目管理模式"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <Palette size={14} aria-hidden="true" />
        <span>模式</span>
      </button>
      {open && (
        <div className="pm-style-popover" role="menu" aria-label="切换项目管理模式">
          <div className="pm-style-popover-title">项目管理模式</div>
          {PROJECT_STYLE_MENU.map((option) => {
            const active = style === option.id;
            return (
              <button
                key={option.id}
                type="button"
                role="menuitemradio"
                aria-checked={active}
                className={`pm-style-option${active ? " active" : ""}`}
                onClick={() => {
                  onChange(option.id);
                  setOpen(false);
                }}
              >
                <span>{option.label}</span>
                {active && <Check size={13} weight="bold" aria-hidden="true" />}
              </button>
            );
          })}
          {onOpenAppearanceSettings && (
            <>
              <div className="pm-style-divider" />
              <button
                type="button"
                role="menuitem"
                className="pm-style-option pm-style-settings-link"
                onClick={() => {
                  setOpen(false);
                  onOpenAppearanceSettings();
                }}
              >
                <span>前往「外观」设置</span>
                <span aria-hidden="true">↗</span>
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

export interface CreativeViewProps {
  style: Exclude<ProjectManagementStyle, "classic">;
  active: Project[];
  archived: Project[];
  search: string;
  activeProjectId?: string;
  services?: LocalProjectServices;
  collaboration: MultiAgentProject[];
  opening: string | null;
  onOpenProject: (project: Project) => void;
  onChangeService: (id: string, action: "start" | "stop" | "recheck") => void;
  onBrowse?: (url: string) => void;
  onOpenLogs: (project: Project) => void;
  onOpenDialog: (project: Project, kind: ProjectEditKind) => void;
  onOpenCollaboration?: (id: string) => void;
}

function ProjectActionMenu({
  project,
  running,
  working,
  onOpenDialog,
  compact = false,
}: {
  project: Project;
  running: boolean;
  working: boolean;
  onOpenDialog: (project: Project, kind: ProjectEditKind) => void;
  compact?: boolean;
}) {
  const archived = project.archivedAt != null;
  return (
    <details className={`project-management-menu${compact ? " pm-menu-compact" : ""}`}>
      <summary aria-label={`${project.name}的项目操作`}>
        <DotsThree size={18} />
      </summary>
      <div>
        {([
          ["rename", PencilSimple, "改名"],
          ["directory", FolderSimple, "修改目录"],
          ["collaboration", LinkSimple, "关联协作"],
          ["archive", Archive, archived ? "恢复项目" : "归档"],
        ] as const).map(([kind, Icon, label]) => (
          <button
            type="button"
            key={kind}
            disabled={(kind === "directory" || kind === "archive") && (running || working)}
            onClick={(event) => {
              const menu = event.currentTarget.closest("details");
              if (menu) {
                menu.open = false;
                menu.querySelector("summary")?.focus();
              }
              onOpenDialog(project, kind);
            }}
          >
            <Icon size={15} />
            {label}
          </button>
        ))}
      </div>
    </details>
  );
}

function ArchivedStrip({
  archived,
  search,
  onOpenDialog,
}: {
  archived: Project[];
  search: string;
  onOpenDialog: (project: Project, kind: ProjectEditKind) => void;
}) {
  if (!archived.length) return null;
  return (
    <details className="pm-archived-strip" open={search ? true : undefined}>
      <summary>已归档（{archived.length}）</summary>
      <div className="pm-archived-grid">
        {archived.map((project) => {
          const [c1] = resolveProjectColors(project);
          return (
            <div className="pm-archived-chip" key={project.id}>
              <span className="pm-archived-dot" style={{ background: c1 }} aria-hidden="true" />
              <div className="pm-archived-copy">
                <strong>{project.name}</strong>
                <small title={project.path}>{shortPath(project.path)}</small>
              </div>
              <button
                type="button"
                className="btn secondary pm-mini-btn"
                onClick={() => onOpenDialog(project, "archive")}
              >
                恢复项目
              </button>
            </div>
          );
        })}
      </div>
    </details>
  );
}

export function ProjectManagementCreativeViews(props: CreativeViewProps) {
  const {
    style,
    active,
    archived,
    search,
    activeProjectId,
    services,
    collaboration,
    opening,
    onOpenProject,
    onChangeService,
    onBrowse,
    onOpenLogs,
    onOpenDialog,
    onOpenCollaboration,
  } = props;

  const [density, setDensity] = useState<ProjectGalleryDensity>(loadProjectGalleryDensity);
  const [sheetId, setSheetId] = useState<string | null>(null);
  const [vinylSelectedId, setVinylSelectedId] = useState<string | null>(activeProjectId ?? null);
  const [pinboardNeat, setPinboardNeat] = useState<boolean>(loadProjectPinboardNeat);

  const setAndSaveDensity = (next: ProjectGalleryDensity) => {
    setDensity(next);
    saveProjectGalleryDensity(next);
  };

  const toggleAndSaveNeat = () => {
    setPinboardNeat((current) => {
      const next = !current;
      saveProjectPinboardNeat(next);
      return next;
    });
  };

  const getServiceInfo = (project: Project) => {
    const status = services?.statuses[project.id];
    const running = projectServiceActive(status);
    const working = services?.pending.has(project.id) ?? false;
    const serviceError = services?.startErrors[project.id] || status?.error;
    const linked = collaboration.filter((item) => project.collaborationProjectIds?.includes(item.id));
    return { status, running, working, serviceError, linked };
  };

  /* ---------------- 1. 精选画廊 (gallery) ---------------- */
  if (style === "gallery") {
    const sheetProject = sheetId ? active.find((item) => item.id === sheetId) ?? null : null;
    const runningCount = active.filter((item) => projectServiceActive(services?.statuses[item.id])).length;

    return (
      <div className="pm-creative pm-gallery">
        <div className="pm-subbar">
          <span className="pm-subbar-meta">
            {active.length} 个活跃项目 · {runningCount} 个服务运行中
          </span>
          <div className="pm-density-group" role="group" aria-label="相簿密度">
            {([
              ["large", "大"],
              ["medium", "中"],
              ["compact", "密"],
            ] as const).map(([id, label]) => (
              <button
                key={id}
                type="button"
                className={`pm-density-btn${density === id ? " active" : ""}`}
                aria-pressed={density === id}
                onClick={() => setAndSaveDensity(id)}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className={`pm-gallery-grid density-${density}`}>
          {active.map((project, index) => {
            const { status, running, working, serviceError, linked } = getServiceInfo(project);
            const isHero = index === 0 && !search && density !== "compact" && active.length > 1;
            return (
              <article
                key={project.id}
                className={`pm-gallery-card${isHero ? " is-hero" : ""}${running ? " is-running" : ""}`}
                onClick={() => setSheetId(project.id)}
              >
                <div className="pm-cover-art" style={buildCoverGradient(project)} aria-hidden="true" />
                <div className="pm-cover-scrim" aria-hidden="true" />

                <div className="pm-gallery-top">
                  <span className="pm-glass-pill">
                    {running ? (
                      <>
                        <span className="pm-live-dot" aria-hidden="true" />
                        <span>
                          {status?.state === "starting" ? "正在启动" : "运行中"}
                          {status?.url ? ` · ${formatShortUrl(status.url)}` : ""}
                        </span>
                      </>
                    ) : (
                      <span>{serviceError ? "启动异常" : formatRecency(project)}</span>
                    )}
                  </span>

                  <div className="pm-gallery-quick" onClick={(event) => event.stopPropagation()}>
                    {services && (
                      <button
                        type="button"
                        className="pm-glass-btn"
                        disabled={working}
                        onClick={() => onChangeService(project.id, running ? "stop" : "start")}
                      >
                        {running ? <Stop size={13} /> : <Play size={13} />}
                        <span>{running ? "停止" : "启动"}</span>
                      </button>
                    )}
                    <button
                      type="button"
                      className="pm-glass-btn"
                      disabled={opening !== null}
                      onClick={() => onOpenProject(project)}
                    >
                      <span>打开</span>
                    </button>
                  </div>
                </div>

                <div className="pm-gallery-bottom">
                  <div className="pm-gallery-copy">
                    <strong>{project.name}</strong>
                    <small title={project.path}>
                      {shortPath(project.path)} · {formatRecency(project)}
                    </small>
                  </div>
                  {linked.length > 0 && isHero && (
                    <span className="pm-glass-pill">
                      <LinkSimple size={12} aria-hidden="true" />
                      <span>{linked.length} 项协作</span>
                    </span>
                  )}
                </div>
              </article>
            );
          })}
        </div>

        <ArchivedStrip archived={archived} search={search} onOpenDialog={onOpenDialog} />

        {sheetProject && (() => {
          const { status, running, working, serviceError, linked } = getServiceInfo(sheetProject);
          return (
            <Modal
              className="pm-gallery-sheet"
              ariaLabel={`${sheetProject.name} 项目详情`}
              onClose={() => setSheetId(null)}
            >
              <div className="pm-sheet-hero">
                <div className="pm-cover-art" style={buildCoverGradient(sheetProject)} aria-hidden="true" />
                <div className="pm-cover-scrim" aria-hidden="true" />
                <button
                  type="button"
                  className="pm-glass-btn pm-sheet-close"
                  aria-label="关闭详情"
                  onClick={() => setSheetId(null)}
                >
                  <X size={15} />
                </button>
                <div className="pm-sheet-hero-copy">
                  <h3>{sheetProject.name}</h3>
                  <code title={sheetProject.path}>{sheetProject.path}</code>
                </div>
              </div>

              <div className="pm-sheet-body">
                <div className="pm-sheet-section">
                  <div>
                    <strong>本地开发服务</strong>
                    <span className="project-management-status" data-state={status?.state ?? "idle"}>
                      {status?.state === "running" && <span className="project-management-dot" />}
                      {working && !running ? "正在准备启动" : STATUS_LABELS[status?.state ?? "idle"]}
                      {status?.url ? ` · ${status.url}` : ""}
                    </span>
                  </div>
                  <div className="pm-sheet-actions">
                    {status?.state === "running" && status.url && onBrowse && (
                      <button type="button" className="btn secondary" onClick={() => onBrowse(status.url!)}>
                        <ArrowSquareOut size={15} />
                        打开页面
                      </button>
                    )}
                    {status && (
                      <button type="button" className="btn secondary" onClick={() => onOpenLogs(sheetProject)}>
                        <FileText size={15} />
                        日志
                      </button>
                    )}
                    {services && (
                      <button
                        type="button"
                        className="btn secondary project-management-primary"
                        disabled={working}
                        onClick={() => onChangeService(sheetProject.id, running ? "stop" : "start")}
                      >
                        {running ? <Stop size={15} /> : <Play size={15} />}
                        {running ? "停止项目" : "启动项目"}
                      </button>
                    )}
                  </div>
                </div>

                {serviceError && (
                  <div className="pm-sheet-error">
                    <p className="project-management-error" role="alert">{serviceError}</p>
                    {!running && services && (
                      <button
                        type="button"
                        className="btn secondary"
                        disabled={working}
                        onClick={() => onChangeService(sheetProject.id, "recheck")}
                      >
                        <ArrowCounterClockwise size={14} />
                        重新识别启动方式
                      </button>
                    )}
                  </div>
                )}

                <div className="pm-sheet-section">
                  <div className="pm-sheet-collab">
                    <button
                      type="button"
                      className="pm-link-btn"
                      onClick={() => {
                        setSheetId(null);
                        onOpenDialog(sheetProject, "collaboration");
                      }}
                    >
                      <LinkSimple size={15} />
                      <span>关联协作</span>
                    </button>
                    {linked.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        className="pm-link-chip"
                        onClick={() => {
                          setSheetId(null);
                          onOpenCollaboration?.(item.id);
                        }}
                      >
                        <span>{item.name}</span>
                        <ArrowSquareOut size={13} />
                      </button>
                    ))}
                  </div>
                </div>

                <footer className="pm-sheet-footer">
                  <div className="pm-sheet-footer-left">
                    <button
                      type="button"
                      className="btn secondary"
                      onClick={() => {
                        setSheetId(null);
                        onOpenDialog(sheetProject, "rename");
                      }}
                    >
                      <PencilSimple size={14} />
                      改名
                    </button>
                    <button
                      type="button"
                      className="btn secondary"
                      disabled={running || working}
                      onClick={() => {
                        setSheetId(null);
                        onOpenDialog(sheetProject, "directory");
                      }}
                    >
                      <FolderSimple size={14} />
                      修改目录
                    </button>
                    <button
                      type="button"
                      className="btn secondary"
                      disabled={running || working}
                      onClick={() => {
                        setSheetId(null);
                        onOpenDialog(sheetProject, "archive");
                      }}
                    >
                      <Archive size={14} />
                      归档
                    </button>
                  </div>
                  <button
                    type="button"
                    className="btn primary"
                    disabled={opening !== null}
                    onClick={() => {
                      setSheetId(null);
                      onOpenProject(sheetProject);
                    }}
                  >
                    <ArrowSquareOut size={15} />
                    打开工作区
                  </button>
                </footer>
              </div>
            </Modal>
          );
        })()}
      </div>
    );
  }

  /* ---------------- 2. 黑胶唱片架 (vinyl) ---------------- */
  if (style === "vinyl") {
    const selected = active.find((item) => item.id === vinylSelectedId) ?? active[0] ?? null;
    const selectedInfo = selected ? getServiceInfo(selected) : null;

    return (
      <div className="pm-creative pm-vinyl">
        <div className="pm-subbar">
          <span className="pm-subbar-meta">启动本地服务时黑胶唱片从封套滑出并旋转，点选唱片可在下方唱机台操控。</span>
        </div>

        <div className="pm-vinyl-grid">
          {active.map((project) => {
            const { running } = getServiceInfo(project);
            const [c1] = resolveProjectColors(project);
            const isSelected = selected?.id === project.id;
            return (
              <article
                key={project.id}
                className={`pm-vinyl-card${running ? " spinning" : ""}${isSelected ? " selected" : ""}`}
                onClick={() => setVinylSelectedId(project.id)}
              >
                <div
                  className="pm-vinyl-disc"
                  style={{ "--pm-disc-color": c1 } as CSSProperties}
                  aria-hidden="true"
                />
                <div className="pm-vinyl-sleeve" style={buildCoverGradient(project)}>
                  <div className="pm-cover-scrim" aria-hidden="true" />
                  <div className="pm-vinyl-sleeve-top">
                    <span>SIDE A</span>
                    <span>{running ? "● ON AIR" : "33 RPM"}</span>
                  </div>
                  <div className="pm-vinyl-sleeve-bottom">
                    <strong>{project.name}</strong>
                    <small>{formatRecency(project)}</small>
                  </div>
                </div>
              </article>
            );
          })}
        </div>

        {selected && selectedInfo && (
          <section className="pm-turntable-deck" aria-label="唱机控制台">
            <div className="pm-turntable-main">
              <div
                className={`pm-turntable-platter${selectedInfo.running ? " spinning" : ""}`}
                style={{ "--pm-disc-color": resolveProjectColors(selected)[0] } as CSSProperties}
                aria-hidden="true"
              />
              <div className="pm-turntable-copy">
                <div className="pm-turntable-title">
                  <strong>{selected.name}</strong>
                  <span className="project-management-status" data-state={selectedInfo.status?.state ?? "idle"}>
                    {selectedInfo.running && <span className="pm-live-dot" aria-hidden="true" />}
                    {selectedInfo.working && !selectedInfo.running
                      ? "正在准备启动"
                      : STATUS_LABELS[selectedInfo.status?.state ?? "idle"]}
                    {selectedInfo.status?.url ? ` · ${selectedInfo.status.url}` : ""}
                  </span>
                </div>
                <small title={selected.path}>{selected.path}</small>
                <div className="pm-turntable-links">
                  <button
                    type="button"
                    className="pm-link-btn"
                    onClick={() => onOpenDialog(selected, "collaboration")}
                  >
                    <LinkSimple size={14} />
                    <span>关联协作</span>
                  </button>
                  {selectedInfo.linked.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className="pm-link-chip"
                      onClick={() => onOpenCollaboration?.(item.id)}
                    >
                      <span>{item.name}</span>
                      <ArrowSquareOut size={12} />
                    </button>
                  ))}
                </div>
              </div>

              <div className="pm-turntable-actions">
                {services && (
                  <button
                    type="button"
                    className="btn secondary project-management-primary"
                    disabled={selectedInfo.working}
                    onClick={() => onChangeService(selected.id, selectedInfo.running ? "stop" : "start")}
                  >
                    {selectedInfo.running ? <Stop size={15} /> : <Play size={15} />}
                    {selectedInfo.running ? "停止项目" : "启动项目"}
                  </button>
                )}
                <button
                  type="button"
                  className="btn primary"
                  disabled={opening !== null}
                  onClick={() => onOpenProject(selected)}
                >
                  <ArrowSquareOut size={15} />
                  打开工作区
                </button>
                {selectedInfo.status?.state === "running" && selectedInfo.status.url && onBrowse && (
                  <button
                    type="button"
                    className="btn secondary"
                    onClick={() => onBrowse(selectedInfo.status!.url!)}
                  >
                    打开页面
                  </button>
                )}
                {selectedInfo.status && (
                  <button
                    type="button"
                    className="btn secondary"
                    onClick={() => onOpenLogs(selected)}
                  >
                    <FileText size={15} />
                    日志
                  </button>
                )}
                <ProjectActionMenu
                  project={selected}
                  running={selectedInfo.running}
                  working={selectedInfo.working}
                  onOpenDialog={onOpenDialog}
                  compact
                />
              </div>
            </div>

            {selectedInfo.serviceError && (
              <div className="pm-inline-error">
                <p className="project-management-error" role="alert">{selectedInfo.serviceError}</p>
                {!selectedInfo.running && services && (
                  <button
                    type="button"
                    className="btn secondary pm-mini-btn"
                    disabled={selectedInfo.working}
                    onClick={() => onChangeService(selected.id, "recheck")}
                  >
                    重新识别启动方式
                  </button>
                )}
              </div>
            )}
          </section>
        )}

        <ArchivedStrip archived={archived} search={search} onOpenDialog={onOpenDialog} />
      </div>
    );
  }

  /* ---------------- 4. 相纸灵感墙 (pinboard) ---------------- */
  if (style === "pinboard") {
    const tilts = [-2.2, 1.8, -1.5, 2.1, -1.6, 1.4];

    return (
      <div className="pm-creative pm-pinboard">
        <div className="pm-subbar">
          <span className="pm-subbar-meta">将项目相纸钉在灵感墙上，运行中亮起状态指示灯。</span>
          <button
            type="button"
            className="btn secondary pm-mini-btn"
            onClick={toggleAndSaveNeat}
          >
            {pinboardNeat ? <Sparkle size={14} /> : <GridFour size={14} />}
            <span>{pinboardNeat ? "自然散落" : "整齐对齐"}</span>
          </button>
        </div>

        <div className="pm-corkboard">
          {active.map((project, index) => {
            const { status, running, working, serviceError, linked } = getServiceInfo(project);
            const [c1] = resolveProjectColors(project);
            const tilt = pinboardNeat ? 0 : tilts[index % tilts.length];
            return (
              <article
                key={project.id}
                className={`pm-photo-card${running ? " is-running" : ""}`}
                style={{ "--pm-tilt": `${tilt}deg` } as CSSProperties}
              >
                <span className="pm-photo-pin" style={{ background: c1 }} aria-hidden="true" />
                <div
                  className="pm-photo-frame"
                  style={buildCoverGradient(project)}
                  onClick={() => onOpenProject(project)}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      onOpenProject(project);
                    }
                  }}
                >
                  <div className="pm-cover-scrim" aria-hidden="true" />
                  <div className="pm-photo-frame-top">
                    <span className="pm-glass-pill">{formatRecency(project)}</span>
                    {running && (
                      <span className="pm-glass-pill is-live">
                        <span className="pm-live-dot" aria-hidden="true" />
                        <span>ON</span>
                      </span>
                    )}
                  </div>
                  <div className="pm-photo-frame-path" title={project.path}>
                    {shortPath(project.path)}
                  </div>
                </div>

                <div className="pm-photo-caption">
                  <div className="pm-photo-title-row">
                    <strong title={project.name}>{project.name}</strong>
                    <ProjectActionMenu
                      project={project}
                      running={running}
                      working={working}
                      onOpenDialog={onOpenDialog}
                      compact
                    />
                  </div>

                  {serviceError && (
                    <div className="pm-photo-error">
                      <span className="project-management-error" role="alert">{serviceError}</span>
                      {!running && services && (
                        <button
                          type="button"
                          className="pm-link-btn"
                          disabled={working}
                          onClick={() => onChangeService(project.id, "recheck")}
                        >
                          重新识别
                        </button>
                      )}
                    </div>
                  )}

                  {linked.length > 0 && (
                    <div className="pm-photo-links">
                      {linked.map((item) => (
                        <button
                          key={item.id}
                          type="button"
                          className="pm-link-chip"
                          onClick={() => onOpenCollaboration?.(item.id)}
                        >
                          <span>{item.name}</span>
                          <ArrowSquareOut size={11} />
                        </button>
                      ))}
                    </div>
                  )}

                  <div className="pm-photo-actions">
                    {services && (
                      <button
                        type="button"
                        className={`btn secondary pm-mini-btn${running ? " project-management-primary" : ""}`}
                        disabled={working}
                        onClick={() => onChangeService(project.id, running ? "stop" : "start")}
                      >
                        {running ? <Stop size={13} /> : <Play size={13} />}
                        <span>{running ? "停止" : "启动"}</span>
                      </button>
                    )}
                    <button
                      type="button"
                      className="btn secondary pm-mini-btn"
                      disabled={opening !== null}
                      onClick={() => onOpenProject(project)}
                    >
                      工作区
                    </button>
                    {status?.state === "running" && status.url && onBrowse && (
                      <button
                        type="button"
                        className="btn secondary pm-mini-btn"
                        onClick={() => onBrowse(status.url!)}
                        title={status.url}
                      >
                        页面
                      </button>
                    )}
                    {status && (
                      <button
                        type="button"
                        className="btn secondary pm-mini-btn"
                        onClick={() => onOpenLogs(project)}
                      >
                        日志
                      </button>
                    )}
                  </div>
                </div>
              </article>
            );
          })}
        </div>

        <ArchivedStrip archived={archived} search={search} onOpenDialog={onOpenDialog} />
      </div>
    );
  }

  /* ---------------- 5. 亮灯工作室 (studio) ---------------- */
  const litCount = active.filter((item) => projectServiceActive(services?.statuses[item.id])).length;
  const windowPattern = [1, 1, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1];

  return (
    <div className="pm-creative pm-studio">
      <div className="pm-subbar">
        <span className="pm-subbar-meta">启动本地开发服务即可点亮对应工作室灯火，直观掌握园区运行态。</span>
        <span className="pm-studio-counter">
          <span className={litCount > 0 ? "pm-live-dot" : "pm-dim-dot"} aria-hidden="true" />
          <strong>
            {litCount} / {active.length}
          </strong>
          <span>栋亮灯运行中</span>
        </span>
      </div>

      <div className="pm-studio-grid">
        {active.map((project, index) => {
          const { status, running, working, serviceError, linked } = getServiceInfo(project);
          const [c1] = resolveProjectColors(project);

          return (
            <article
              key={project.id}
              className={`pm-studio-block${running ? " is-lit" : ""}`}
              style={{ "--pm-studio-color": c1 } as CSSProperties}
            >
              <div className="pm-studio-top">
                <div className="pm-studio-info">
                  <div className="pm-studio-title">
                    <span className="pm-studio-beacon" aria-hidden="true" />
                    <strong>{project.name}</strong>
                  </div>
                  <small className="pm-studio-path" title={project.path}>
                    {project.path}
                  </small>
                  <span className="project-management-status" data-state={status?.state ?? "idle"}>
                    {working && !running
                      ? "正在准备启动"
                      : running
                        ? `亮灯运行中${status?.url ? ` · ${status.url}` : ""}`
                        : `熄灯休眠 · ${formatRecency(project)}`}
                  </span>
                </div>

                <div className="pm-studio-facade" aria-hidden="true">
                  {windowPattern.map((bit, wi) => {
                    const litCell = Boolean(bit ^ (index % 2 === 1 && wi % 3 === 0 ? 1 : 0));
                    return <i key={wi} className={litCell ? "on" : ""} />;
                  })}
                </div>
              </div>

              {serviceError && (
                <div className="pm-inline-error">
                  <p className="project-management-error" role="alert">{serviceError}</p>
                  {!running && services && (
                    <button
                      type="button"
                      className="btn secondary pm-mini-btn"
                      disabled={working}
                      onClick={() => onChangeService(project.id, "recheck")}
                    >
                      重新识别启动方式
                    </button>
                  )}
                </div>
              )}

              {linked.length > 0 && (
                <div className="pm-studio-links">
                  {linked.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className="pm-link-chip"
                      onClick={() => onOpenCollaboration?.(item.id)}
                    >
                      <LinkSimple size={12} />
                      <span>{item.name}</span>
                      <ArrowSquareOut size={12} />
                    </button>
                  ))}
                </div>
              )}

              <footer className="pm-studio-footer">
                <div className="pm-studio-footer-left">
                  {services && (
                    <button
                      type="button"
                      className={`btn secondary pm-mini-btn${running ? " project-management-primary" : ""}`}
                      disabled={working}
                      onClick={() => onChangeService(project.id, running ? "stop" : "start")}
                    >
                      {running ? <Moon size={14} /> : <Lightbulb size={14} />}
                      <span>{running ? "熄灯停止" : "点灯启动"}</span>
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn secondary pm-mini-btn"
                    disabled={opening !== null}
                    onClick={() => onOpenProject(project)}
                  >
                    <ArrowSquareOut size={14} />
                    <span>打开工作区</span>
                  </button>
                  {status?.state === "running" && status.url && onBrowse && (
                    <button
                      type="button"
                      className="btn secondary pm-mini-btn"
                      onClick={() => onBrowse(status.url!)}
                    >
                      打开页面
                    </button>
                  )}
                </div>

                <div className="pm-studio-footer-right">
                  {status && (
                    <button
                      type="button"
                      className="btn secondary pm-mini-btn"
                      onClick={() => onOpenLogs(project)}
                    >
                      <FileText size={14} />
                      <span>日志</span>
                    </button>
                  )}
                  <ProjectActionMenu
                    project={project}
                    running={running}
                    working={working}
                    onOpenDialog={onOpenDialog}
                    compact
                  />
                </div>
              </footer>
            </article>
          );
        })}
      </div>

      <ArchivedStrip archived={archived} search={search} onOpenDialog={onOpenDialog} />
    </div>
  );
}

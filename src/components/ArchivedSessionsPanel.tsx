import { getInterfaceLanguage } from "../interfaceLanguage";
import { useTranslation } from "../useInterfaceLanguage";
import { sessionDisplayTitle, sessionDisplayProvider } from "../sessionCapabilities";
import { useMemo, useState } from "react";
import { ArchiveIcon } from "@phosphor-icons/react/dist/csr/Archive";
import { ArrowCounterClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowCounterClockwise";
import { FileTextIcon } from "@phosphor-icons/react/dist/csr/FileText";
import { FolderIcon } from "@phosphor-icons/react/dist/csr/Folder";
import { MagnifyingGlassIcon } from "@phosphor-icons/react/dist/csr/MagnifyingGlass";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import {
  filterArchivedSessions,
  groupArchivedSessions,
  UNGROUPED_ARCHIVE_PROJECT_ID,
} from "../archivedSessions";
import type { ArchivedSessionsSettingsStyle, Project, SessionRecord } from "../types";
import "../styles/settings-creative-modes.css";

interface Props {
  sessions: SessionRecord[];
  projects: Project[];
  onRestore: (session: SessionRecord) => void | Promise<void>;
  onDelete: (session: SessionRecord) => void | Promise<void>;
  onViewTranscript: (session: SessionRecord) => void;
  viewStyle?: ArchivedSessionsSettingsStyle;
}

const dateOptions: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
};
const dateFormatters = {
  "zh-CN": new Intl.DateTimeFormat("zh-CN", dateOptions),
  en: new Intl.DateTimeFormat("en", dateOptions),
};
const formatDate = (time: number) => dateFormatters[getInterfaceLanguage()].format(time);

function formatTinNumber(index: number): string {
  return String(index + 1).padStart(2, "0");
}

export function ArchivedSessionsPanel({
  sessions,
  projects,
  onRestore,
  onDelete,
  onViewTranscript,
  viewStyle = "classic",
}: Props) {
  const t = useTranslation();
  const [query, setQuery] = useState("");
  const [projectId, setProjectId] = useState("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const filteredSessions = useMemo(
    () => filterArchivedSessions(sessions, projects, query, projectId),
    [sessions, projects, query, projectId],
  );
  const groups = useMemo(
    () => groupArchivedSessions(filteredSessions, projects),
    [filteredSessions, projects],
  );
  const archivedProjectIds = useMemo(
    () => new Set(sessions.flatMap((session) => (session.projectId ? [session.projectId] : []))),
    [sessions],
  );
  const hasUngrouped = sessions.some((session) => !session.projectId);

  const sessionIndexMap = useMemo(() => {
    const map = new Map<string, string>();
    filteredSessions.forEach((session, idx) => {
      map.set(session.id, formatTinNumber(idx));
    });
    return map;
  }, [filteredSessions]);

  const selectedSession = useMemo(() => {
    if (filteredSessions.length === 0) return null;
    return filteredSessions.find((s) => s.id === selectedId) ?? filteredSessions[0];
  }, [filteredSessions, selectedId]);

  const projectNameById = useMemo(() => {
    const map = new Map<string, string>();
    projects.forEach((project) => map.set(project.id, project.name));
    return map;
  }, [projects]);

  const renderProviderLabel = (session: SessionRecord) =>
    session.agentRuntimeKind === "oh_my_pi"
      ? "Oh My Pi（已移除）"
      : sessionDisplayProvider(session);

  if (viewStyle === "tea") {
    const selectedTinNo = selectedSession ? (sessionIndexMap.get(selectedSession.id) ?? "01") : "01";
    const selectedProjectName = selectedSession?.projectId
      ? (projectNameById.get(selectedSession.projectId) ?? "未分组")
      : "未分组";
    const selectedProviderLabel = selectedSession ? renderProviderLabel(selectedSession) : "";

    return (
      <div className="archived-settings-panel archived-tea-mode">
        <div className="archived-settings-toolbar">
          <label className="archived-settings-search">
            <MagnifyingGlassIcon size={16} aria-hidden="true" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="搜索归档会话"
              aria-label="搜索归档会话"
            />
          </label>
          <select
            value={projectId}
            onChange={(event) => setProjectId(event.target.value)}
            aria-label="按项目筛选归档会话"
          >
            <option value="all">所有项目</option>
            {projects
              .filter((project) => archivedProjectIds.has(project.id))
              .map((project) => (
                <option value={project.id} key={project.id}>
                  {project.name}
                </option>
              ))}
            {hasUngrouped && <option value={UNGROUPED_ARCHIVE_PROJECT_ID}>{t("未分组")}</option>}
          </select>
        </div>

        <div className="archived-settings-summary">
          <span>{filteredSessions.length} 个会话</span>
          <small>归档只会从首页隐藏会话，不会删除记录或项目文件。</small>
        </div>

        {groups.length === 0 || !selectedSession ? (
          <div className="archived-settings-empty">
            <ArchiveIcon size={28} aria-hidden="true" />
            <strong>{sessions.length === 0 ? "还没有归档会话" : "没有匹配的归档会话"}</strong>
            <span>归档后的会话会保留在这里，可随时恢复。</span>
          </div>
        ) : (
          <>
            <section className="archived-tea-stage unsealed" aria-label="当前启盖查看的归档会话">
              <div className="archived-tea-visual">
                <svg className="archived-tea-svg" viewBox="0 0 248 150" aria-hidden="true">
                  <defs>
                    <linearGradient id="cvTeaBodyGrad" x1="0" y1="0" x2="1" y2="1">
                      <stop offset="0%" stopColor="var(--bg-hover)" />
                      <stop offset="55%" stopColor="var(--bg-elev)" />
                      <stop offset="100%" stopColor="var(--bg-sunken)" />
                    </linearGradient>
                  </defs>
                  <rect x="22" y="130" width="204" height="10" rx="5" fill="var(--bg-hover)" stroke="var(--border-strong)" strokeWidth="1.1" />
                  <ellipse cx="124" cy="130" rx="48" ry="6" fill="rgba(0,0,0,0.22)" />
                  <g opacity="0.45">
                    <rect x="38" y="84" width="32" height="45" rx="7" fill="var(--bg-elev)" stroke="var(--border-strong)" />
                    <rect x="42" y="77" width="24" height="7" rx="3" fill="var(--border-strong)" />
                    <rect x="178" y="88" width="30" height="41" rx="7" fill="var(--bg-elev)" stroke="var(--border-strong)" />
                    <rect x="182" y="81" width="22" height="7" rx="3" fill="var(--border-strong)" />
                  </g>
                  <g className="archived-tea-steam" fill="none" stroke="var(--accent)" strokeWidth="1.7" strokeLinecap="round">
                    <path className="steam-1" d="M134 52 C140 42, 132 32, 139 20" />
                    <path className="steam-2" d="M146 53 C153 41, 145 30, 154 16" />
                    <path className="steam-3" d="M157 55 C163 45, 157 35, 164 25" />
                  </g>
                  <rect x="86" y="56" width="76" height="74" rx="14" fill="url(#cvTeaBodyGrad)" stroke="var(--accent)" strokeOpacity="0.52" strokeWidth="1.5" />
                  <rect x="96" y="51" width="56" height="8" rx="3" fill="var(--bg-hover)" stroke="var(--border-strong)" />
                  <rect x="112" y="65" width="24" height="54" rx="4" fill="#f1e6d2" opacity="0.92" />
                  <line x1="124" y1="73" x2="124" y2="108" stroke="var(--accent)" strokeWidth="1.35" strokeDasharray="3 3" />
                  <circle cx="124" cy="112" r="3.4" fill="var(--accent)" />
                  <text x="124" y="85" textAnchor="middle" fontSize="8.5" fontWeight="700" fill="#3a2c20">茶罐</text>
                  <text x="124" y="99" textAnchor="middle" fontSize="8" fontWeight="600" fill="#7d4a34">#{selectedTinNo}</text>
                  <g className="archived-tea-lid">
                    <rect x="92" y="42" width="64" height="12" rx="5" fill="var(--warn, #e5a94c)" fillOpacity="0.72" stroke="var(--warn, #e5a94c)" strokeWidth="1.1" />
                    <circle cx="124" cy="39" r="5" fill="var(--accent)" />
                  </g>
                </svg>
              </div>

              <div className="archived-tea-inspect">
                <span className="archived-tea-kicker">
                  当前启盖查香 · {selectedProjectName} · 藏罐 #{selectedTinNo}
                </span>
                <h3 className="archived-tea-title" title={sessionDisplayTitle(selectedSession)}>
                  {sessionDisplayTitle(selectedSession)}
                </h3>
                <div className="archived-tea-meta">
                  <em
                    className={`archived-provider-badge${selectedSession.agentRuntimeKind === "oh_my_pi" ? " is-removed" : ""}`}
                  >
                    {selectedProviderLabel}
                  </em>
                  <span>封存于 {formatDate(selectedSession.archivedAt ?? selectedSession.updatedAt)}</span>
                </div>
                <small className="archived-tea-path" title={selectedSession.cwd}>
                  {selectedSession.cwd}
                </small>
                <div className="archived-settings-actions archived-tea-actions">
                  {selectedSession.kind === "claude" && (
                    <button
                      type="button"
                      className="archived-action-view"
                      onClick={() => onViewTranscript(selectedSession)}
                    >
                      <FileTextIcon size={14} aria-hidden="true" />
                      {t("查看记录")}
                    </button>
                  )}
                  <button
                    type="button"
                    className="archived-action-restore"
                    onClick={() => void onRestore(selectedSession)}
                  >
                    <ArrowCounterClockwiseIcon size={14} aria-hidden="true" />
                    取消归档
                  </button>
                  <button
                    type="button"
                    className="danger archived-action-delete"
                    onClick={() => void onDelete(selectedSession)}
                  >
                    <TrashIcon size={14} aria-hidden="true" />
                    永久删除
                  </button>
                </div>
              </div>
            </section>

            <div className="archived-tea-shelves">
              {groups.map((group) => (
                <section className="archived-tea-shelf" key={group.projectId}>
                  <header className="archived-tea-shelf-head">
                    <FolderIcon size={15} aria-hidden="true" />
                    <strong>{group.projectName}</strong>
                    <span>{group.sessions.length} 罐</span>
                  </header>
                  <div className="archived-tea-plank">
                    {group.sessions.map((session) => {
                      const providerLabel = renderProviderLabel(session);
                      const isSelected = selectedSession.id === session.id;
                      return (
                        <button
                          type="button"
                          key={session.id}
                          className={`archived-tea-card${isSelected ? " selected" : ""}`}
                          aria-pressed={isSelected}
                          onClick={() => setSelectedId(session.id)}
                        >
                          <svg className="archived-tea-mini" viewBox="0 0 34 44" aria-hidden="true">
                            <rect
                              x="4"
                              y="13"
                              width="26"
                              height="28"
                              rx="6"
                              fill="var(--bg-sunken)"
                              stroke={isSelected ? "var(--accent)" : "var(--border-strong)"}
                              strokeWidth="1.3"
                            />
                            <rect x="13" y="17" width="8" height="19" rx="2" fill="#f1e6d2" opacity="0.86" />
                            <circle cx="17" cy="32" r="1.8" fill="var(--accent)" />
                            <g className="archived-tea-mini-lid">
                              <rect
                                x="7"
                                y="7"
                                width="20"
                                height="6"
                                rx="2.5"
                                fill="var(--warn, #e5a94c)"
                                fillOpacity="0.65"
                                stroke="var(--warn, #e5a94c)"
                                strokeWidth="0.9"
                              />
                              <circle
                                cx="17"
                                cy="5.5"
                                r="2.2"
                                fill={isSelected ? "var(--accent)" : "var(--warn, #e5a94c)"}
                              />
                            </g>
                          </svg>
                          <span className="archived-tea-card-body">
                            <strong title={sessionDisplayTitle(session)}>
                              {sessionDisplayTitle(session)}
                            </strong>
                            <span className="archived-tea-card-sub">
                              <em className="archived-provider-badge">{providerLabel}</em>
                              <span>{formatDate(session.archivedAt ?? session.updatedAt)}</span>
                            </span>
                          </span>
                        </button>
                      );
                    })}
                  </div>
                </section>
              ))}
            </div>
          </>
        )}
      </div>
    );
  }

  return (
    <div className="archived-settings-panel">
      <div className="archived-settings-toolbar">
        <label className="archived-settings-search">
          <MagnifyingGlassIcon size={16} aria-hidden="true" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索归档会话"
            aria-label="搜索归档会话"
          />
        </label>
        <select
          value={projectId}
          onChange={(event) => setProjectId(event.target.value)}
          aria-label="按项目筛选归档会话"
        >
          <option value="all">所有项目</option>
          {projects
            .filter((project) => archivedProjectIds.has(project.id))
            .map((project) => (
              <option value={project.id} key={project.id}>
                {project.name}
              </option>
            ))}
          {hasUngrouped && <option value={UNGROUPED_ARCHIVE_PROJECT_ID}>{t("未分组")}</option>}
        </select>
      </div>

      <div className="archived-settings-summary">
        <span>{filteredSessions.length} 个会话</span>
        <small>归档只会从首页隐藏会话，不会删除记录或项目文件。</small>
      </div>

      {groups.length === 0 ? (
        <div className="archived-settings-empty">
          <ArchiveIcon size={28} aria-hidden="true" />
          <strong>{sessions.length === 0 ? "还没有归档会话" : "没有匹配的归档会话"}</strong>
          <span>归档后的会话会保留在这里，可随时恢复。</span>
        </div>
      ) : (
        <div className="archived-settings-groups">
          {groups.map((group) => (
            <section className="archived-settings-group" key={group.projectId}>
              <header>
                <FolderIcon size={15} aria-hidden="true" />
                <strong>{group.projectName}</strong>
                <span>{group.sessions.length}</span>
              </header>
              <div className="archived-settings-list">
                {group.sessions.map((session) => {
                  const providerLabel = session.agentRuntimeKind === "oh_my_pi"
                    ? "Oh My Pi（已移除）"
                    : sessionDisplayProvider(session);
                  return (
                    <article className="archived-settings-row" key={session.id}>
                      <div className="archived-settings-copy">
                        <div className="archived-settings-title-line">
                          <strong>{sessionDisplayTitle(session)}</strong>
                          <em className="archived-provider-badge">{providerLabel}</em>
                        </div>
                        <span>
                          {providerLabel} · {formatDate(session.archivedAt ?? session.updatedAt)}
                        </span>
                        <small title={session.cwd}>{session.cwd}</small>
                      </div>
                      <div className="archived-settings-actions">
                        {session.kind === "claude" && (
                          <button type="button" className="archived-action-view" onClick={() => onViewTranscript(session)}>
                            <FileTextIcon size={14} aria-hidden="true" />
                            {t("查看记录")}
                          </button>
                        )}
                        <button type="button" className="archived-action-restore" onClick={() => void onRestore(session)}>
                          <ArrowCounterClockwiseIcon size={14} aria-hidden="true" />
                          取消归档
                        </button>
                        <button
                          className="danger archived-action-delete"
                          type="button"
                          onClick={() => void onDelete(session)}
                        >
                          <TrashIcon size={14} aria-hidden="true" />
                          永久删除
                        </button>
                      </div>
                    </article>
                  );
                })}
              </div>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

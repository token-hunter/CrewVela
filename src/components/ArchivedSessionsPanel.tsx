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
import type { Project, SessionRecord } from "../types";

interface Props {
  sessions: SessionRecord[];
  projects: Project[];
  onRestore: (session: SessionRecord) => void | Promise<void>;
  onDelete: (session: SessionRecord) => void | Promise<void>;
  onViewTranscript: (session: SessionRecord) => void;
}

const dateFormatter = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
});

export function ArchivedSessionsPanel({
  sessions,
  projects,
  onRestore,
  onDelete,
  onViewTranscript,
}: Props) {
  const [query, setQuery] = useState("");
  const [projectId, setProjectId] = useState("all");
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
          {hasUngrouped && <option value={UNGROUPED_ARCHIVE_PROJECT_ID}>未分组</option>}
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
                {group.sessions.map((session) => (
                  <article className="archived-settings-row" key={session.id}>
                    <div className="archived-settings-copy">
                      <strong>{sessionDisplayTitle(session)}</strong>
                      <span>
                        {session.agentRuntimeKind === "oh_my_pi" ? "Oh My Pi（已移除）" : sessionDisplayProvider(session)} · {dateFormatter.format(session.archivedAt ?? session.updatedAt)}
                      </span>
                      <small title={session.cwd}>{session.cwd}</small>
                    </div>
                    <div className="archived-settings-actions">
                      {session.kind === "claude" && (
                        <button type="button" onClick={() => onViewTranscript(session)}>
                          <FileTextIcon size={14} aria-hidden="true" />
                          查看记录
                        </button>
                      )}
                      <button type="button" onClick={() => void onRestore(session)}>
                        <ArrowCounterClockwiseIcon size={14} aria-hidden="true" />
                        取消归档
                      </button>
                      <button
                        className="danger"
                        type="button"
                        onClick={() => void onDelete(session)}
                      >
                        <TrashIcon size={14} aria-hidden="true" />
                        永久删除
                      </button>
                    </div>
                  </article>
                ))}
              </div>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

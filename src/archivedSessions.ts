import type { Project, SessionRecord } from "./types";

export const UNGROUPED_ARCHIVE_PROJECT_ID = "__ungrouped__";

export interface ArchivedSessionGroup {
  projectId: string;
  projectName: string;
  sessions: SessionRecord[];
}

export function filterArchivedSessions(
  sessions: SessionRecord[],
  projects: Project[],
  query: string,
  projectId: string,
): SessionRecord[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const projectNames = new Map(projects.map((project) => [project.id, project.name]));

  return sessions
    .filter((session) => {
      if (projectId === UNGROUPED_ARCHIVE_PROJECT_ID && session.projectId) return false;
      if (
        projectId !== "all" &&
        projectId !== UNGROUPED_ARCHIVE_PROJECT_ID &&
        session.projectId !== projectId
      ) {
        return false;
      }
      if (!normalizedQuery) return true;
      const haystack = [
        session.title,
        session.providerName,
        session.cwd,
        session.agentRuntimeKind ?? "",
        session.projectId ? projectNames.get(session.projectId) ?? "" : "未分组",
      ]
        .join("\n")
        .toLocaleLowerCase();
      return haystack.includes(normalizedQuery);
    })
    .sort(
      (left, right) =>
        (right.archivedAt ?? right.updatedAt) - (left.archivedAt ?? left.updatedAt),
    );
}

export function groupArchivedSessions(
  sessions: SessionRecord[],
  projects: Project[],
): ArchivedSessionGroup[] {
  const projectNames = new Map(projects.map((project) => [project.id, project.name]));
  const groups = new Map<string, ArchivedSessionGroup>();

  for (const session of sessions) {
    const projectId = session.projectId ?? UNGROUPED_ARCHIVE_PROJECT_ID;
    let group = groups.get(projectId);
    if (!group) {
      group = {
        projectId,
        projectName:
          projectId === UNGROUPED_ARCHIVE_PROJECT_ID
            ? "未分组"
            : projectNames.get(projectId) ?? "已移除的项目",
        sessions: [],
      };
      groups.set(projectId, group);
    }
    group.sessions.push(session);
  }

  return [...groups.values()].sort((left, right) => {
    if (left.projectId === UNGROUPED_ARCHIVE_PROJECT_ID) return 1;
    if (right.projectId === UNGROUPED_ARCHIVE_PROJECT_ID) return -1;
    return left.projectName.localeCompare(right.projectName, "zh-CN");
  });
}

import assert from "node:assert/strict";
import test from "node:test";
import {
  filterArchivedSessions,
  groupArchivedSessions,
  UNGROUPED_ARCHIVE_PROJECT_ID,
} from "../src/archivedSessions.ts";
import type { Project, SessionRecord } from "../src/types.ts";

const projects: Project[] = [
  {
    id: "project-a",
    name: "CrewVela",
    path: "C:/example/crewvela",
    badgeColor: "",
    createdAt: 1,
    updatedAt: 1,
  },
];

function archivedSession(
  id: string,
  title: string,
  projectId: string | null,
  archivedAt: number,
): SessionRecord {
  return {
    id,
    claudeSessionId: id,
    kind: "claude",
    shell: null,
    projectId,
    providerId: "kimi",
    providerName: "Kimi K3",
    cwd: projectId ? "C:/example/crewvela" : "C:/Users/demo",
    title,
    status: "stopped",
    ptyId: null,
    yolo: false,
    autoMode: false,
    permissionMode: null,
    titleLocked: true,
    exitCode: 0,
    lastError: null,
    archivedAt,
    createdAt: 1,
    updatedAt: archivedAt,
  };
}

const sessions = [
  archivedSession("a", "修复归档功能", "project-a", 100),
  archivedSession("b", "独立会话", null, 200),
];

test("归档搜索会匹配会话、模型和项目名称", () => {
  assert.deepEqual(
    filterArchivedSessions(sessions, projects, "CrewVela", "all").map((item) => item.id),
    ["a"],
  );
  assert.equal(filterArchivedSessions(sessions, projects, "Kimi", "all").length, 2);
});

test("归档筛选支持项目和未分组，并按归档时间倒序", () => {
  assert.deepEqual(
    filterArchivedSessions(sessions, projects, "", UNGROUPED_ARCHIVE_PROJECT_ID).map(
      (item) => item.id,
    ),
    ["b"],
  );
  assert.deepEqual(
    filterArchivedSessions(sessions, projects, "", "all").map((item) => item.id),
    ["b", "a"],
  );
});

test("归档会话按项目分组，未分组显示在最后", () => {
  const groups = groupArchivedSessions(sessions, projects);
  assert.deepEqual(
    groups.map((group) => [group.projectName, group.sessions.length]),
    [
      ["CrewVela", 1],
      ["未分组", 1],
    ],
  );
});

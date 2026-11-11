import { useState } from "react";
import { createRoot } from "react-dom/client";
import { SessionList } from "../../src/components/SessionList";
import type { SessionRecord, SessionTypeFilter } from "../../src/types";
import "../../src/styles.css";

function makeSession(id: string, title: string, kind: SessionRecord["kind"] = "claude", index = 1): SessionRecord {
  return {
    id,
    claudeSessionId: id,
    kind,
    shell: kind === "terminal" ? "powershell" : null,
    providerId: kind === "claude" ? "glm-5.2" : "local-terminal",
    providerName: kind === "claude" ? "智谱 GLM" : "PowerShell",
    cwd: `C:\\example\\crewvela\\workspace-${index}`,
    title,
    status: "running",
    ptyId: index,
    yolo: false,
    autoMode: false,
    permissionMode: "default",
    titleLocked: false,
    exitCode: null,
    lastError: null,
    createdAt: index,
    updatedAt: index,
  };
}

const initialSessions = [
  makeSession("session-main", "为当前项目补充跨模块 Agent 接力与长标题裁切验证的会话", "claude", 1),
  makeSession("session-shell", "临时终端", "terminal", 2),
  makeSession("session-three", "接口检查", "claude", 3),
  makeSession("session-four", "构建检查", "claude", 4),
  makeSession("session-last", "列表底部会话", "claude", 5),
];

const pinFixture = new URLSearchParams(location.search).has("pins");
const pinSessions = initialSessions.map((session, index) => ({
  ...session,
  projectId: index === 1 ? null : index === 4 ? "project-b" : "project-a",
}));
const pinProjects = ["a", "b"].map((id) => ({
  id: `project-${id}`, name: `项目 ${id.toUpperCase()}`, path: `/fixture/${id}`,
  badgeColor: "blue", createdAt: 1, updatedAt: 1,
}));

function SessionListActionsFixture() {
  const [selectedId, setSelectedId] = useState<string | null>("session-shell");
  const [events, setEvents] = useState<string[]>([]);
  const [typeFilter, setTypeFilter] = useState<SessionTypeFilter>("all");
  const record = (action: string) => (session: SessionRecord) => {
    setEvents(current => [...current, `${action}:${session.id}`]);
  };

  return <main className="fixture-shell">
    <aside className="fixture-sidebar" aria-label="窄侧栏会话列表">
      <SessionList
        sessions={pinFixture ? pinSessions : initialSessions}
        projects={pinFixture ? pinProjects : []}
        usages={{}}
        selectedId={selectedId}
        loadingId={null}
        typeFilter={typeFilter}
        onTypeFilter={setTypeFilter}
        onNewSessionInProject={pinFixture ? project => setEvents(current => [...current, `new:${project.id}`]) : undefined}
        onRenameProject={pinFixture ? project => setEvents(current => [...current, `rename-project:${project.id}`]) : undefined}
        onDeleteProject={pinFixture ? project => setEvents(current => [...current, `delete-project:${project.id}`]) : undefined}
        onArchiveProjectSessions={pinFixture ? project => setEvents(current => [...current, `archive-project:${project.id}`]) : undefined}
        onSelect={setSelectedId}
        onRename={record("rename")}
        onStop={record("stop")}
        onArchive={record("archive")}
        onDelete={record("delete")}
        onViewTranscript={record("transcript")}
        onSwitchAgent={record("handoff")}
        sourceHistory={session => session.id === "session-main" ? session : null}
        onRelayDetails={record("relay")}
      />
    </aside>
    <section className="fixture-context">
      <h1>窄侧栏会话操作验证</h1>
      <p>标题需要省略显示，更多按钮有固定位置；菜单浮在滚动列表之外，删除操作放在分隔线下方。</p>
      <button type="button" id="outside-control">侧栏外控件</button>
      <p id="selection-status">selected:{selectedId}</p>
      <p id="action-log" aria-live="polite">{events.join(" | ") || "暂无操作"}</p>
    </section>
  </main>;
}

createRoot(document.getElementById("root")!).render(<SessionListActionsFixture />);

import { createRoot } from "react-dom/client";
import { CollaborationSettingsPanel, COLLABORATION_UI_PREVIEW_SNAPSHOT } from "../../src/components/CollaborationSettingsPanel";
import { MultiAgentWorkspace } from "../../src/components/MultiAgentWorkspace";
import "../../src/styles.css";
const base = COLLABORATION_UI_PREVIEW_SNAPSHOT;
const projects = ["A", "B"].map(id => ({ ...base.project, id, name: `项目 ${id}` }));
const snapshot = (id: string) => ({ ...base, project: projects.find(item => item.id === id)!,
  agents: [{ ...base.agents[0], id: `${id}-agent`, name: `${id}成员` }],
  agentSessions: [{ agentId: `${id}-agent`, appSessionId: `${id}-session`, status: "idle" }],
  messages: [{ id: `${id}-message`, senderId: `${id}-agent`, targetAgentId: null, createdAt: Date.now(), body: `${id}项目完整消息\n第二行正文`, senderType: "agent" }],
  events: [{ seq: 1, createdAt: Date.now()-1000, eventType: "message.sent", payload: {}, entityType: "project", entityId: id }],
  runtimeEvents: [], artifacts: [], memories: [], runs: [], tasks: [],
});
localStorage.setItem("crewvela-multi-agent-project", "A");
const calls: any[] = []; const pending: any[] = [];
const mock = { calls, pending, holdDelivery: false, failDelivery: false,
  invoke(command: string, args: any) {
    calls.push({ command, args });
    if (command === "list_delivery_records") {
      if (mock.failDelivery) return Promise.reject(new Error("fixture delivery failure"));
      if (mock.holdDelivery) return new Promise(resolve => pending.push(resolve));
      return Promise.resolve(["A", "B", "foreign"].map(id => ({ id, at: Date.now()-2000, fromSessionId: null, fromLabel: "用户", toSessionId: `${id}-session`, body: `${id}投递正文`, kind: "direct" })));
    }
    if (command === "ma_list_projects") return Promise.resolve(projects);
    if (command === "ma_snapshot") return Promise.resolve(snapshot(args.projectId));
    if (command === "collaboration_health") return Promise.resolve({ cliPath: "fixture", cliPathExists: true, replyEvidence: "fixture", runtimes: [], messages: [], handoffs: [], sessions: [] });
    if (command === "get_runtime_preferences") return Promise.resolve({ disabledRuntimes: [] });
    if (command === "skill_center_snapshot") return Promise.resolve({ skillSets: [] });
    if (command === "plugin:event|listen") return Promise.resolve(1);
    return Promise.resolve([]);
  },
  resolveDelivery(value: any) { pending.shift()(value); },
};
(window as any).recordMock = mock;
const mode=new URLSearchParams(location.search).get("mode");
createRoot(document.getElementById("root")!).render(mode === "workspace"
  ? <div style={{ height: "100vh" }}><MultiAgentWorkspace projectId="A" sessions={[]} theme="dark" onSessionsChange={()=>{}} onCloseSession={()=>{}} onClose={()=>{}} /></div>
  : <CollaborationSettingsPanel initialTab="records" onClose={()=>{}} />);

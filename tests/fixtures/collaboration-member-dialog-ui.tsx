import { useState } from "react";
import { createRoot } from "react-dom/client";
import { CollaborationSettingsPanel, COLLABORATION_UI_PREVIEW_SNAPSHOT } from "../../src/components/CollaborationSettingsPanel";
import "../../src/styles.css";

// Only this fixture replaces IPC; no native accounts or personal data are used.
const project = { ...COLLABORATION_UI_PREVIEW_SNAPSHOT.project, id: "member-fixture", name: "窗口回归项目", rootPath: "/fixture/project", status: "paused" };
let snapshot: any = { ...structuredClone(COLLABORATION_UI_PREVIEW_SNAPSHOT), project, agents: COLLABORATION_UI_PREVIEW_SNAPSHOT.agents.slice(0, 2).map(a => ({ ...a, projectId: project.id })), tasks: [], events: [] };
let failNext = false;
let saves = 0;
let report: (text: string) => void = () => {};
(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any) {
  if (command === "plugin:event|listen") return 1;
  if (command === "plugin:event|unlisten") return;
  if (command === "get_runtime_preferences") return { disabledRuntimes: [] };
  if (command === "ma_list_projects") return [project];
  if (command === "ma_snapshot") return structuredClone(snapshot);
  if (["ma_probe_runtimes", "ma_check_runtime_updates"].includes(command)) return snapshot.agents.map((a: any) => ({ kind: a.runtimeKind, installed: true, version: "fixture", capabilities: [], error: null }));
  if (command === "skill_center_snapshot") return { skillSets: [] };
  if (command === "collaboration_health") return { sessions: [], runtimes: [], messages: [], handoffs: [], cliPath: "fixture", cliPathExists: true, replyEvidence: "fixture" };
  if (["ma_create_agent", "ma_update_agent"].includes(command)) {
    if (failNext) { failNext = false; throw Error("回归测试：保存失败"); }
    const { agentId, ...draft } = args.input;
    if (agentId) snapshot.agents = snapshot.agents.map((a: any) => a.id === agentId ? { ...a, ...draft } : a);
    else snapshot.agents.push({ ...draft, id: `created-${++saves}`, createdAt: 1, updatedAt: 1, capabilities: [] });
    report(`保存成功：${draft.name}`);
    return;
  }
  return [];
} };

function Fixture() {
  const [contained, setContained] = useState(true);
  const [result, setResult] = useState("");
  report = setResult;
  return <main className="member-window-fixture">
    <style>{`.member-window-fixture{min-height:100vh;padding:16px;background:var(--bg);color:var(--text)}.member-fixture-controls{display:flex;gap:16px;margin-bottom:16px}.member-fixture-host{height:560px;overflow:auto;border:1px solid var(--border);padding:24px}.member-fixture-host.contained{transform:translateZ(0);contain:paint}.member-fixture-host .collab-project-runtime-bar{display:none}`}</style>
    <header className="member-fixture-controls"><label><input type="checkbox" checked={contained} onChange={e => setContained(e.target.checked)} />模拟设置页裁切容器</label><button onClick={() => { failNext = true; }}>下次保存失败</button><output>{result}</output></header>
    <div className={`member-fixture-host${contained ? " contained" : ""}`}><CollaborationSettingsPanel onClose={() => {}} initialTab="team" initialProjectId={project.id} /></div>
  </main>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);

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
const fixtureState = new URLSearchParams(location.search).get("state");
if(fixtureState === "legacy-desktop") snapshot.agents[0]={...snapshot.agents[0],name:"已有 Desktop 成员",runtimeKind:"zcode",model:"desktop/kept-model",permissionProfile:"auto"};
const policyMock = { failLoad: false, failSave: false, stale: false, calls: [] as any[], previews: [] as any[], policies: {} as Record<string, any>, archives: [] as any[] };
(window as any).policyMock = policyMock;
if (fixtureState === "policy") {
  snapshot.runs = [];
  snapshot.agents[0].description = "负责模型设置验收。只回复指定标记，不调用工具。";
  snapshot.project.goal = "模型设置验收，只回复验收标记，不调用工具。";
}
const policySnapshot = () => ({
  projectPolicy: policyEntry(null), memberPolicies: snapshot.agents.map((agent: any) => policyEntry(agent.id)), archivedTaskRules: policyMock.archives,
  runtimeCapabilities: snapshot.agents.map((agent: any) => ({ runtimeKind: agent.runtimeKind, research: true, replyOnly: true, networkControl: true, pathControl: true, status: "adapted", detail: "隔离 UI 数据，不代表真实工具验证" })),
});
function policyEntry(agentId: string | null) {
  const legacyText = agentId ? snapshot.agents.find((agent: any) => agent.id === agentId)?.description : snapshot.project.goal;
  return { projectId: project.id, agentId, legacyText, legacyReviewRequired: legacyText?.includes("不调用工具"), boundary: policyMock.policies[agentId ?? "project"] ?? { tools: "research", network: "public_web", deniedReadPaths: [] } };
}
if (fixtureState === "agents-empty") snapshot.agents = [];
const projects: any[] = fixtureState === "empty" ? [] : [project];
const snapshots = new Map([[project.id, snapshot]]);
const projectMock = { creates: [] as any[], agentSaves: [] as any[], holdCreate: false, releaseCreate: null as (() => void) | null, pickerResult: null as string | null, pickerError: false };
(window as any).projectMock = projectMock;
(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any) {
  if (command === "plugin:event|listen") return 1;
  if (command === "plugin:event|unlisten") return;
  if (command === "get_runtime_preferences") return { disabledRuntimes: [] };
  if (command === "ma_conversation_snapshot") { if (policyMock.failLoad) throw Error("隔离测试：权限读取失败"); return structuredClone(policySnapshot()); }
  if (command === "ma_conversation_policy_set") {
    policyMock.calls.push(structuredClone(args.input));
    if (policyMock.failSave) throw Error("隔离测试：限制保存失败");
    policyMock.policies[args.input.agentId ?? "project"] = structuredClone(args.input.boundary);
    return structuredClone(policySnapshot());
  }
  if (command === "ma_conversation_migration_preview") {
    const entry = policyEntry(args.input.agentId);
    const preview = { ...args.input, id: `preview-${policyMock.previews.length}`, previousText: entry.legacyText, archivedRules: entry.legacyText, createdAt: 1 };
    policyMock.previews.push(preview); return structuredClone(preview);
  }
  if (command === "ma_conversation_migration_apply") {
    const preview = policyMock.previews.find(value => value.id === args.input.previewId);
    if (policyMock.stale || policyEntry(preview.agentId).legacyText !== preview.previousText) throw Error("原文已变化，请重新预览");
    if (preview.agentId) snapshot.agents.find((agent: any) => agent.id === preview.agentId).description = preview.replacementText;
    else snapshot.project.goal = preview.replacementText;
    policyMock.archives.push({ id: preview.id, agentId: preview.agentId, label: "原验收规则", tools: "none", instructions: preview.previousText, createdAt: 1 });
    return structuredClone(policySnapshot());
  }
  if (command === "ma_list_projects") {
    if (fixtureState === "projects-pending") return new Promise(() => {});
    if (fixtureState === "projects-failed") throw Error("回归测试：项目读取失败");
    return [...projects];
  }
  if (command === "ma_snapshot") {
    if (fixtureState === "snapshot-pending") return new Promise(() => {});
    snapshot = snapshots.get(args.projectId) ?? snapshot;
    return structuredClone(snapshot);
  }
  if (command === "plugin:dialog|open") {
    if (projectMock.pickerError) throw Error("回归测试：目录选择失败");
    return projectMock.pickerResult;
  }
  if (command === "ma_create_project") {
    projectMock.creates.push(args.input);
    if (projectMock.holdCreate) await new Promise<void>(resolve => { projectMock.releaseCreate = resolve; });
    if (failNext) { failNext = false; throw Error("回归测试：创建失败"); }
    const created = { ...project, ...args.input, id: `created-project-${projectMock.creates.length}` };
    projects.push(created);
    snapshots.set(created.id, { ...structuredClone(snapshot), project: created, agents: [], tasks: [], events: [] });
    return created;
  }
  if (["ma_probe_runtimes", "ma_check_runtime_updates"].includes(command)) return snapshot.agents.map((a: any) => ({ kind: a.runtimeKind, installed: true, version: "fixture", capabilities: [], error: null, ...(fixtureState === "runtime-update-failed" ? { version: "1.0.0", latestVersion: "2.0.0", updateAvailable: true, canAutoUpdate: true } : {}) }));
  if (command === "ma_upgrade_runtime" && fixtureState === "runtime-update-failed") throw Error("回归测试：升级失败");
  if (command === "skill_center_snapshot") return { skillSets: [] };
  if (command === "collaboration_health") return { sessions: [], runtimes: [], messages: [], handoffs: [], cliPath: "fixture", cliPathExists: true, replyEvidence: "fixture" };
  if (["ma_create_agent", "ma_update_agent"].includes(command)) {
    projectMock.agentSaves.push(structuredClone(args.input));
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
    <div className={`member-fixture-host${contained ? " contained" : ""}`}><CollaborationSettingsPanel onClose={() => setResult("返回应用")} initialTab="team" initialProjectId={fixtureState ? undefined : project.id} /></div>
  </main>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);

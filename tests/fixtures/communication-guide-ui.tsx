import { useState } from "react";
import { createRoot } from "react-dom/client";
import { CommunicationGuide } from "../../src/components/CommunicationGuide";
import { communicationMembers, COMMUNICATION_SEEN_KEY } from "../../src/communicationGuide";
import type { AgentProfile } from "../../src/types";
import "../../src/styles.css";
import "../../src/styles/cowork.css";

const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get('theme') === 'light' ? 'light' : 'dark';
document.documentElement.dataset.appearanceAccent = params.get('accent') ?? 'clay';

// Visible test controls drive the real component; no models or desktop state are touched.
const projects = [{ id: "a", name: "项目甲" }, { id: "b", name: "项目乙" }];
let failList = false, failCopy = false, holdA = false;
let releaseA: (() => void) | undefined;
const calls: string[] = [];
let copied = "";
let holdCopy = false;
let releaseCopy: (() => void) | undefined;
const navigation: Array<{kind: string; projectId?: string; scope?: string}> = [];
const api = { calls, navigation, get copied() { return copied; },
  setFailList(value: boolean) { failList = value; }, setFailCopy(value: boolean) { failCopy = value; },
  holdA(value: boolean) { holdA = value; }, releaseA() { releaseA?.(); },
  holdCopy(value: boolean) { holdCopy = value; }, releaseCopy() { releaseCopy?.(); },
  setSessions: (_value: any[]) => {},
};
Object.assign(window, { communicationMock: api });
const agents = (projectId: string) => [
  { id: `${projectId}-claude`, name: `${projectId === "a" ? "甲" : "乙"}前端`, projectId, enabled: true, runtimeKind: "claude_code" },
  { id: `${projectId}-codex`, name: `${projectId === "a" ? "甲" : "乙"}审查`, projectId, enabled: true, runtimeKind: "codex" },
  { id: `${projectId}-pi`, name: `${projectId === "a" ? "甲" : "乙"}文档`, projectId, enabled: true, runtimeKind: "pi" },
  { id: "foreign", name: "其他项目成员", projectId: "foreign", enabled: true, runtimeKind: "codex" },
  { id: "disabled", name: "禁用成员", projectId, enabled: false, runtimeKind: "codex" },
] as AgentProfile[];
const snapshot = (projectId: string) => ({ project: projects.find(p => p.id === projectId), agents: agents(projectId) });
(window as any).__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
  calls.push(command);
  if (command === "ma_list_projects") { if (failList) throw new Error("模拟项目读取失败"); return projects; }
  if (command === "ma_snapshot") {
    if (args.projectId === "a" && holdA) return new Promise(resolve => { releaseA = () => resolve(snapshot("a")); });
    return snapshot(args.projectId);
  }
  if (command === "plugin:clipboard-manager|write_text") { if (failCopy) throw new Error("模拟剪贴板拒绝"); if (holdCopy) return new Promise<void>(resolve => { releaseCopy = () => { copied = args.text; resolve(); }; }); copied = args.text; return; }
  throw new Error(`不允许的测试调用：${command}`);
} };
function Fixture() {
  const [open, setOpen] = useState(true), [version, setVersion] = useState(0), [output, setOutput] = useState("");
  const [sessions, setSessions] = useState(() => {
    const items = params.has('sample') ? [
      {id:'sample-codex',name:'Codex CLI',runtime:'Codex',family:'codex'},
      {id:'sample-opencode',name:'OpenCode',runtime:'OpenCode',family:'opencode'},
    ] : communicationMembers(agents('a'), 'a');
    if(params.get('sessions') === 'none') return [];
    if(params.get('sessions') === 'one') return items.slice(0,1);
    if(params.get('sessions') === 'long') return items.map(item=>({...item,name:item.name+'很长的工作会话名称'.repeat(8)}));
    return items;
  });
  api.setSessions = setSessions;
  return <main style={{ height: "100vh", position: "relative", background: "var(--bg)", color: "var(--text)", padding: 16, boxSizing: "border-box" }}>
    <div style={{ maxWidth: "calc(100% - 400px)" }}><h1>会话通信交互回归</h1>
      <button onClick={() => { setVersion(v => v + 1); setOpen(true); }}>重新打开</button>
      <button onClick={() => { localStorage.removeItem(COMMUNICATION_SEEN_KEY); setOutput("预览提示已重置，刷新工作区查看"); }}>重置预览提示</button>
      <label><input type="checkbox" onChange={e => { failList = e.target.checked; }} />项目读取失败</label>
      <label><input type="checkbox" onChange={e => { failCopy = e.target.checked; }} />复制失败</label>
      <label><input type="checkbox" onChange={e => { holdA = e.target.checked; }} />延迟项目甲</label>
      <button onClick={() => releaseA?.()}>释放项目甲响应</button>
      <button onClick={() => setOutput(JSON.stringify({ calls, copied, forbidden: calls.filter(c => !["ma_list_projects", "ma_snapshot", "plugin:clipboard-manager|write_text"].includes(c)) }))}>查看调用记录</button>
      <output style={{ display: "block", whiteSpace: "pre-wrap" }}>{output}</output>
    </div>
    {open && <CommunicationGuide key={version} initialScope={params.get('scope') === 'project' ? 'project' : 'sessions'} initialProjectId={params.get('project')} sessions={sessions} onClose={() => setOpen(false)} onDiagnostics={(projectId, scope) => { navigation.push({kind:'diagnostics',projectId,scope}); setOutput(`诊断范围：${scope}，项目：${projectId ?? "全局"}`); }} onOpenCollaboration={projectId=>{navigation.push({kind:'center',projectId});setOutput(`协作中心：${projectId ?? '全局'}`);}} />}
  </main>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);

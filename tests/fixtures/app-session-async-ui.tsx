import { createRoot } from "react-dom/client";
import App from "../../src/App";
import type { SessionRecord } from "../../src/types";
import { navigationProjects, navigationSnapshot, navigationConversation } from "./collaboration-task-navigation-data";
import { GENERAL_SETTINGS_PREVIEW } from "../../src/generalSettings";
import "../../src/styles.css";
import "../../src/styles/cowork.css";

let sessions: SessionRecord[] = ["one", "two"].map((id, i) => ({
  id, title: `fixture-${id}`, kind: "terminal", shell: "powershell", claudeSessionId: "",
  providerId: "local-terminal", providerName: "PowerShell", cwd: "C:\\fixture",
  status: "stopped", ptyId: null, yolo: false, autoMode: false, permissionMode: null,
  titleLocked: true, exitCode: 0, lastError: null, createdAt: i, updatedAt: i,
}));
const bulkFixture = new URLSearchParams(location.search).get("bulk-actions");
if (bulkFixture) {
  sessions = [
    { ...sessions[0], status: "running", ptyId: 1 }, sessions[1],
    { ...sessions[0], id: "three", title: "fixture-ai", kind: "claude", agentRuntimeKind: "codex", providerId: "local", providerName: "Codex", shell: null, status: "running", ptyId: 3 },
    { ...sessions[1], id: "four", title: "fixture-hidden", kind: "claude", agentRuntimeKind: "codex", providerId: "local", providerName: "Codex", shell: null },
    { ...sessions[0], id: "other", title: "other-workspace", status: "running", ptyId: 5 },
  ];
  if (bulkFixture === "empty") sessions = sessions.filter(session => session.id === "other");
  if (bulkFixture === "stopped") sessions = sessions.map(session => session.id === "other" ? session : { ...session, status: "stopped", ptyId: null });
}
if (new URLSearchParams(location.search).has("retired-runtime")) {
  sessions[0] = { ...sessions[0], agentRuntimeKind: "removed_runtime" as SessionRecord["agentRuntimeKind"] };
}
const pending: Array<{ id: string; resolve: (value: SessionRecord) => void; reject: (reason: Error) => void }> = [];
const calls: string[] = [];
const callbacks = new Map<number, (event: unknown) => void>();
const listeners = new Map<string, number>();
let providers = [{ id: "fixture-provider", name: "Fixture provider", baseUrl: "https://example.com", authTokenRef: "fixture", env: {}, builtin: false, timeoutMs: null, disableNonessentialTraffic: false }];
// 轮询命令的返回内容由测试控制；每次都是新对象，键顺序也随机（Rust HashMap 的序列化顺序就是如此）。
const shuffledKeys = (value: Record<string, unknown>) => Object.fromEntries(Object.entries(value).sort(() => Math.random() - 0.5));
const api = {
  cliLaunch: {
    defaults: JSON.parse(localStorage.getItem('fixture-cli-launch-defaults') ?? '{"claude_code":"auto","codex":"read_only"}') as Record<string, string>,
    calls: [] as Array<{command: string; args: any}>,
    failSave: false, failLaunch: false, hold: false,
    pickerValue: 'C:\\fixture' as string | null, pickerFails: false, pickerHold: false,
    pickerCalls: [] as unknown[], pickerPending: [] as Array<() => void>,
    releasePicker() { api.cliLaunch.pickerPending.shift()?.(); },
    pending: [] as Array<() => void>,
    release() { api.cliLaunch.pending.shift()?.(); },
  },
  fullscreen: {
    value: false, hold: false, fail: false, readFails: false,
    maximized: false, unmaximizeFails: false, maximizeFails: false,
    transitions: [] as string[],
    calls: [] as Array<{label:unknown;value:unknown}>,
    pending: [] as Array<() => void>,
    complete() { const finish=api.fullscreen.pending.shift(); if(!finish)throw Error("No pending fullscreen request"); finish(); },
    external(value:boolean) { api.fullscreen.value=value; api.emit("tauri://resize",{width:1440,height:900}); },
  },
  automationAccentCalls: [] as Array<{command:string;args:unknown}>,
  bulkCalls: [] as Array<{command:string;sessionId:string}>,
  rejectBulkDelete: "", rejectBulkStop: "",
  bulkSessions: () => structuredClone(sessions),
  addBulkSession() { sessions = [...sessions, { ...sessions[0], id: "arrived", title: "newly-arrived", status: "stopped", ptyId: null }]; },
  replaceBulkSession() { sessions = sessions.map((session,index) => index === 0 ? { ...session, id: "replacement", title: "replacement-session" } : session); },
  help: {
    checks: 0, version: "0.1.0", versionFails: false, openFails: false, opens: [] as string[],
    pending: [] as Array<{resolve:(value:unknown)=>void;reject:(reason:Error)=>void}>,
    complete(value:unknown) { const entry=api.help.pending.shift(); if(!entry)throw Error("No pending app update check"); entry.resolve(value); },
    fail(message:string) { const entry=api.help.pending.shift(); if(!entry)throw Error("No pending app update check"); entry.reject(new Error(message)); },
  },
  calls, pending, rejectProviderDelete: true,
  rejectSwitchDismiss: false, switchCommands: [] as string[],
  usages: {} as Record<string, unknown>,
  switchRecords: [] as unknown[],
  registerCallback(fn: (event: unknown) => void) { const id = callbacks.size + 1; callbacks.set(id, fn); return id; },
  emit(event: string, payload: unknown) { callbacks.get(listeners.get(event)!)?.({ event, payload }); },
  release(id: string, failure?: string) {
    const index = pending.findIndex(item => item.id === id);
    if (index < 0) throw new Error(`Missing pending resume: ${id}`);
    const entry = pending.splice(index, 1)[0];
    if (failure) entry.reject(new Error(failure));
    else {
      const resumed: SessionRecord = { ...sessions.find(item => item.id === id)!, ptyId: id === "one" ? 1 : 2, status: "running" };
      sessions = sessions.map(item => item.id === id ? resumed : item);
      entry.resolve(resumed);
    }
  },
  async invoke(command: string, args?: Record<string, unknown>) {
    if (location.search.includes('cli-launch')) {
      if (command === 'list_projects') return [{id:'fixture-project',name:'Fixture project',path:'C:\\fixture',createdAt:0}, {id:'second-project',name:'Second project',path:'D:\\Second 项目',createdAt:0}];
      if (command === 'plugin:dialog|open') {
        api.cliLaunch.pickerCalls.push(args);
        if (api.cliLaunch.pickerFails) throw Error('fixture 目录选择失败');
        if (api.cliLaunch.pickerHold) await new Promise<void>(resolve => api.cliLaunch.pickerPending.push(resolve));
        return api.cliLaunch.pickerValue;
      }
      if (command === 'get_runtime_preferences') return {disabledRuntimes:[],defaultLaunchModes:structuredClone(api.cliLaunch.defaults)};
      if (command === 'set_runtime_launch_default') {
        api.cliLaunch.calls.push({command,args});
        if (api.cliLaunch.failSave) throw Error('fixture 默认模式写入失败');
        api.cliLaunch.defaults[String(args?.kind)] = String(args?.mode);
        localStorage.setItem('fixture-cli-launch-defaults', JSON.stringify(api.cliLaunch.defaults));
        const prefs = {disabledRuntimes:[],defaultLaunchModes:structuredClone(api.cliLaunch.defaults)};
        api.emit('runtime-preferences-changed',prefs);return prefs;
      }
      if (command === 'launch_runtime_session') {
        api.cliLaunch.calls.push({command,args});
        if (api.cliLaunch.failLaunch) throw Error('fixture CLI 拒绝启动模式');
        const finish=()=>{
          const record:SessionRecord={...sessions[0],id:`cli-${api.cliLaunch.calls.length}`,title:String(args?.runtimeKind),kind:'terminal',shell:null,agentRuntimeKind:args?.runtimeKind as any,providerName:String(args?.runtimeKind),providerId:`local-runtime-${args?.runtimeKind}`,status:'running',ptyId:999,cwd:String(args?.cwd),projectId:args?.projectId as string|null,cliLaunchMode:String(args?.launchMode)};
          sessions=[...sessions,record];return record;
        };
        if(api.cliLaunch.hold)return new Promise<SessionRecord>(resolve=>api.cliLaunch.pending.push(()=>resolve(finish())));
        return finish();
      }
    }
    if(command==="plugin:window|is_fullscreen") { if(api.fullscreen.readFails)throw Error("fixture fullscreen read rejected");return api.fullscreen.value; }
    if(command==="plugin:window|is_maximized") return api.fullscreen.maximized;
    if(command==="plugin:window|unmaximize") {
      api.fullscreen.transitions.push("unmaximize");
      if(api.fullscreen.unmaximizeFails)throw Error("fixture unmaximize rejected");
      api.fullscreen.maximized=false;return;
    }
    if(command==="plugin:window|maximize") {
      api.fullscreen.transitions.push("maximize");
      if(api.fullscreen.maximizeFails)throw Error("fixture maximize rejected");
      api.fullscreen.maximized=true;return;
    }
    if(command==="plugin:window|set_fullscreen") {
      api.fullscreen.transitions.push(`fullscreen:${args?.value}`);
      api.fullscreen.calls.push({label:args?.label,value:args?.value});
      if(api.fullscreen.fail)throw Error("fixture fullscreen rejected");
      const finish=()=>{api.fullscreen.value=args?.value===true;};
      if(api.fullscreen.hold)return new Promise<void>(resolve=>api.fullscreen.pending.push(()=>{finish();resolve();}));
      finish();return;
    }
    if (new URLSearchParams(location.search).has("automation-accent")) {
      api.automationAccentCalls.push({command,args});
      if(command==="list_automations")return structuredClone((window as any).__automationSeed ?? []);
      if(command==="general_settings_get")return GENERAL_SETTINGS_PREVIEW;
      if(command==="ma_list_projects")return [];
      if(command==="ma_probe_runtimes")return [{kind:"codex",installed:true,executable:"fixture",version:"fixture",capabilities:[],error:null}];
      if(["create_automation","run_automation_now","set_automation_enabled","stop_automation_run","delete_automation"].includes(command))throw Error("Appearance fixture must not mutate automations");
    }
    if (new URLSearchParams(location.search).has("tasks-nav")) {
      if(command==="ma_list_projects")return structuredClone(navigationProjects);
      if(command==="ma_snapshot")return navigationSnapshot(String(args?.projectId));
      if(command==="ma_conversation_snapshot")return navigationConversation(String(args?.projectId));
      if(["ma_probe_runtimes","ma_check_runtime_updates"].includes(command))return [{kind:"codex",installed:true,executable:"fixture",version:"fixture",protocol:"local",capabilities:[],error:null}];
      if(command==="skill_center_snapshot")return {skillSets:[]};
      if(command==="collaboration_health")return {scopeLabel:"当前项目",sessions:[],runtimes:[],messages:[],handoffs:[],cliPath:"fixture",cliPathExists:true,replyEvidence:"fixture"};
      if(command==="general_settings_get")return GENERAL_SETTINGS_PREVIEW;
      if(command==="ma_list_interventions")return [];
    }
    if (bulkFixture && ["stop_session", "delete_session"].includes(command)) {
      const sessionId = String(args?.sessionId); api.bulkCalls.push({ command, sessionId });
      if (command === "delete_session") {
        if (api.rejectBulkDelete === sessionId) throw Error("fixture deletion rejected");
        sessions = sessions.filter(session => session.id !== sessionId);
      } else {
        if (api.rejectBulkStop === sessionId) throw Error("fixture stop rejected");
        sessions = sessions.map(session => session.id === sessionId ? { ...session, status: "stopped", ptyId: null } : session);
      }
      return null;
    }
    if (new URLSearchParams(location.search).has("app-help")) {
      if(command === "plugin:app|version") { if(api.help.versionFails)throw Error("version unavailable");return api.help.version; }
      if(command === "app_check_update") { if(args && Object.keys(args).length)throw Error("App check must not send session/config data");api.help.checks+=1;return new Promise((resolve,reject)=>api.help.pending.push({resolve,reject})); }
      if(command === "plugin:opener|open_url") {api.help.opens.push(String(args?.url));if(api.help.openFails)throw Error("browser unavailable");return;}
    }
    if (command === "model_assignments_snapshot") return { rows: [], projects: [] };
    if (command === "list_providers") return providers;
    if (command === "delete_provider") {
      if (api.rejectProviderDelete) throw Error("fixture provider delete rejected");
      providers = []; return null;
    }
    if (command === "resume_session") { calls.push(String(args?.sessionId)); return new Promise<SessionRecord>((resolve, reject) => pending.push({ id: String(args?.sessionId), resolve, reject })); }
    if (["list_sessions", "session_snapshot"].includes(command)) return sessions;
    if (command === "pty_poll_all") return { sessions, chunks: [] };
    if (command === "get_runtime_preferences") return { disabledRuntimes: [] };
    if (command === "session_usages") return shuffledKeys(structuredClone(api.usages));
    if (command === "session_switch_list") return structuredClone(api.switchRecords);
    if (command === "list_archived_sessions" && location.search.includes("handoff-recovery")) return [(api.switchRecords[0] as any).source];
    if (command === "session_switch_dismiss_recovery") {
      api.switchCommands.push(command);
      if (api.rejectSwitchDismiss) throw Error("fixture dismiss failed");
      const index = api.switchRecords.findIndex(item => (item as any).id === args?.id);
      if (index < 0) throw Error("missing fixture handoff");
      const updated = { ...(api.switchRecords[index] as any), recoveryDismissed: true };
      api.switchRecords[index] = updated;
      localStorage.setItem("fixture-handoff-record", JSON.stringify(updated));
      return structuredClone(updated);
    }
    if (command === "plugin:event|listen") {
      if (new URLSearchParams(location.search).has("event-error") && ["multi-agent-workspace-open", "browser-control-panel-needed"].includes(String(args?.event))) {
        throw Error(`fixture listener rejected: ${args?.event}`);
      }
      listeners.set(String(args?.event), Number(args?.handler)); return 1;
    }
    if (command.startsWith("list_") || command === "session_switch_list") return [];
    return null;
  },
};
if (location.search.includes("handoff-recovery")) {
  const saved = localStorage.getItem("fixture-handoff-record");
  api.switchRecords = [saved ? JSON.parse(saved) : {
    id: "stale-handoff", source: { ...sessions[0], kind: "claude", shell: null, agentRuntimeKind: "claude_code", archivedAt: 123, title: "已归档的接力来源" },
    sourcePosition: 0, sourcePty: null, targetId: "deleted-target", threadId: null,
    targetRuntime: "codex", state: "queued", historyMode: "handoff", context: "fixture history retained", error: null,
  }];
  sessions = sessions.filter(item => item.id !== "one");
}
Object.assign(window, { appSessionMock: api });
if(new URLSearchParams(location.search).has("app-help"))localStorage.setItem("crewvela-appearance",JSON.stringify({theme:new URLSearchParams(location.search).get("theme")==="light"?"light":"dark",accent:new URLSearchParams(location.search).get("accent")??"clay",followSystem:false}));
localStorage.setItem("crewvela-notification-settings-v1", JSON.stringify({ enabled: false }));
localStorage.setItem("crewvela-workspace-tabs", JSON.stringify({ tabs: [{ id: "a", title: new URLSearchParams(location.search).get("workspace-name") ?? "工作区 A" }, { id: "b", title: "工作区 B" }], activeTabId: "a", assignments: { one: "a", two: "a", ...(bulkFixture ? { three: "a", four: "a", other: "b" } : {}) } }));
createRoot(document.getElementById("root")!).render(<App />);

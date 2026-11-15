import { useState } from "react";
import { createRoot } from "react-dom/client";
import { GeneralSettingsPanel } from "../../src/components/GeneralSettingsPanel";
import { BrowserControlSettingsPanel } from "../../src/components/BrowserControlSettingsPanel";
import { ComputerControlSettingsPanel } from "../../src/components/ComputerControlSettingsPanel";
import { SkillCenterPanel } from "../../src/components/SkillCenterPanel";
import { GENERAL_SETTINGS_PREVIEW } from "../../src/generalSettings";
import { BROWSER_CONTROL_PREVIEW } from "../../src/browserControlSettings";
import { COMPUTER_CONTROL_PREVIEW } from "../../src/computerControlSettings";
import { KIMI_COMPUTER_USE_PREVIEW } from "../../src/kimiComputerUseSettings";
import "../../src/styles.css";

// Only this fixture replaces IPC. Production components keep their real handlers.
const statuses: Record<string, any> = {
  general_settings: structuredClone(GENERAL_SETTINGS_PREVIEW),
  browser_control: structuredClone(BROWSER_CONTROL_PREVIEW),
  computer_control: structuredClone(COMPUTER_CONTROL_PREVIEW),
  kimi_computer_use: structuredClone(KIMI_COMPUTER_USE_PREVIEW),
};
if (/mac/i.test(navigator.platform)) {
  const scenario = new URLSearchParams(location.search).get("scenario");
  for (const prefix of ["kimi_computer_use"]) {
    statuses[prefix].settings = { enabled: false };
    statuses[prefix].macos = { supported: scenario !== "unsupported", installed: scenario !== "missing" && scenario !== "unsupported", executable: "/Applications/Fixture.app/Contents/MacOS/tool", mcpRegistered: false, accessibility: null, screenRecording: null, serviceRunning: null, detail: "运行时已检测到；权限尚未验证。" };
    statuses[prefix].warning = null;
  }
}
const runtimes = ["Claude Code", "Codex", "Kimi Code", "Pi", "OpenCode"].map((label, i) => ({ runtime: `runtime-${i}`, label, globalRoot: "C:/fixture/skills", projectRoot: ".agents/skills", discoveryRoots: [], recursiveScan: false, installed: i !== 4 }));
const assets = ["frontend-design", "long-skill-name-for-overflow-verification-2026"].map((name, i) => ({ id: `asset-${i}`, name, description: "回归测试资产，不会读写真实目录", sourceType: "local", sourceRef: "C:/fixture/very-long-project-directory/skills/design", sourceRevision: null, centralPath: "C:/fixture/assets", contentHash: "a".repeat(64), trustState: "trusted", fileCount: 2, scriptCount: 0, riskSummary: [], updatedAt: 0 }));
const snapshot: any = { assets, runtimes, deployments: [], skillSets: [] };
let failNext = false;
let report: (text: string) => void = () => {};
const calls: string[] = [];
let nextCallback = 1;
const callbacks = new Map<number, (value: any) => void>();
const listeners = new Map<number, any>();
const emit = (event: string, payload: any) => { for (const item of listeners.values()) if (item.event === event) callbacks.get(item.handler)?.({ payload }); };
(window as any).__computerMock = { calls, emit, listeners };
(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event: string, id: number) => listeners.delete(id) };
(window as any).__TAURI_INTERNALS__ = { transformCallback: (callback: (value: any) => void) => { const id = nextCallback++; callbacks.set(id, callback); return id; }, async invoke(command: string, args: any) {
  calls.push(`${command} ${JSON.stringify(args ?? {})}`);
  report(calls.join("\n"));
  if (command === "plugin:event|listen") { const id = nextCallback++; listeners.set(id, args); return id; }
  if (command === "plugin:event|unlisten") { listeners.delete(args.eventId); return; }
  if (command === "macos_computer_use_install") {
    if (args.tool !== "kimiCu") throw Error("Unexpected computer-control tool");
    const failed = failNext; failNext = false;
    emit("macos-computer-use-install-progress", { ...args, message: "正在校验官方安装包…" });
    if (new URLSearchParams(location.search).get("installHold") === "1") await new Promise<void>(resolve => { (window as any).__computerMock.release = resolve; });
    else await new Promise(resolve => setTimeout(resolve, 300));
    if (failed) throw Error("测试：下载校验失败，已有安装未修改");
    const status = statuses.kimi_computer_use;
    status.macos = { ...status.macos, installed: true, accessibility: false, screenRecording: false, serviceRunning: true, mcpRegistered: true, detail: "安装并启用完成，请在官方窗口完成系统授权，再重开 Agent 会话。" };
    status.settings = { enabled: true }; status.installed = true; status.mcpRegistered = true;
    if (new URLSearchParams(location.search).get("installOutcome") === "partial") {
      status.settings.enabled = false; status.macos.mcpRegistered = false;
      throw Error("运行时已安装，但 MCP 配置冲突，请启用并保存");
    }
    return structuredClone(status.macos);
  }
  if (failNext && (command.endsWith("_save") || command === "skill_center_deploy_batch" || command === "macos_computer_use_action")) { failNext = false; throw Error("测试保存失败"); }
  if (command === "macos_computer_use_action") {
    if (args.tool !== "kimiCu") throw Error("Unexpected computer-control tool");
    const status = statuses.kimi_computer_use;
    status.macos = { ...status.macos, accessibility: args.prepare, screenRecording: args.prepare, serviceRunning: args.prepare, detail: args.prepare ? "Mock 授权已完成；请重开 Agent 会话验证。" : "待授权：辅助功能和屏幕录制尚未授予。" };
    return structuredClone(status.macos);
  }
  const prefix = command.replace(/_(get|save)$/, "");
  if (statuses[prefix]) {
    if (command.endsWith("_save")) {
      statuses[prefix].settings = args.settings;
      if (statuses[prefix].macos) statuses[prefix].macos.mcpRegistered = args.settings.enabled;
    }
    return structuredClone(statuses[prefix]);
  }
  if (command === "skill_center_snapshot") return structuredClone(snapshot);
  if (command === "skill_center_deploy_batch") {
    snapshot.deployments.push(...args.inputs.map((input: any, i: number) => ({ ...input, id: `deploy-${i}`, status: "synced", mode: "copy", targetPath: "C:/fixture/skills", sourceHash: "a".repeat(64), targetHash: "a".repeat(64) })));
  }
  if (command === "skill_center_save_set") snapshot.skillSets.push({ ...args.input, id: "set-1", description: "", updatedAt: 0 });
  if (command === "skill_center_preview") return { skillId: args.skillId, skillMd: "# Fixture Skill\n\n仅用于检查正式文件预览布局。", files: [{ path: "SKILL.md", size: 128, isScript: false }, { path: "references/very-long-reference-file-name.md", size: 256, isScript: false }] };
  if (command === "skill_center_read_file") return `# ${args.relativePath}\n\nFixture file content`;
  if (command === "plugin:dialog|open") return "C:/fixture/task-worktree";
  if (command === "skill_center_market_browse") return { skills: [], errors: [] };
  return [];
} };
const pages = { general: "常规", browser: "浏览器控制", computer: "电脑控制", skills: "Skills" };
function Fixture() {
  const [mode, setMode] = useState<keyof typeof pages>((new URLSearchParams(location.search).get("page") as keyof typeof pages) || "general");
  const [log, setLog] = useState("");
  const [light, setLight] = useState(false);
  report = setLog;
  return <><div style={{ position: "fixed", inset: "0 0 auto", zIndex: 10000, padding: 8, background: "var(--bg)", display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
    <strong>隔离 Mock 回归</strong>
    <button onClick={() => { const next = !light; setLight(next); document.documentElement.dataset.theme = next ? "light" : "dark"; }}>切换明暗主题</button>
    <button onClick={() => { failNext = true; }}>下次保存失败</button>
    <details><summary>调用记录</summary><pre style={{ maxHeight: 180, maxWidth: "80vw", overflow: "auto", whiteSpace: "pre-wrap" }}>{log}</pre></details>
  </div><div className="settings-overlay"><div className="settings-dialog settings-workspace-dialog" style={{ position: "fixed", inset: "48px 0 0", width: "100%", height: "calc(100% - 48px)", minWidth: 0 }}>
    <aside className="settings-nav"><div className="settings-brand">设置</div><nav className="settings-nav-list">{Object.entries(pages).map(([id, label]) => <button className={`settings-nav-item${id === mode ? " active" : ""}`} key={id} onClick={() => setMode(id as keyof typeof pages)}>{label}</button>)}</nav></aside>
    <div className="settings-content"><div className="settings-content-inner"><div className="settings-heading"><h2>{pages[mode]}</h2></div>
      {mode === "general" ? <GeneralSettingsPanel /> : mode === "browser" ? <BrowserControlSettingsPanel /> : mode === "computer" ? <ComputerControlSettingsPanel /> : <SkillCenterPanel projects={[{ id: "fixture", name: "回归项目", path: "C:/fixture" } as any]} />}
    </div></div>
  </div></div></>;
}
document.documentElement.dataset.theme = "dark";
createRoot(document.getElementById("root")!).render(<Fixture />);

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
import { OPEN_COMPUTER_USE_PREVIEW } from "../../src/openComputerUseSettings";
import "../../src/styles.css";

// Only this fixture replaces IPC. Production components keep their real handlers.
const statuses: Record<string, any> = {
  general_settings: structuredClone(GENERAL_SETTINGS_PREVIEW),
  browser_control: structuredClone(BROWSER_CONTROL_PREVIEW),
  computer_control: structuredClone(COMPUTER_CONTROL_PREVIEW),
  kimi_computer_use: structuredClone(KIMI_COMPUTER_USE_PREVIEW),
  open_computer_use: structuredClone(OPEN_COMPUTER_USE_PREVIEW),
};
const runtimes = ["Claude Code", "Codex", "Kimi Code", "Pi", "OpenCode"].map((label, i) => ({ runtime: `runtime-${i}`, label, globalRoot: "C:/fixture/skills", projectRoot: ".agents/skills", discoveryRoots: [], recursiveScan: false, installed: i !== 4 }));
const assets = ["frontend-design", "long-skill-name-for-overflow-verification-2026"].map((name, i) => ({ id: `asset-${i}`, name, description: "回归测试资产，不会读写真实目录", sourceType: "local", sourceRef: "C:/fixture/very-long-project-directory/skills/design", sourceRevision: null, centralPath: "C:/fixture/assets", contentHash: "a".repeat(64), trustState: "trusted", fileCount: 2, scriptCount: 0, riskSummary: [], updatedAt: 0 }));
const snapshot: any = { assets, runtimes, deployments: [], skillSets: [] };
let failNext = false;
let report: (text: string) => void = () => {};
const calls: string[] = [];
(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any) {
  calls.push(`${command} ${JSON.stringify(args ?? {})}`);
  report(calls.join("\n"));
  if (failNext && (command.endsWith("_save") || command === "skill_center_deploy_batch")) { failNext = false; throw Error("测试保存失败"); }
  const prefix = command.replace(/_(get|save)$/, "");
  if (statuses[prefix]) {
    if (command.endsWith("_save")) statuses[prefix].settings = args.settings;
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
const pages = { general: "常规", browser: "浏览器控制", computer: "电脑控制", skills: "Skill 中心" };
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

import { useState } from "react";
import { createRoot } from "react-dom/client";
import { Terminal } from "@xterm/xterm";
import { TerminalPane } from "../../src/components/TerminalPane";
import { MultiTerminal } from "../../src/components/MultiTerminal";
import { SettingsDialog } from "../../src/components/SettingsDialog";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import { DEFAULT_GENERAL_SETTINGS, type GeneralSettings } from "../../src/generalSettings";
import type { SessionRecord } from "../../src/types";
import type { SessionSwitchRecord } from "../../src/sessionSwitch";
import "../../src/styles.css";

const callbacks = new Map<number, (event: any) => void>();
let callbackId = 0;
const listeners = new Map<string, number>();
const fontSettings = new URLSearchParams(location.search).has("fonts");
const initialSettings = { ...DEFAULT_GENERAL_SETTINGS, terminalFontFamily: "Consolas, monospace" };
const installedFonts = [
  { family: "Arial", aliases: [], monospace: false },
  { family: "Courier New", aliases: [], monospace: true },
  { family: "Microsoft YaHei", aliases: ["微软雅黑"], monospace: false },
  { family: "Consolas", aliases: [], monospace: true },
];
const fixture = { terminal: null as Terminal | null, pending: null as null | ((value: any) => void), rejectInput: true,
  preview: null as Terminal | null, fontReads: 0, failFontRead: false, saves: [] as GeneralSettings[],
  emitAppearance(settings = { ...DEFAULT_GENERAL_SETTINGS, terminalFontSize: 24 }) { callbacks.get(listeners.get("general-settings-changed")!)?.({ payload: settings }); },
  release() { fixture.pending?.({ settings: { ...DEFAULT_GENERAL_SETTINGS, terminalFontSize: 10 } }); },
};
const originalOpen = Terminal.prototype.open;
Terminal.prototype.open = function(element) {
  if (element.closest(".terminal-font-preview")) fixture.preview = this;
  else fixture.terminal = this;
  return originalOpen.call(this, element);
};
Object.assign(window, { terminalMock: fixture, __TAURI_INTERNALS__: {
  transformCallback(fn: (event: any) => void) { callbacks.set(++callbackId, fn); return callbackId; },
  invoke(command: string, args: any) {
    if (fontSettings && command === "general_settings_get") return Promise.resolve({ settings: initialSettings, proxyEnvActive: false, keepAwakeActive: null, warning: null });
    if (command === "terminal_fonts_list") {
      fixture.fontReads++;
      if (fixture.failFontRead) return Promise.reject(Error("fixture read failed"));
      return Promise.resolve(installedFonts);
    }
    if (command === "general_settings_save") {
      fixture.saves.push(args.settings);
      fixture.emitAppearance(args.settings);
      return Promise.resolve({ settings: args.settings, proxyEnvActive: false, keepAwakeActive: null, warning: null });
    }
    if (command === "general_settings_get") return new Promise(resolve => { fixture.pending = resolve; });
    if (command === "plugin:event|listen") { listeners.set(args.event, args.handler); return Promise.resolve(1); }
    if (command === "pty_input" && fixture.rejectInput) return Promise.reject(Error("fixture input refused"));
    return Promise.resolve(null);
  },
}, __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} } });
const sessions = ["relayed", "ordinary"].map((id, index) => ({ id, title: id, kind: "terminal", shell: "powershell", providerName: "PowerShell", cwd: "/fixture", status: "running", ptyId: index + 1 })) as SessionRecord[];
const records = [{ source: { ...sessions[0], id: "original" }, targetId: "relayed", logicalId: "original", state: "queued" }] as SessionSwitchRecord[];
function Fixture() {
  const [visible, setVisible] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(fontSettings);
  if (location.search.includes("drag")) return <MultiTerminal sessions={sessions} switchRecords={records} selectedSessionId={null} theme="dark" preview onClose={() => {}} onToggleMaximize={() => {}} onSessionsChange={() => {}} />;
  const pane = visible && <TerminalPane sessionId={1} title="初始化测试" providerName="fixture" kind="terminal" shell="powershell" cwd="/fixture" status="running" theme="dark" hidden={false} onClose={() => setVisible(false)} />;
  if (fontSettings) return <>{pane}{settingsOpen && <SettingsDialog initialSection="generalSettings" sessions={[]} archivedSessions={[]} projects={[]}
    appearance={DEFAULT_APPEARANCE} resolvedAppearanceTheme="dark" petSettings={DEFAULT_PET_SETTINGS} notificationSettings={DEFAULT_NOTIFICATION_SETTINGS}
    onAppearanceChange={() => {}} onPetSettingsChange={() => {}} onNotificationSettingsChange={() => {}}
    onRestoreArchivedSession={() => {}} onDeleteArchivedSession={() => {}} onViewArchivedTranscript={() => {}}
    onClose={() => setSettingsOpen(false)} />}</>;
  return <><button onClick={() => setVisible(false)}>关闭终端</button>{location.search.includes("sidebar")
    ? <div className="unified-right-sidebar-content"><div className="right-sidebar-panel right-sidebar-terminal active">{pane}</div></div>
    : pane}</>;
}
document.body.style.cssText = "margin:0;background:#202020;color:#eee;font-family:system-ui";
document.getElementById("root")!.style.cssText = "height:90vh;display:flex;flex-direction:column";
createRoot(document.getElementById("root")!).render(<Fixture />);

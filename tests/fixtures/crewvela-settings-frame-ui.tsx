import { useState } from "react";
import { createRoot } from "react-dom/client";
import { SettingsDialog } from "../../src/components/SettingsDialog";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import { DEFAULT_COMPANION_SETTINGS } from "../../src/companionSettings";
import { GENERAL_SETTINGS_PREVIEW } from "../../src/generalSettings";
import { installSkillRuntimeFixture } from "./skill-runtime-first-mock";
import type { AppAppearance, ProviderConfig, SessionRecord } from "../../src/types";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
const theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.theme = theme;
if (params.get("mock") === "skills") installSkillRuntimeFixture(params);
// Explicit fixture-only IPC. Never contacts a real runtime or user profile.
if (params.get("mock") === "general") {
  let general = structuredClone(GENERAL_SETTINGS_PREVIEW);
  let failNext = params.get("fail") === "1";
  (window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any) {
    if (command === "general_settings_get") return structuredClone(general);
    if (command === "general_settings_save") {
      if (failNext) { failNext = false; throw Error("回归测试：写入失败"); }
      general = { ...general, settings: args.settings };
      return structuredClone(general);
    }
    if (command === "plugin:event|listen") return 1;
    if (command === "plugin:event|unlisten") return;
    throw Error(`此隔离页面未模拟 ${command}`);
  } };
}
const archiveCount = params.get("archive") === "long" ? 100 : 0;
const sampleProviders: ProviderConfig[] = params.get("providers") === "sample" ? [
  { id: "fixture-api", name: "示例模型服务", baseUrl: "https://example.invalid/v1", authTokenRef: "", env: { ANTHROPIC_MODEL: "example-model" }, timeoutMs: null, disableNonessentialTraffic: false, builtin: true },
  { id: "fixture-local", name: "本地模型", baseUrl: "http://127.0.0.1:11434", authTokenRef: "", env: {}, timeoutMs: null, disableNonessentialTraffic: false, builtin: false },
] : [];
const archive = Array.from({ length: archiveCount }, (_, index): SessionRecord => ({
  id: `archive-${index}`, claudeSessionId: `archive-${index}`, kind: "claude", shell: null,
  providerId: "fixture", providerName: "Fixture Provider", cwd: `C:/fixture/project-${index}`,
  title: `归档会话 ${index + 1} · ${"长标题".repeat(12)}`, status: "stopped", ptyId: null,
  yolo: false, autoMode: false, titleLocked: false, exitCode: 0, lastError: null,
  createdAt: 1, updatedAt: 1, archivedAt: 1,
}));

function Fixture() {
  const [open, setOpen] = useState(params.get("open") === "1");
  const [appearance, setAppearance] = useState<AppAppearance>({ ...DEFAULT_APPEARANCE, theme });
  return <>
    <button type="button" onClick={() => setOpen(true)}>打开设置回归</button>
    {params.get("debug") === "1" && <pre id="fixture-skills-log" />}
    {open && <SettingsDialog sessions={[]} archivedSessions={archive} projects={params.get("mock") === "skills" ? [{ id: "fixture-project", name: "示例项目", path: "/fixture/project", badgeColor: "blue", createdAt: 0, updatedAt: 0 }] : []} providers={sampleProviders}
      keyStates={{}} usageByProvider={{}} appearance={appearance} resolvedAppearanceTheme={theme}
      petSettings={DEFAULT_PET_SETTINGS} notificationSettings={DEFAULT_NOTIFICATION_SETTINGS}
      companionSettings={DEFAULT_COMPANION_SETTINGS} onAppearanceChange={setAppearance}
      onPetSettingsChange={() => {}} onNotificationSettingsChange={() => {}} onCompanionSettingsChange={() => {}}
      onEdit={() => {}} onNew={() => {}} onDelete={() => {}} onSetHidden={() => {}}
      onRestoreDefault={() => {}} onReorder={async () => {}} onRestoreArchivedSession={() => {}}
      onDeleteArchivedSession={() => {}} onViewArchivedTranscript={() => {}} onClose={() => setOpen(false)}
      initialSection={params.get("section") === "collaboration" ? "collaboration" : params.get("section") === "browserControl" ? "browserControl" : params.get("section") === "skills" ? "skills" : "generalSettings"} />}
  </>;
}

createRoot(document.getElementById("root")!).render(<Fixture />);

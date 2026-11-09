import { useState } from "react";
import { createRoot } from "react-dom/client";
import { SettingsDialog } from "../../src/components/SettingsDialog";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import { DEFAULT_COMPANION_SETTINGS } from "../../src/companionSettings";
import type { AppAppearance, SessionRecord } from "../../src/types";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
const theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.theme = theme;
const archiveCount = params.get("archive") === "long" ? 100 : 0;
const archive = Array.from({ length: archiveCount }, (_, index): SessionRecord => ({
  id: `archive-${index}`, claudeSessionId: `archive-${index}`, kind: "claude", shell: null,
  providerId: "fixture", providerName: "Fixture Provider", cwd: `C:/fixture/project-${index}`,
  title: `归档会话 ${index + 1} · ${"长标题".repeat(12)}`, status: "stopped", ptyId: null,
  yolo: false, autoMode: false, titleLocked: false, exitCode: 0, lastError: null,
  createdAt: 1, updatedAt: 1, archivedAt: 1,
}));

function Fixture() {
  const [open, setOpen] = useState(false);
  const [appearance, setAppearance] = useState<AppAppearance>({ ...DEFAULT_APPEARANCE, theme });
  return <>
    <button type="button" onClick={() => setOpen(true)}>打开设置回归</button>
    {open && <SettingsDialog sessions={[]} archivedSessions={archive} projects={[]} providers={[]}
      keyStates={{}} usageByProvider={{}} appearance={appearance} resolvedAppearanceTheme={theme}
      petSettings={DEFAULT_PET_SETTINGS} notificationSettings={DEFAULT_NOTIFICATION_SETTINGS}
      companionSettings={DEFAULT_COMPANION_SETTINGS} onAppearanceChange={setAppearance}
      onPetSettingsChange={() => {}} onNotificationSettingsChange={() => {}} onCompanionSettingsChange={() => {}}
      onEdit={() => {}} onNew={() => {}} onDelete={() => {}} onSetHidden={() => {}}
      onRestoreDefault={() => {}} onReorder={async () => {}} onRestoreArchivedSession={() => {}}
      onDeleteArchivedSession={() => {}} onViewArchivedTranscript={() => {}} onClose={() => setOpen(false)}
      initialSection="generalSettings" />}
  </>;
}

createRoot(document.getElementById("root")!).render(<Fixture />);

// Isolated callbacks and quota IPC: never reads or changes the user's settings.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { SettingsDialog } from "../../src/components/SettingsDialog";
import { DEFAULT_APPEARANCE } from "../../src/appearance";
import { DEFAULT_PET_SETTINGS } from "../../src/petSettings";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../../src/notificationSettings";
import type { ProviderConfig } from "../../src/types";
import type { ProviderQuota } from "../../src/providerQuota";
import "../../src/styles.css";

const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
const calls: string[] = [];
const record = (value: string) => { calls.push(value); const log = document.getElementById("fixture-events"); if (log) log.textContent = calls.join("\n"); };
(window as any).__modelCalls = calls;
(window as any).__TAURI_INTERNALS__ = { transformCallback: () => 1, async invoke(command: string, args: any) {
  if (command !== "get_provider_quota") throw Error(`未模拟 ${command}`);
  record(`quota:${args.id}:${args.force}`);
  if (args.force && params.get("quota") === "throw") throw Error("刷新失败");
  const result: ProviderQuota = { status: "ok", updatedAt: Date.now(), message: null, balances: [], windows: [] };
  if (args.id === "glm") result.windows = [
    { label: "5 小时", usedPercent: params.get("quota") === "high" ? 95 : 5, remainingPercent: null, resetAt: Date.now() + 12_240_000 },
    { label: "7 天", usedPercent: 44, remainingPercent: null, resetAt: Date.now() + 223_200_000 },
  ];
  else if (args.id === "deepseek") result.balances = [{ amount: 5.89, currency: "CNY" }];
  else return { ...result, status: "unsupported", updatedAt: null, message: "此供应商暂不支持账户额度查询" };
  return result;
} };
const provider = (id: string, name: string, baseUrl: string, model: string, builtin = true): ProviderConfig => ({ id, name, baseUrl, authTokenRef: id, env: model ? { ANTHROPIC_MODEL: model } : {}, timeoutMs: null, disableNonessentialTraffic: false, builtin });
const initial = [
  provider("glm", "GLM", "https://open.bigmodel.cn/api/anthropic", "glm-5.3-flash"),
  provider("deepseek", "DeepSeek", "https://api.deepseek.com/anthropic", "deepseek-flash"),
  provider("kimi", "Kimi", "https://api.kimi.com/coding/", "k3"),
  provider("minimax", "MiniMax", "https://api.minimaxi.com/anthropic", "MiniMax-M3"),
  provider("agnes", "agnes-ai", "https://apihub.agnes-ai.com/v1", "agnes-3.0-flash", false),
  { ...provider("custom", "自定义", "", ""), hidden: true },
];
function Fixture() {
  const [providers, setProviders] = useState(initial);
  const [open, setOpen] = useState(true);
  const [appearance, setAppearance] = useState(DEFAULT_APPEARANCE);
  return <><output id="fixture-events" hidden />{open ? <SettingsDialog initialSection="providers" initialModelView="connections" sessions={[]} archivedSessions={[]} projects={[]} providers={providers}
    keyStates={Object.fromEntries(providers.map(p => [p.id, true]))} usageByProvider={{}}
    appearance={appearance} resolvedAppearanceTheme={document.documentElement.dataset.theme === "light" ? "light" : "dark"}
    petSettings={DEFAULT_PET_SETTINGS} notificationSettings={DEFAULT_NOTIFICATION_SETTINGS}
    onAppearanceChange={setAppearance} onPetSettingsChange={() => {}} onNotificationSettingsChange={() => {}}
    onEdit={p => record(`edit:${p.id}`)} onNew={() => record("new")}
    onDelete={id => { record(`delete:${id}`); setProviders(ps => ps.filter(p => p.id !== id)); }}
    onSetHidden={(id, hidden) => { record(`hidden:${id}:${hidden}`); setProviders(ps => ps.map(p => p.id === id ? { ...p, hidden } : p)); }}
    onRestoreDefault={id => { record(`restore:${id}`); setProviders(ps => ps.map(p => p.id === id ? { ...p, hidden: false } : p)); }}
    onReorder={async ids => { record(`reorder:${ids.join(",")}`); if (params.get("order") === "fail") throw Error("模拟写入失败"); setProviders(ps => ids.map(id => ps.find(p => p.id === id)!)); }}
    onRestoreArchivedSession={() => {}} onDeleteArchivedSession={() => {}} onViewArchivedTranscript={() => {}} onClose={() => setOpen(false)} /> : <p>已关闭</p>}</>;
}
createRoot(document.getElementById("root")!).render(<Fixture />);

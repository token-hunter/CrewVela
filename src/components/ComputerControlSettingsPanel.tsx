import { IS_MAC } from "../platform";
import "../styles/settings-workspace.css";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Monitor, Cube, Stack, ShieldCheck } from "@phosphor-icons/react";
import { SettingsGroup, SettingSwitch, SettingsActions } from "./ui/RuntimeSettings";
import {
  COMPUTER_CONTROL_COVERAGE,
  COMPUTER_CONTROL_PREVIEW,
  COMPUTER_CONTROL_SAFETY_NOTE,
  DEFAULT_COMPUTER_CONTROL_SETTINGS,
  type ComputerControlSettings,
  type ComputerControlStatus,
} from "../computerControlSettings";
import {
  DEFAULT_KIMI_COMPUTER_USE_SETTINGS,
  KIMI_COMPUTER_USE_COVERAGE,
  KIMI_COMPUTER_USE_PREVIEW,
  KIMI_COMPUTER_USE_SAFETY_NOTE,
  type KimiComputerUseSettings,
  type KimiComputerUseStatus,
} from "../kimiComputerUseSettings";
import {
  DEFAULT_OPEN_COMPUTER_USE_SETTINGS,
  OPEN_COMPUTER_USE_COVERAGE,
  OPEN_COMPUTER_USE_PREVIEW,
  OPEN_COMPUTER_USE_SAFETY_NOTE,
  type OpenComputerUseSettings,
  type OpenComputerUseStatus,
} from "../openComputerUseSettings";

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";

export function ComputerControlSettingsPanel() {
  const [status, setStatus] = useState<ComputerControlStatus | null>(
    UI_PREVIEW ? COMPUTER_CONTROL_PREVIEW : null,
  );
  const [draft, setDraft] = useState<ComputerControlSettings>(
    UI_PREVIEW ? COMPUTER_CONTROL_PREVIEW.settings : DEFAULT_COMPUTER_CONTROL_SETTINGS,
  );
  const [kimiStatus, setKimiStatus] = useState<KimiComputerUseStatus | null>(
    UI_PREVIEW ? KIMI_COMPUTER_USE_PREVIEW : null,
  );
  const [kimiDraft, setKimiDraft] = useState<KimiComputerUseSettings>(
    UI_PREVIEW ? KIMI_COMPUTER_USE_PREVIEW.settings : DEFAULT_KIMI_COMPUTER_USE_SETTINGS,
  );
  const [openStatus, setOpenStatus] = useState<OpenComputerUseStatus | null>(
    UI_PREVIEW ? OPEN_COMPUTER_USE_PREVIEW : null,
  );
  const [openDraft, setOpenDraft] = useState<OpenComputerUseSettings>(
    UI_PREVIEW ? OPEN_COMPUTER_USE_PREVIEW.settings : DEFAULT_OPEN_COMPUTER_USE_SETTINGS,
  );
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [loading, setLoading] = useState(!UI_PREVIEW);
  const [reload, setReload] = useState(0);
  const saveInFlight = useRef(false);

  useEffect(() => {
    if (UI_PREVIEW || IS_MAC) return;
    let disposed = false;
    setLoading(true); setNotice(null);
    void Promise.allSettled([
      invoke<ComputerControlStatus>("computer_control_get"),
      invoke<KimiComputerUseStatus>("kimi_computer_use_get"),
      invoke<OpenComputerUseStatus>("open_computer_use_get"),
    ]).then(([local, kimi, open]) => {
      if (disposed) return;
      if (local.status === "fulfilled") { setStatus(local.value); setDraft(local.value.settings); }
      if (kimi.status === "fulfilled") { setKimiStatus(kimi.value); setKimiDraft(kimi.value.settings); }
      if (open.status === "fulfilled") { setOpenStatus(open.value); setOpenDraft(open.value.settings); }
      const failures = [local, kimi, open].flatMap((result, i) => result.status === "rejected" ? [`${["computer-control", "kimi-computer-use", "open-computer-use"][i]}: ${String(result.reason)}`] : []);
      if (failures.length) setNotice(`读取设置失败：${failures.join("；")}`);
      setLoading(false);
    });
    return () => { disposed = true; };
  }, [reload]);

  const ready = !!status && !!kimiStatus && !!openStatus && !loading;

  const dirty =
    JSON.stringify(draft) !== JSON.stringify(status?.settings ?? null) ||
    JSON.stringify(kimiDraft) !== JSON.stringify(kimiStatus?.settings ?? null) ||
    JSON.stringify(openDraft) !== JSON.stringify(openStatus?.settings ?? null);

  const save = async () => {
    if (!ready || saveInFlight.current) return;
    saveInFlight.current = true;
    setNotice(null);
    setBusy(true);
    try {
      const [local, kimi, open] = await Promise.allSettled([
        JSON.stringify(draft) === JSON.stringify(status.settings) ? Promise.resolve(status) : invoke<ComputerControlStatus>("computer_control_save", { settings: draft }),
        JSON.stringify(kimiDraft) === JSON.stringify(kimiStatus.settings) ? Promise.resolve(kimiStatus) : invoke<KimiComputerUseStatus>("kimi_computer_use_save", { settings: kimiDraft }),
        JSON.stringify(openDraft) === JSON.stringify(openStatus.settings) ? Promise.resolve(openStatus) : invoke<OpenComputerUseStatus>("open_computer_use_save", { settings: openDraft }),
      ]);
      if (local.status === "fulfilled") { setStatus(local.value); setDraft(local.value.settings); }
      if (kimi.status === "fulfilled") { setKimiStatus(kimi.value); setKimiDraft(kimi.value.settings); }
      if (open.status === "fulfilled") { setOpenStatus(open.value); setOpenDraft(open.value.settings); }
      const failures = [local, kimi, open].flatMap((result, i) => result.status === "rejected" ? [`${["computer-control", "kimi-computer-use", "open-computer-use"][i]}: ${String(result.reason)}`] : []);
      setNotice(failures.length ? `部分设置未保存：${failures.join("；")}` : "电脑控制设置已应用");
    } catch (reason) {
      setNotice(`保存失败：${String(reason)}`);
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  };

  if (IS_MAC) return <p className="runtime-warning" role="status">macOS 暂不支持电脑控制，此功能需要后续适配。</p>;

  const methods = [
    { title: "内置电脑控制", hint: "通过 MCP 与技能读取窗口、点击和输入。", icon: Monitor, enabled: draft.enabled, change: setDraft, status, coverage: COMPUTER_CONTROL_COVERAGE, safety: COMPUTER_CONTROL_SAFETY_NOTE },
    { title: "KimiCU", hint: "需单独安装，使用时自动启动。", icon: Cube, enabled: kimiDraft.enabled, change: setKimiDraft, status: kimiStatus, coverage: KIMI_COMPUTER_USE_COVERAGE, safety: KIMI_COMPUTER_USE_SAFETY_NOTE },
    { title: "Open Computer Use", hint: "通过本机安装的 open-computer-use 提供 MCP 服务。", icon: Stack, enabled: openDraft.enabled, change: setOpenDraft, status: openStatus, coverage: OPEN_COMPUTER_USE_COVERAGE, safety: OPEN_COMPUTER_USE_SAFETY_NOTE },
  ];

  return (
    <div className="runtime-settings settings-workspace computer-settings">
      <div className="workspace-control-grid">
        <fieldset className="workspace-section workspace-configuration" disabled={busy || !ready}>
          <SettingsGroup title="控制方式" description="选择 Agent 可以使用的电脑控制方式，可同时启用多种。">
            {methods.map(method => <SettingSwitch key={method.title} title={method.title} hint={method.hint}
              icon={<method.icon size={24} aria-hidden="true" />} checked={method.enabled} onChange={enabled => method.change({ enabled })} />)}
          </SettingsGroup>
        </fieldset>
        <aside className="workspace-section workspace-status" aria-label="电脑控制运行状态">
        <SettingsGroup title="本机状态" description="查看已保存的控制配置与本机运行时。">
          <dl className="runtime-facts">
            <div><dt>操作系统</dt><dd>Windows</dd></div>
            <div><dt>KimiCU 安装状态</dt><dd>{kimiStatus?.installed == null ? "尚未确认" : kimiStatus.installed ? "已安装" : "未安装"}</dd></div>
            <div><dt>KimiCU MCP 注册状态</dt><dd>{kimiStatus?.mcpRegistered == null ? "尚未确认" : kimiStatus.mcpRegistered ? "已注册并启用" : "未启用"}</dd></div>
          </dl>
          {methods.map(method => method.status?.warning && <p key={method.title} className="runtime-warning" role="alert">{method.title}：{method.status.warning}</p>)}
          <details className="runtime-details"><summary>运行时详情与安全说明</summary>
            {methods.map(method => <section key={method.title}><h4>{method.title}</h4><p>{method.status?.runtime}</p><p>{method.coverage}</p><p>{method.safety}</p></section>)}
          </details>
        </SettingsGroup>
        </aside>
      </div>
      <SettingsActions ready={ready} loading={loading} busy={busy} dirty={dirty} message={notice}
        hint={<><ShieldCheck size={17} aria-hidden="true" />仅在可信窗口中使用。</>} onSave={save} onRetry={() => setReload(n => n + 1)} />
    </div>
  );
}

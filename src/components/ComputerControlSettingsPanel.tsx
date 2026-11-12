import { IS_MAC } from "../platform";
import { macControlLabel, permissionLabel, type MacControlTool, type MacControlStatus } from "../macosComputerUse";
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
    if (UI_PREVIEW) return;
    let disposed = false;
    setLoading(true); setNotice(null);
    void Promise.allSettled([
      IS_MAC ? Promise.resolve(null) : invoke<ComputerControlStatus>("computer_control_get"),
      invoke<KimiComputerUseStatus>("kimi_computer_use_get"),
      invoke<OpenComputerUseStatus>("open_computer_use_get"),
    ]).then(([local, kimi, open]) => {
      if (disposed) return;
      if (local.status === "fulfilled" && local.value) { setStatus(local.value); setDraft(local.value.settings); }
      if (kimi.status === "fulfilled") { setKimiStatus(kimi.value); setKimiDraft(kimi.value.settings); }
      if (open.status === "fulfilled") { setOpenStatus(open.value); setOpenDraft(open.value.settings); }
      const failures = [local, kimi, open].flatMap((result, i) => result.status === "rejected" ? [`${["computer-control", "kimi-computer-use", "open-computer-use"][i]}: ${String(result.reason)}`] : []);
      if (failures.length) setNotice(`读取设置失败：${failures.join("；")}`);
      setLoading(false);
    });
    return () => { disposed = true; };
  }, [reload]);

  const ready = (IS_MAC || !!status) && !!kimiStatus && !!openStatus && !loading;

  const dirty =
    (!IS_MAC && JSON.stringify(draft) !== JSON.stringify(status?.settings ?? null)) ||
    JSON.stringify(kimiDraft) !== JSON.stringify(kimiStatus?.settings ?? null) ||
    JSON.stringify(openDraft) !== JSON.stringify(openStatus?.settings ?? null);

  const save = async () => {
    if (!ready || saveInFlight.current) return;
    saveInFlight.current = true;
    setNotice(null);
    setBusy(true);
    try {
      const [local, kimi, open] = await Promise.allSettled([
        IS_MAC || JSON.stringify(draft) === JSON.stringify(status?.settings) ? Promise.resolve(status) : invoke<ComputerControlStatus>("computer_control_save", { settings: draft }),
        JSON.stringify(kimiDraft) === JSON.stringify(kimiStatus?.settings) ? Promise.resolve(kimiStatus) : invoke<KimiComputerUseStatus>("kimi_computer_use_save", { settings: kimiDraft }),
        JSON.stringify(openDraft) === JSON.stringify(openStatus?.settings) ? Promise.resolve(openStatus) : invoke<OpenComputerUseStatus>("open_computer_use_save", { settings: openDraft }),
      ]);
      if (local.status === "fulfilled" && local.value) { setStatus(local.value); setDraft(local.value.settings); }
      if (kimi.status === "fulfilled" && kimi.value) { setKimiStatus(kimi.value); setKimiDraft(kimi.value.settings); }
      if (open.status === "fulfilled" && open.value) { setOpenStatus(open.value); setOpenDraft(open.value.settings); }
      const failures = [local, kimi, open].flatMap((result, i) => result.status === "rejected" ? [`${["computer-control", "kimi-computer-use", "open-computer-use"][i]}: ${String(result.reason)}`] : []);
      setNotice(failures.length ? `部分设置未保存：${failures.join("；")}` : "电脑控制设置已应用");
    } catch (reason) {
      setNotice(`保存失败：${String(reason)}`);
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  };

  const macAction = async (tool: MacControlTool, prepare: boolean) => {
    if (saveInFlight.current) return;
    saveInFlight.current = true; setBusy(true); setNotice(null);
    try {
      const macos = await invoke<MacControlStatus>("macos_computer_use_action", { tool, prepare });
      if (tool === "openComputerUse") setOpenStatus(previous => previous && ({ ...previous, macos, warning: macos.detail }));
      else setKimiStatus(previous => previous && ({ ...previous, macos, installed: macos.installed, mcpRegistered: macos.mcpRegistered, warning: macos.detail }));
      setNotice(macos.detail);
    } catch (error) { setNotice(`检查或准备失败：${String(error)}`); }
    finally { saveInFlight.current = false; setBusy(false); }
  };

  const allMethods = [
    { title: "内置电脑控制", hint: "通过 MCP 与技能读取窗口、点击和输入。", icon: Monitor, enabled: draft.enabled, change: setDraft, status, coverage: COMPUTER_CONTROL_COVERAGE, safety: COMPUTER_CONTROL_SAFETY_NOTE },
    { title: "KimiCU", hint: IS_MAC ? "安装官方 Mac 应用，打开授权与后台服务，再保存启用。" : "需单独安装，使用时自动启动。", icon: Cube, enabled: kimiDraft.enabled, change: setKimiDraft, status: kimiStatus, coverage: KIMI_COMPUTER_USE_COVERAGE, safety: KIMI_COMPUTER_USE_SAFETY_NOTE },
    { title: "Open Computer Use", hint: "通过本机安装的 open-computer-use 提供 MCP 服务。", icon: Stack, enabled: openDraft.enabled, change: setOpenDraft, status: openStatus, coverage: OPEN_COMPUTER_USE_COVERAGE, safety: OPEN_COMPUTER_USE_SAFETY_NOTE },
  ];
  const methods = IS_MAC ? [allMethods[2], allMethods[1]] : allMethods;
  const external = [{ title: "Open Computer Use", tool: "openComputerUse" as const, macos: openStatus?.macos }, { title: "KimiCU", tool: "kimiCu" as const, macos: kimiStatus?.macos }];

  return (
    <div className="runtime-settings settings-workspace computer-settings">
      <div className="workspace-control-grid">
        <fieldset className="workspace-section workspace-configuration" disabled={busy || !ready}>
          <SettingsGroup title="控制方式" description="选择 Agent 可以使用的电脑控制方式，可同时启用多种。">
            {methods.map(method => <SettingSwitch key={method.title} title={method.title} hint={method.hint}
              disabled={IS_MAC && !method.enabled && !("macos" in (method.status ?? {}) && (method.status as OpenComputerUseStatus | KimiComputerUseStatus)?.macos?.supported && (method.status as OpenComputerUseStatus | KimiComputerUseStatus)?.macos?.installed)}
              icon={<method.icon size={24} aria-hidden="true" />} checked={method.enabled} onChange={enabled => method.change({ enabled })} />)}
            {IS_MAC && <p>macOS 仅接入以上两种外部方式。内置电脑控制暂未适配。启用后需重开 Agent 会话加载工具。</p>}
          </SettingsGroup>
        </fieldset>
        <aside className="workspace-section workspace-status" aria-label="电脑控制运行状态">
        <SettingsGroup title="本机状态" description="查看已保存的控制配置与本机运行时。">
          <dl className="runtime-facts">
            <div><dt>操作系统</dt><dd>{IS_MAC ? "macOS" : "Windows"}</dd></div>
            {!IS_MAC && <><div><dt>KimiCU 安装状态</dt><dd>{kimiStatus?.installed == null ? "尚未确认" : kimiStatus.installed ? "已安装" : "未安装"}</dd></div>
            <div><dt>KimiCU MCP 注册状态</dt><dd>{kimiStatus?.mcpRegistered == null ? "尚未确认" : kimiStatus.mcpRegistered ? "已注册并启用" : "未启用"}</dd></div></>}
          </dl>
          {IS_MAC && external.map(({ title, tool, macos }) => <section key={tool} className="runtime-group external-control-status">
            <h4>{title}</h4><p role="status">{macControlLabel(macos)}</p>
            <dl className="runtime-facts">
              <div><dt>辅助功能</dt><dd>{permissionLabel(macos?.accessibility)}</dd></div>
              <div><dt>屏幕录制</dt><dd>{permissionLabel(macos?.screenRecording)}</dd></div>
              <div><dt>MCP 配置</dt><dd>{macos?.mcpRegistered == null ? "尚未确认" : macos.mcpRegistered ? "已注册" : "未注册或需重新连接"}</dd></div>
              {tool === "kimiCu" && <div><dt>后台服务</dt><dd>{macos?.serviceRunning == null ? "待检查" : macos.serviceRunning ? "运行中" : "未运行"}</dd></div>}
            </dl>
            <div className="runtime-inline-actions">
              <button type="button" className="btn secondary" disabled={busy || !macos?.supported || !macos.installed} onClick={() => void macAction(tool, false)}>检查 {title} 权限</button>
              <button type="button" className="btn secondary" disabled={busy || !macos?.supported || !macos.installed} onClick={() => void macAction(tool, true)}>{tool === "kimiCu" ? "打开 KimiCU 授权与后台服务" : "打开 Open Computer Use 授权"}</button>
              <button type="button" className="btn secondary" disabled={busy || !macos?.supported || !macos.installed || !(tool === "kimiCu" ? kimiDraft.enabled : openDraft.enabled)} onClick={async () => {
                if (saveInFlight.current) return;
                saveInFlight.current = true; setBusy(true); setNotice(null);
                try {
                  if (tool === "kimiCu") setKimiStatus(await invoke<KimiComputerUseStatus>("kimi_computer_use_save", { settings: kimiDraft }));
                  else setOpenStatus(await invoke<OpenComputerUseStatus>("open_computer_use_save", { settings: openDraft }));
                  setNotice("MCP 已重新注册；请重开 Agent 会话验证工具连接。");
                } catch (e) { setNotice(`重新连接失败：${String(e)}`); }
                finally { saveInFlight.current = false; setBusy(false); }
              }}>重新连接 {title}</button>
            </div>
          </section>)}
          {IS_MAC && <><p>首次使用：先安装官方运行时，再重新检测。授权须由你在系统设置中完成；不会自动授予权限。</p><button type="button" className="btn secondary" disabled={busy || loading} onClick={() => setReload(n => n + 1)}>重新检测本机运行时</button></>}
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

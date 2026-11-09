import { IS_MAC } from "../platform";
import "../styles/settings-workspace.css";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { SettingsGroup, SettingSwitch, SettingsActions } from "./ui/RuntimeSettings";

import {
  DEFAULT_GENERAL_SETTINGS,
  GENERAL_HW_ACCEL_DESCRIPTION,
  GENERAL_KEEP_AWAKE_DESCRIPTION,
  GENERAL_PROXY_COVERAGE,
  GENERAL_PROXY_RESTART_HINT,
  GENERAL_SETTINGS_PREVIEW,
  GENERAL_TRAY_DESCRIPTION,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  type GeneralSettings,
  type GeneralSettingsStatus,
  validateGeneralSettings,
} from "../generalSettings";

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";

const FONT_SIZE_OPTIONS = Array.from({ length: MAX_TERMINAL_FONT_SIZE - MIN_TERMINAL_FONT_SIZE + 1 }, (_, i) => MIN_TERMINAL_FONT_SIZE + i);

export function GeneralSettingsPanel() {
  const [status, setStatus] = useState<GeneralSettingsStatus | null>(
    UI_PREVIEW ? GENERAL_SETTINGS_PREVIEW : null,
  );
  const [draft, setDraft] = useState<GeneralSettings>(
    UI_PREVIEW ? GENERAL_SETTINGS_PREVIEW.settings : DEFAULT_GENERAL_SETTINGS,
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
    void invoke<GeneralSettingsStatus>("general_settings_get")
      .then((latest) => {
        if (disposed) return;
        setStatus(latest);
        setDraft(latest.settings);
      })
      .catch((reason) => { if (!disposed) setNotice(`读取常规设置失败：${String(reason)}`); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [reload]);

  const dirty = JSON.stringify(draft) !== JSON.stringify(status?.settings ?? null);
  const errors = validateGeneralSettings(draft);
  const save = async () => {
    if (!status || saveInFlight.current || errors.length) return;
    saveInFlight.current = true;
    setBusy(true);
    setNotice(null);
    try {
      const latest = await invoke<GeneralSettingsStatus>("general_settings_save", { settings: draft });
      setStatus(latest);
      setDraft(latest.settings);
      setNotice("已保存并应用（终端字体即时生效，网络类设置对新会话立即生效）");
    } catch (reason) {
      setNotice(`保存失败：${String(reason)}`);
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  };

  return (
    <div className="runtime-settings settings-workspace general-settings-panel">
      <fieldset className="workspace-general-grid" disabled={busy || !status}>
        <div className="workspace-section workspace-network">
          <SettingsGroup title="网络代理" description="配置模型请求与 Agent 会话的网络出口。">
            <div className="runtime-field">
              <div><label htmlFor="general-http-proxy">HTTP 代理</label><small id="general-proxy-hint">留空时沿用系统环境变量。</small></div>
              <input id="general-http-proxy" aria-describedby="general-proxy-hint" value={draft.httpProxy} placeholder="http://127.0.0.1:7890" onChange={e => setDraft({ ...draft, httpProxy: e.target.value })} />
            </div>
            <div className="runtime-field">
              <label htmlFor="general-http-no-proxy">不使用代理的地址<small>多个地址用逗号分隔，本机回环始终直连。</small></label>
              <input id="general-http-no-proxy" value={draft.httpNoProxy} placeholder="localhost, 127.0.0.1, .internal" onChange={e => setDraft({ ...draft, httpNoProxy: e.target.value })} />
            </div>
            <div className="runtime-field">
              <label htmlFor="general-http-ca-cert">CA 证书路径<small>可选，用于信任企业代理的 PEM 根证书。</small></label>
              <input id="general-http-ca-cert" value={draft.httpCaCertPath} placeholder="PEM 文件的绝对路径" onChange={e => setDraft({ ...draft, httpCaCertPath: e.target.value })} />
            </div>
          </SettingsGroup>
        </div>
        <div className="workspace-section workspace-window">
          <SettingsGroup title="窗口与电源">
            <SettingSwitch title="关闭窗口时隐藏到托盘" hint={GENERAL_TRAY_DESCRIPTION} checked={draft.closeToTray} onChange={closeToTray => setDraft({ ...draft, closeToTray })} />
            <SettingSwitch title="保持电脑运行" hint={IS_MAC ? "macOS 暂不支持保持电脑运行" : GENERAL_KEEP_AWAKE_DESCRIPTION} disabled={IS_MAC} checked={!IS_MAC && draft.keepAwake} onChange={keepAwake => setDraft({ ...draft, keepAwake })} />
          </SettingsGroup>
        </div>
        <div className="workspace-section workspace-terminal">
          <SettingsGroup title="终端">
            <div className="runtime-field">
              <label htmlFor="general-terminal-font">字体<small>留空使用默认终端字体。</small></label>
              <input id="general-terminal-font" value={draft.terminalFontFamily} placeholder="Cascadia Code" onChange={e => setDraft({ ...draft, terminalFontFamily: e.target.value })} />
            </div>
            <div className="runtime-field">
              <label htmlFor="general-terminal-font-size">字号<small>保存后即时应用到终端。</small></label>
              <select id="general-terminal-font-size" value={draft.terminalFontSize} onChange={e => setDraft({ ...draft, terminalFontSize: Number(e.target.value) })}>
                {FONT_SIZE_OPTIONS.map(size => <option key={size} value={size}>{size} px</option>)}
              </select>
            </div>
          </SettingsGroup>
        </div>
        <div className="workspace-section workspace-advanced">
          <SettingsGroup title="高级">
            <SettingSwitch title="WebView2 硬件加速" hint={IS_MAC ? "macOS 使用系统 WebKit，此项仅适用于 Windows WebView2" : GENERAL_HW_ACCEL_DESCRIPTION}
              disabled={IS_MAC} checked={!IS_MAC && draft.webview2HardwareAcceleration} onChange={webview2HardwareAcceleration => setDraft({ ...draft, webview2HardwareAcceleration })} />
          </SettingsGroup>
        </div>
      </fieldset>
      {status && <details className="runtime-details">
        <summary>查看运行状态</summary>
        <dl><div><dt>代理出口</dt><dd>{status.proxyEnvActive ? "已按配置覆盖" : "沿用系统环境变量"}</dd></div>
          <div><dt>保持运行</dt><dd>{IS_MAC || status.keepAwakeActive === null ? "当前平台不支持" : status.keepAwakeActive ? "正在阻止休眠" : "待命"}</dd></div>
          <div><dt>关闭行为</dt><dd>{status.settings.closeToTray ? "隐藏到托盘" : "退出应用"}</dd></div></dl>
        <p>{GENERAL_PROXY_COVERAGE} {GENERAL_PROXY_RESTART_HINT}</p>
      </details>}
      {status?.warning && <p className="runtime-warning" role="alert">{status.warning}</p>}
      <SettingsActions ready={!!status} loading={loading} busy={busy} dirty={dirty} invalid={errors.length > 0} message={errors[0] || notice} onSave={save} onRetry={() => setReload(n => n + 1)} />
    </div>
  );
}

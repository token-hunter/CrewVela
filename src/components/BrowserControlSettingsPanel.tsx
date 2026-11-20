import { IS_MAC } from "../platform";
import "../styles/settings-workspace.css";
import "../styles/browser-settings-single-column.css";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SettingsGroup, SettingSwitch, SettingsActions } from "./ui/RuntimeSettings";
import {
  BROWSER_CHANNEL_OPTIONS,
  BROWSER_CONTROL_COVERAGE,
  BROWSER_CONTROL_PANEL_HINT,
  BROWSER_CONTROL_PREVIEW,
  DEFAULT_BROWSER_CONTROL_SETTINGS,
  validateBrowserControlSettings,
  type BrowserChannelId,
  type BrowserControlSettings,
  type BrowserControlStatus,
} from "../browserControlSettings";

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";

export function BrowserControlSettingsPanel() {
  const [status, setStatus] = useState<BrowserControlStatus | null>(
    UI_PREVIEW ? BROWSER_CONTROL_PREVIEW : null,
  );
  const [draft, setDraft] = useState<BrowserControlSettings>(
    UI_PREVIEW ? BROWSER_CONTROL_PREVIEW.settings : DEFAULT_BROWSER_CONTROL_SETTINGS,
  );
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [loading, setLoading] = useState(!UI_PREVIEW);
  const [reload, setReload] = useState(0);
  const saveInFlight = useRef(false);

  useEffect(() => {
    if (UI_PREVIEW) return;
    let disposed = false;
    setLoading(true); setNotice(null);
    void invoke<BrowserControlStatus>("browser_control_get")
      .then((latest) => {
        if (disposed) return;
        setStatus(latest);
        setDraft(latest.settings);
      })
      .catch((reason) => { if (!disposed) setNotice(`读取浏览器控制设置失败：${String(reason)}`); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [reload]);

  const dirty = JSON.stringify(draft) !== JSON.stringify(status?.settings ?? null);

  const save = async () => {
    if (!status || saveInFlight.current) return;
    setFormError(null);
    setNotice(null);
    const localError = validateBrowserControlSettings(draft);
    if (localError) {
      setFormError(localError);
      return;
    }
    saveInFlight.current = true; setBusy(true);
    try {
      const latest = await invoke<BrowserControlStatus>("browser_control_save", {
        settings: draft,
      });
      setStatus(latest);
      setDraft(latest.settings);
      setNotice(latest.settings.enabled ? "已开启浏览器控制" : "已关闭浏览器控制");
    } catch (reason) {
      setNotice(`保存失败：${String(reason)}`);
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  };

  const selectChannel = (channel: BrowserChannelId) => {
    setDraft((current) => ({ ...current, browser: channel }));
  };

  return (
    <div className="runtime-settings settings-workspace browser-settings">
      <div className="workspace-browser-stack">
        <fieldset className="workspace-section workspace-configuration" disabled={busy || !status}>
          <div className="browser-master-card">
            <SettingsGroup title="启用浏览器控制">
              <SettingSwitch title="允许 Agent 操作浏览器" hint="打开网页、读取内容、点击与填写表单。" checked={draft.enabled} onChange={enabled => setDraft({ ...draft, enabled })} />
            </SettingsGroup>
          </div>
          <fieldset className="workspace-browser-options" disabled={!draft.enabled}>
            <div className="browser-option-card">
              <SettingsGroup title="浏览器" description="选择外部网页自动化调用的浏览器渠道。">
                <div className="runtime-field browser-channel-field">
                  <div><label htmlFor="browser-control-channel">{IS_MAC ? "外部浏览器" : "使用的浏览器"}</label>
                    <small id="browser-control-channel-hint">{IS_MAC && draft.preferPanel ? "关闭内置面板优先后使用此配置。" : BROWSER_CHANNEL_OPTIONS.find(option => option.id === draft.browser)?.hint}</small>
                  </div>
                  <select id="browser-control-channel" aria-describedby="browser-control-channel-hint" value={draft.browser} onChange={event => selectChannel(event.target.value as BrowserChannelId)}>
                    {BROWSER_CHANNEL_OPTIONS.map(option => <option key={option.id} value={option.id}>{option.label}</option>)}
                  </select>
                </div>
                {draft.browser === "custom" && <div className="runtime-path">
                  <label htmlFor="browser-control-custom-path">浏览器可执行文件路径</label>
                  <input id="browser-control-custom-path" value={draft.customPath} placeholder={IS_MAC ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" : "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"} onChange={e => setDraft({ ...draft, customPath: e.target.value })} />
                </div>}
              </SettingsGroup>
            </div>
            <div className="browser-option-card">
              <SettingsGroup title="运行方式" description="控制应用内嵌面板优先级与后台静默模式。">
                <SettingSwitch title="无头模式" hint="使用外部浏览器时隐藏窗口。" checked={draft.headless} onChange={headless => setDraft({ ...draft, headless })} />
                <SettingSwitch title="优先内置浏览器面板" hint={IS_MAC ? "在应用内显示网页操作；如需外部 Chrome，请关闭此项并重启会话。" : "在应用内显示操作，面板不可用时使用外部浏览器。"} checked={draft.preferPanel} onChange={preferPanel => setDraft({ ...draft, preferPanel })} />
              </SettingsGroup>
            </div>
          </fieldset>
        </fieldset>
        <aside className="workspace-section workspace-status browser-status-card" aria-label="浏览器运行状态">
        {status && <SettingsGroup title="当前运行环境" description="已保存的配置。">
          <dl className="browser-saved-facts"><div><dt>浏览器</dt><dd>{status.resolvedBrowser?.match(/^Chrome(?=[（(\s]|$)/)?.[0] ?? status.resolvedBrowser ?? "尚未解析浏览器"}</dd></div></dl>
          <details className="runtime-details"><summary>引擎与接入详情</summary>
            <dl><div><dt>浏览器</dt><dd>{status.resolvedBrowser ?? "尚未解析浏览器"}</dd></div>
              <div><dt>引擎</dt><dd>{status.engine}</dd></div>
              {status.panel && <div><dt>面板</dt><dd>{status.panel}</dd></div>}</dl>
            <p>{BROWSER_CONTROL_COVERAGE}</p>
            {!IS_MAC && <p>{BROWSER_CONTROL_PANEL_HINT}</p>}
          </details>
        </SettingsGroup>}
        {status?.warning && <p className="runtime-warning" role="alert">{status.warning}</p>}
        </aside>
      </div>
      <SettingsActions ready={!!status} loading={loading} busy={busy} dirty={dirty} message={formError || notice} hint="更改后需重启会话。" onSave={save} onRetry={() => setReload(n => n + 1)} />
    </div>
  );
}

import { LocalizedErrorText } from "./LocalizedErrorText";
import { useTranslation } from "../useInterfaceLanguage";
import { IS_MAC } from "../platform";
import "../styles/settings-workspace.css";
import "../styles/browser-settings-single-column.css";
import "../styles/settings-creative-modes.css";
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
import type { BrowserControlSettingsStyle } from "../types";

const UI_PREVIEW = new URLSearchParams(window.location.search).get("ui-preview") === "1";

interface Props {
  viewStyle?: BrowserControlSettingsStyle;
}

export function BrowserControlSettingsPanel({ viewStyle = "classic" }: Props) {
  const t = useTranslation();
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

  const shortResolvedBrowser =
    status?.resolvedBrowser?.match(/^Chrome(?=[（(\s]|$)/)?.[0] ??
    status?.resolvedBrowser ??
    "尚未解析浏览器";

  if (viewStyle === "patchbay") {
    const channelMeta =
      BROWSER_CHANNEL_OPTIONS.find((option) => option.id === draft.browser) ??
      BROWSER_CHANNEL_OPTIONS[0];
    const controlsLocked = busy || !status;
    const cordPath = !draft.enabled
      ? "M 76 88 C 92 128, 72 156, 114 152"
      : draft.preferPanel
        ? "M 76 88 C 118 88, 132 88, 174 88 C 218 88, 224 56, 268 56"
        : "M 76 88 C 118 88, 132 88, 174 88 C 218 88, 224 122, 268 122";
    const activeStroke = !draft.enabled
      ? "var(--text-dim)"
      : draft.preferPanel
        ? "var(--accent)"
        : draft.headless
          ? "#6aa6f8"
          : "var(--success)";

    const toggleEnabledFromJack = () => {
      if (controlsLocked) return;
      setDraft((current) => ({ ...current, enabled: !current.enabled }));
    };
    const routePanelFromJack = () => {
      if (controlsLocked) return;
      setDraft((current) => ({ ...current, enabled: true, preferPanel: true }));
    };
    const routeExternalFromJack = () => {
      if (controlsLocked) return;
      setDraft((current) => ({ ...current, enabled: true, preferPanel: false }));
    };

    return (
      <div className="runtime-settings settings-workspace browser-settings browser-patchbay-mode">
        <div className="workspace-browser-stack">
          <fieldset className="workspace-section workspace-configuration bc-patch-fieldset" disabled={controlsLocked}>
            <section className="bc-patch-hero">
              <div className="bc-patch-master">
                <SettingSwitch
                  title={t("允许 Agent 操作浏览器")}
                  hint="打开网页、读取内容、点击与填写表单。"
                  checked={draft.enabled}
                  disabled={controlsLocked}
                  onChange={(enabled) => setDraft((current) => ({ ...current, enabled }))}
                />
              </div>

              <div className="bc-patch-stage">
                <svg className="bc-patch-svg" viewBox="0 0 350 176" aria-label={t("网页跳线总机实时链路示意图")}>
                  <rect x="12" y="12" width="326" height="152" rx="15" fill="var(--bg-elev)" stroke="var(--border-strong)" strokeWidth="1.4" />
                  <circle cx="24" cy="24" r="3" fill="var(--bg-hover)" stroke="var(--border-strong)" />
                  <circle cx="326" cy="24" r="3" fill="var(--bg-hover)" stroke="var(--border-strong)" />
                  <circle cx="24" cy="152" r="3" fill="var(--bg-hover)" stroke="var(--border-strong)" />
                  <circle cx="326" cy="152" r="3" fill="var(--bg-hover)" stroke="var(--border-strong)" />

                  {draft.enabled && draft.preferPanel && (
                    <path
                      d="M 174 88 C 212 104, 226 122, 268 122"
                      fill="none"
                      stroke="var(--text-dim)"
                      strokeWidth="1.4"
                      strokeDasharray="4 4"
                      opacity="0.55"
                    />
                  )}

                  <g
                    className={`bc-patch-jack${controlsLocked ? " is-disabled" : ""}`}
                    role="button"
                    tabIndex={controlsLocked ? -1 : 0}
                    aria-label={draft.enabled ? t("拔出 Agent 跳线关闭浏览器控制") : t("插入 Agent 跳线开启浏览器控制")}
                    onClick={toggleEnabledFromJack}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        toggleEnabledFromJack();
                      }
                    }}
                  >
                    <rect
                      x="28"
                      y="60"
                      width="70"
                      height="56"
                      rx="11"
                      fill="var(--bg-sunken)"
                      stroke={draft.enabled ? "var(--accent)" : "var(--border)"}
                      strokeOpacity={draft.enabled ? 0.65 : 1}
                    />
                    <circle cx="76" cy="88" r="11" fill="var(--bg-elev)" stroke={draft.enabled ? "var(--accent)" : "var(--text-dim)"} strokeWidth="2.4" />
                    <circle cx="76" cy="88" r="4.5" fill={draft.enabled ? "var(--accent)" : "var(--border-strong)"} />
                    <text x="48" y="84" textAnchor="middle" fontSize="9" fontWeight="700" fill="var(--text)">Agent</text>
                    <text x="48" y="98" textAnchor="middle" fontSize="7.5" fill="var(--text-dim)">{t("总控口")}</text>
                  </g>

                  <g>
                    <rect x="139" y="54" width="70" height="68" rx="11" fill="var(--bg-sunken)" stroke="var(--border-strong)" />
                    <circle cx="174" cy="88" r="8" fill="var(--bg-elev)" stroke="var(--warn, #e5a94c)" strokeWidth="2" />
                    <text x="174" y="69" textAnchor="middle" fontSize="8" fontWeight="650" fill="var(--warn, #e5a94c)">Playwright</text>
                    <text x="174" y="113" textAnchor="middle" fontSize="7.5" fill="var(--text-dim)">{t("MCP 桥")}</text>
                  </g>

                  <g
                    className={`bc-patch-jack${controlsLocked ? " is-disabled" : ""}`}
                    role="button"
                    tabIndex={controlsLocked ? -1 : 0}
                    aria-label={t("切换至优先内置浏览器面板")}
                    onClick={routePanelFromJack}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        routePanelFromJack();
                      }
                    }}
                  >
                    <rect
                      x="244"
                      y="26"
                      width="84"
                      height="54"
                      rx="11"
                      fill="var(--bg-sunken)"
                      stroke={draft.enabled && draft.preferPanel ? "var(--accent)" : "var(--border)"}
                      strokeOpacity={draft.enabled && draft.preferPanel ? 0.65 : 1}
                    />
                    <circle cx="268" cy="56" r="10.5" fill="var(--bg-elev)" stroke={draft.enabled && draft.preferPanel ? "var(--accent)" : "var(--text-dim)"} strokeWidth="2.4" />
                    <circle cx="268" cy="56" r="4.2" fill={draft.enabled && draft.preferPanel ? "var(--accent)" : "var(--border-strong)"} />
                    <text x="303" y="49" textAnchor="middle" fontSize="8.5" fontWeight="700" fill="var(--text)">{t("内置面板")}</text>
                    <text x="303" y="63" textAnchor="middle" fontSize="7.5" fill="var(--text-dim)">{t("应用内嵌")}</text>
                  </g>

                  <g
                    className={`bc-patch-jack${controlsLocked ? " is-disabled" : ""}`}
                    role="button"
                    tabIndex={controlsLocked ? -1 : 0}
                    aria-label={t("切换至直连外部浏览器")}
                    onClick={routeExternalFromJack}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        routeExternalFromJack();
                      }
                    }}
                  >
                    <rect
                      x="244"
                      y="96"
                      width="84"
                      height="54"
                      rx="11"
                      fill="var(--bg-sunken)"
                      stroke={draft.enabled && !draft.preferPanel ? (draft.headless ? "#6aa6f8" : "var(--success)") : "var(--border)"}
                    />
                    <circle cx="268" cy="122" r="10.5" fill="var(--bg-elev)" stroke={draft.enabled && !draft.preferPanel ? (draft.headless ? "#6aa6f8" : "var(--success)") : "var(--text-dim)"} strokeWidth="2.4" />
                    <circle cx="268" cy="122" r="4.2" fill={draft.enabled && !draft.preferPanel ? (draft.headless ? "#6aa6f8" : "var(--success)") : "var(--border-strong)"} />
                    <text x="303" y="115" textAnchor="middle" fontSize="8.5" fontWeight="700" fill="var(--text)">{channelMeta.label}</text>
                    <text x="303" y="129" textAnchor="middle" fontSize="7.5" fill={draft.headless ? "#6aa6f8" : "var(--text-dim)"}>
                      {draft.headless ? t("无头静默帽") : t("外部窗口")}
                    </text>
                  </g>

                  <path d={cordPath} fill="none" stroke="rgba(0,0,0,0.3)" strokeWidth="7.5" strokeLinecap="round" transform="translate(0, 3)" />
                  <path className="bc-patch-cord" d={cordPath} fill="none" stroke={activeStroke} strokeWidth="4.4" strokeLinecap="round" />
                  {draft.enabled && (
                    <path className="bc-patch-pulse" d={cordPath} fill="none" stroke="#ffffff" strokeWidth="2" strokeLinecap="round" opacity="0.72" />
                  )}
                  {!draft.enabled && <rect x="108" y="145" width="14" height="9" rx="3" fill="var(--warn, #e5a94c)" />}
                </svg>

                <div className="bc-patch-telemetry">
                  <div className="bc-patch-chips">
                    <span className={`bc-patch-chip${draft.enabled ? " is-live" : ""}`}>
                      <i />
                      {draft.enabled ? t("跳线已接通") : t("插头已拔出")}
                    </span>
                    <span className="bc-patch-chip is-accent">
                      <i />
                      {draft.preferPanel ? t("主链：内置面板") : t("主链：外部渠道")}
                    </span>
                    <span className={`bc-patch-chip${draft.headless ? " is-cool" : ""}`}>
                      <i />
                      {draft.headless ? t("外部无头静默") : t("外部可见窗口")}
                    </span>
                  </div>
                  <h3>
                    {!draft.enabled
                      ? t("跳线已拔出 · 浏览器链路断开")
                      : draft.preferPanel
                        ? t("已接入内置面板（附外部自动回退线）")
                        : t("已直连外部浏览器 · {browser}", { browser: t(channelMeta.label) })}
                  </h3>
                  <p>
                    {t("点击插孔板上的「Agent 总控口」「内置面板」或「{browser}」插孔可直接拔插换线；虚线表示内置面板不可用时的外部自动回退通道。", { browser: t(channelMeta.label) })}
                  </p>
                </div>
              </div>
            </section>

            <fieldset className={`bc-patch-options-grid${!draft.enabled ? " is-locked" : ""}`} disabled={!draft.enabled}>
              <section className="bc-patch-card">
                <header>
                  <h3>{IS_MAC ? t("外部浏览器") : t("浏览器")}</h3>
                  <p>
                    {IS_MAC && draft.preferPanel
                      ? t("关闭内置面板优先后使用此配置。")
                      : t("选择外部网页自动化调用的浏览器渠道。")}
                  </p>
                </header>
                <div className="bc-patch-channels" role="group" aria-label={t("使用的浏览器")}>
                  {BROWSER_CHANNEL_OPTIONS.map((option) => {
                    const selected = draft.browser === option.id;
                    return (
                      <button
                        key={option.id}
                        type="button"
                        className={`bc-patch-channel-btn${selected ? " selected" : ""}`}
                        aria-pressed={selected}
                        onClick={() => selectChannel(option.id)}
                      >
                        <strong>{t(option.label)}</strong>
                        <small>{t(option.hint)}</small>
                      </button>
                    );
                  })}
                </div>
                {draft.browser === "custom" && (
                  <div className="runtime-path bc-patch-custom-path">
                    <label htmlFor="browser-control-custom-path">{t("浏览器可执行文件路径")}</label>
                    <input
                      id="browser-control-custom-path"
                      value={draft.customPath}
                      placeholder={
                        IS_MAC
                          ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
                          : "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
                      }
                      onChange={(e) => setDraft({ ...draft, customPath: e.target.value })}
                    />
                  </div>
                )}
              </section>

              <section className="bc-patch-card">
                <header>
                  <h3>{t("运行方式")}</h3>
                  <p>{t("控制应用内嵌面板优先级与后台静默模式。")}</p>
                </header>
                <div className="bc-patch-switches">
                  <div className="bc-patch-switch-row">
                    <SettingSwitch
                      title={t("无头模式")}
                      hint="使用外部浏览器时隐藏窗口。"
                      checked={draft.headless}
                      disabled={!draft.enabled}
                      onChange={(headless) => setDraft({ ...draft, headless })}
                    />
                  </div>
                  <div className="bc-patch-switch-row">
                    <SettingSwitch
                      title={t("优先内置浏览器面板")}
                      hint={
                        IS_MAC
                          ? "在应用内显示网页操作；如需外部 Chrome，请关闭此项并重启会话。"
                          : "在应用内显示操作，面板不可用时使用外部浏览器。"
                      }
                      checked={draft.preferPanel}
                      disabled={!draft.enabled}
                      onChange={(preferPanel) => setDraft({ ...draft, preferPanel })}
                    />
                  </div>
                </div>
              </section>
            </fieldset>
          </fieldset>

          <aside className="workspace-section workspace-status browser-status-card bc-patch-status" aria-label={t("浏览器运行状态")}>
            {status && (
              <>
                <div className="bc-patch-status-head">
                  <div>
                    <h3>{t("当前运行环境")}</h3>
                    <small>{t("已保存的配置。")}</small>
                  </div>
                  <span className="bc-patch-fact-pill">
                    <i className={status.settings.enabled ? "on" : ""} />
                    <span>{t("浏览器：")}{shortResolvedBrowser}</span>
                  </span>
                </div>
                <details className="runtime-details">
                  <summary>{t("引擎与接入详情")}</summary>
                  <dl>
                    <div><dt>{t("浏览器")}</dt><dd>{status.resolvedBrowser ?? t("尚未解析浏览器")}</dd></div>
                    <div><dt>{t("引擎")}</dt><dd><LocalizedErrorText source={status.engine} fallback="查看原始诊断" /></dd></div>
                    {status.panel && <div><dt>{t("面板")}</dt><dd><LocalizedErrorText source={status.panel} fallback="查看原始诊断" /></dd></div>}
                  </dl>
                  <p>{t(BROWSER_CONTROL_COVERAGE)}</p>
                  {!IS_MAC && <p>{t(BROWSER_CONTROL_PANEL_HINT)}</p>}
                </details>
              </>
            )}
            {status?.warning && <p className="runtime-warning" role="alert"><LocalizedErrorText source={status.warning} /></p>}
          </aside>
        </div>

        <SettingsActions
          ready={!!status}
          loading={loading}
          busy={busy}
          dirty={dirty}
          message={formError || notice}
          hint={t("更改后需重启会话。")}
          onSave={save}
          onRetry={() => setReload((n) => n + 1)}
        />
      </div>
    );
  }

  return (
    <div className="runtime-settings settings-workspace browser-settings">
      <div className="workspace-browser-stack">
        <fieldset className="workspace-section workspace-configuration" disabled={busy || !status}>
          <div className="browser-master-card">
            <SettingsGroup title={t("启用浏览器控制")}>
              <SettingSwitch title={t("允许 Agent 操作浏览器")} hint="打开网页、读取内容、点击与填写表单。" checked={draft.enabled} onChange={enabled => setDraft({ ...draft, enabled })} />
            </SettingsGroup>
          </div>
          <fieldset className="workspace-browser-options" disabled={!draft.enabled}>
            <div className="browser-option-card">
              <SettingsGroup title={t("浏览器")} description={t("选择外部网页自动化调用的浏览器渠道。")}>
                <div className="runtime-field browser-channel-field">
                  <div><label htmlFor="browser-control-channel">{IS_MAC ? t("外部浏览器") : t("使用的浏览器")}</label>
                    <small id="browser-control-channel-hint">{IS_MAC && draft.preferPanel ? t("关闭内置面板优先后使用此配置。") : t(BROWSER_CHANNEL_OPTIONS.find(option => option.id === draft.browser)?.hint ?? "")}</small>
                  </div>
                  <select id="browser-control-channel" aria-describedby="browser-control-channel-hint" value={draft.browser} onChange={event => selectChannel(event.target.value as BrowserChannelId)}>
                    {BROWSER_CHANNEL_OPTIONS.map(option => <option key={option.id} value={option.id}>{t(option.label)}</option>)}
                  </select>
                </div>
                {draft.browser === "custom" && <div className="runtime-path">
                  <label htmlFor="browser-control-custom-path">{t("浏览器可执行文件路径")}</label>
                  <input id="browser-control-custom-path" value={draft.customPath} placeholder={IS_MAC ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" : "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"} onChange={e => setDraft({ ...draft, customPath: e.target.value })} />
                </div>}
              </SettingsGroup>
            </div>
            <div className="browser-option-card">
              <SettingsGroup title={t("运行方式")} description={t("控制应用内嵌面板优先级与后台静默模式。")}>
                <SettingSwitch title={t("无头模式")} hint="使用外部浏览器时隐藏窗口。" checked={draft.headless} onChange={headless => setDraft({ ...draft, headless })} />
                <SettingSwitch title={t("优先内置浏览器面板")} hint={IS_MAC ? "在应用内显示网页操作；如需外部 Chrome，请关闭此项并重启会话。" : "在应用内显示操作，面板不可用时使用外部浏览器。"} checked={draft.preferPanel} onChange={preferPanel => setDraft({ ...draft, preferPanel })} />
              </SettingsGroup>
            </div>
          </fieldset>
        </fieldset>
        <aside className="workspace-section workspace-status browser-status-card" aria-label={t("浏览器运行状态")}>
        {status && <SettingsGroup title={t("当前运行环境")} description={t("已保存的配置。")}>
          <dl className="browser-saved-facts"><div><dt>{t("浏览器")}</dt><dd>{status.resolvedBrowser?.match(/^Chrome(?=[（(\s]|$)/)?.[0] ?? status.resolvedBrowser ?? t("尚未解析浏览器")}</dd></div></dl>
          <details className="runtime-details"><summary>{t("引擎与接入详情")}</summary>
            <dl><div><dt>{t("浏览器")}</dt><dd>{status.resolvedBrowser ?? t("尚未解析浏览器")}</dd></div>
              <div><dt>{t("引擎")}</dt><dd><LocalizedErrorText source={status.engine} fallback="查看原始诊断" /></dd></div>
              {status.panel && <div><dt>{t("面板")}</dt><dd><LocalizedErrorText source={status.panel} fallback="查看原始诊断" /></dd></div>}</dl>
            <p>{t(BROWSER_CONTROL_COVERAGE)}</p>
            {!IS_MAC && <p>{t(BROWSER_CONTROL_PANEL_HINT)}</p>}
          </details>
        </SettingsGroup>}
        {status?.warning && <p className="runtime-warning" role="alert"><LocalizedErrorText source={status.warning} /></p>}
        </aside>
      </div>
      <SettingsActions ready={!!status} loading={loading} busy={busy} dirty={dirty} message={formError || notice} hint={t("更改后需重启会话。")} onSave={save} onRetry={() => setReload(n => n + 1)} />
    </div>
  );
}

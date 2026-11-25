import { LocalizedErrorText } from "./LocalizedErrorText";
import { useTranslation } from "../useInterfaceLanguage";
import { IS_MAC } from "../platform";
import "../styles/settings-workspace.css";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { SettingsGroup, SettingSwitch, SettingsActions } from "./ui/RuntimeSettings";
import { Switch } from "./ui/Switch";
import { TerminalFontPicker } from "./TerminalFontPicker";
import { TerminalFontPreview } from "./TerminalFontPreview";
import { resolveFontFamily } from "../terminalAppearance";
import type { AppTheme, GeneralSettingsStyle } from "../types";
import "../styles/settings-creative-modes.css";

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

const COFFEE_SAMPLES = {
  code: "async function syncCrew(agent: string) {\n  return await dispatch({ agent, retry: 2 });\n}",
  zh: "中文等宽排版测试：零壹贰叁 0123456789\n对齐标点：「」，。！？ () [] {} <> @ #",
  log: "[15:08:04] INFO  worktree ready -> ./worktrees/ui-polish\n[15:08:05] OK    checks passed (420ms)",
} as const;

type CoffeeSampleKey = "terminal" | keyof typeof COFFEE_SAMPLES;

export function GeneralSettingsPanel({
  theme = "dark",
  viewStyle = "classic",
}: {
  theme?: AppTheme;
  viewStyle?: GeneralSettingsStyle;
}) {
  const t = useTranslation();
  const [status, setStatus] = useState<GeneralSettingsStatus | null>(
    UI_PREVIEW ? GENERAL_SETTINGS_PREVIEW : null,
  );
  const [draft, setDraft] = useState<GeneralSettings>(
    UI_PREVIEW ? GENERAL_SETTINGS_PREVIEW.settings : DEFAULT_GENERAL_SETTINGS,
  );
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ text: string; reason?: string } | null>(null);
  const [loading, setLoading] = useState(!UI_PREVIEW);
  const [reload, setReload] = useState(0);
  const [sampleTab, setSampleTab] = useState<CoffeeSampleKey>("terminal");
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
      .catch((reason) => { if (!disposed) setNotice({ text: "读取常规设置失败：{reason}", reason: String(reason) }); })
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
      setNotice({ text: "已保存并应用（终端字体即时生效，网络类设置对新会话立即生效）" });
    } catch (reason) {
      setNotice({ text: "保存失败：{reason}", reason: String(reason) });
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  };

  const awakeActive = !IS_MAC && draft.keepAwake;

  return (
    <div className="runtime-settings settings-workspace general-settings-panel">
      {viewStyle === "coffee" ? (
        <fieldset className="general-coffee-layout" disabled={busy || !status} style={{ border: 0, padding: 0, margin: 0 }}>
          <section className="general-coffee-card general-coffee-cup-card" aria-label={t("防休眠咖啡杯与系统开关")}>
            <span className="general-coffee-kicker">{t("电源与防休眠")}</span>
            <button
              type="button"
              className="general-coffee-cup-btn"
              disabled={IS_MAC || busy || !status}
              aria-pressed={awakeActive}
              aria-label={t("保持电脑运行")}
              title={IS_MAC ? t("macOS 暂不支持保持电脑运行") : awakeActive ? t("点击盖上杯盖，允许休眠") : t("点击掀盖冲泡，阻止系统休眠")}
              onClick={() => setDraft((current) => ({ ...current, keepAwake: !current.keepAwake }))}
            >
              <svg viewBox="0 0 128 124" width="118" height="114" aria-hidden="true">
                {awakeActive ? (
                  <>
                    <path d="M45 36 C41 26, 51 20, 46 10" fill="none" stroke="var(--accent)" strokeWidth="2.5" strokeLinecap="round" className="general-coffee-steam-1" />
                    <path d="M62 34 C58 24, 68 18, 63 8" fill="none" stroke="var(--accent)" strokeWidth="2.5" strokeLinecap="round" className="general-coffee-steam-2" />
                    <path d="M79 36 C75 26, 85 20, 80 10" fill="none" stroke="var(--accent)" strokeWidth="2.5" strokeLinecap="round" className="general-coffee-steam-3" />
                  </>
                ) : (
                  <path d="M68 14 A13 13 0 1 0 82 28 A10 10 0 1 1 68 14 Z" fill="#6aa6f8" opacity="0.82" />
                )}
                <rect x="28" y="46" width="66" height="52" rx="14" fill="var(--bg-sunken)" stroke={awakeActive ? "var(--accent)" : "var(--border-strong)"} strokeWidth="2" />
                <path d="M94 57 C110 57, 110 85, 94 85" fill="none" stroke={awakeActive ? "var(--accent)" : "var(--border-strong)"} strokeWidth="3.2" strokeLinecap="round" />
                {awakeActive ? (
                  <ellipse cx="61" cy="53" rx="26" ry="5" fill="#7c4728" />
                ) : (
                  <rect x="24" y="42" width="74" height="8" rx="4" fill="var(--border-strong)" />
                )}
                <ellipse cx="61" cy="105" rx="42" ry="6" fill="var(--bg-sunken)" stroke="var(--border)" />
              </svg>
            </button>
            <div className="general-coffee-cup-copy">
              <strong>
                {IS_MAC
                  ? t("macOS 暂不支持防休眠")
                  : awakeActive
                    ? t("防休眠咖啡 · 温热在线")
                    : t("已盖上杯盖 · 允许休眠")}
              </strong>
              <p>{IS_MAC ? t("此项仅在 Windows 平台生效。") : t(GENERAL_KEEP_AWAKE_DESCRIPTION)}</p>
            </div>
            <div className="general-coffee-switches">
              <div className="general-coffee-switch-row">
                <div>
                  <strong>{t("关闭窗口时隐藏到托盘")}</strong>
                  <small>{t(GENERAL_TRAY_DESCRIPTION)}</small>
                </div>
                <Switch
                  className="pet-switch"
                  label={t("关闭窗口时隐藏到托盘")}
                  checked={draft.closeToTray}
                  onChange={(closeToTray) => setDraft({ ...draft, closeToTray })}
                />
              </div>
              <div className="general-coffee-switch-row">
                <div>
                  <strong>{t("WebView2 硬件加速")}</strong>
                  <small>{IS_MAC ? t("仅适用于 Windows WebView2") : t(GENERAL_HW_ACCEL_DESCRIPTION)}</small>
                </div>
                <Switch
                  className="pet-switch"
                  label={t("WebView2 硬件加速")}
                  disabled={IS_MAC}
                  checked={!IS_MAC && draft.webview2HardwareAcceleration}
                  onChange={(webview2HardwareAcceleration) =>
                    setDraft({ ...draft, webview2HardwareAcceleration })
                  }
                />
              </div>
            </div>
          </section>

          <div className="general-coffee-right">
            <section className="general-coffee-card" aria-label={t("终端活字试字台")}>
              <div className="general-coffee-head">
                <div>
                  <strong>{t("终端活字试字台")}</strong>
                  <small>{t("边滑字号边核对真实终端、代码与中文对齐，保存后即时生效。")}</small>
                </div>
                <div className="general-coffee-sample-tabs" role="tablist" aria-label={t("试字样张")}>
                  {([
                    ["terminal", t("真实终端")],
                    ["code", t("代码样张")],
                    ["zh", t("中文对齐")],
                    ["log", t("任务日志")],
                  ] as const).map(([key, label]) => (
                    <button
                      key={key}
                      type="button"
                      role="tab"
                      aria-selected={sampleTab === key}
                      className={sampleTab === key ? "active" : ""}
                      onClick={() => setSampleTab(key)}
                    >
                      {t(label)}
                    </button>
                  ))}
                </div>
              </div>

              <div className="general-coffee-font-bar">
                <TerminalFontPicker
                  value={draft.terminalFontFamily}
                  preview={UI_PREVIEW}
                  onChange={(terminalFontFamily) =>
                    setDraft((current) => ({ ...current, terminalFontFamily }))
                  }
                />
                <label className="general-coffee-size-slider">
                  <span>{t("字号")}</span>
                  <input
                    type="range"
                    aria-label={t("终端字号")}
                    min={MIN_TERMINAL_FONT_SIZE}
                    max={MAX_TERMINAL_FONT_SIZE}
                    value={draft.terminalFontSize}
                    onChange={(event) =>
                      setDraft({ ...draft, terminalFontSize: Number(event.target.value) })
                    }
                  />
                  <code>{draft.terminalFontSize} px</code>
                </label>
              </div>

              {sampleTab === "terminal" ? (
                <TerminalFontPreview
                  fontFamily={draft.terminalFontFamily}
                  fontSize={draft.terminalFontSize}
                  theme={theme}
                />
              ) : (
                <pre
                  className="general-coffee-specimen"
                  style={{
                    fontFamily: resolveFontFamily(draft.terminalFontFamily),
                    fontSize: `${draft.terminalFontSize}px`,
                  }}
                >
                  {COFFEE_SAMPLES[sampleTab]}
                </pre>
              )}
            </section>

            <section className="general-coffee-card" aria-label={t("网络代理")}>
              <div className="general-coffee-head">
                <div>
                  <strong>{t("网络代理")}</strong>
                  <small>{t("配置模型请求与 Agent 会话的网络出口。")}</small>
                </div>
                {status && (
                  <span className={`general-status-badge${status.proxyEnvActive ? " active" : ""}`}>
                    <i />
                    {status.proxyEnvActive ? t("已按配置覆盖") : t("沿用系统环境")}
                  </span>
                )}
              </div>
              <div className="general-coffee-proxy-grid">
                <label className="general-coffee-proxy-field">
                  {t("HTTP 代理")}
                  <input
                    id="general-http-proxy"
                    value={draft.httpProxy}
                    placeholder="http://127.0.0.1:7890"
                    onChange={(e) => setDraft({ ...draft, httpProxy: e.target.value })}
                  />
                </label>
                <label className="general-coffee-proxy-field">
                  {t("不使用代理的地址")}
                  <input
                    id="general-http-no-proxy"
                    value={draft.httpNoProxy}
                    placeholder="localhost, 127.0.0.1, .internal"
                    onChange={(e) => setDraft({ ...draft, httpNoProxy: e.target.value })}
                  />
                </label>
                <label className="general-coffee-proxy-field">
                  {t("CA 证书路径（PEM）")}
                  <input
                    id="general-http-ca-cert"
                    value={draft.httpCaCertPath}
                    placeholder={t("可选 · PEM 绝对路径")}
                    onChange={(e) => setDraft({ ...draft, httpCaCertPath: e.target.value })}
                  />
                </label>
              </div>
            </section>
          </div>
        </fieldset>
      ) : (
        <fieldset className="workspace-general-grid" disabled={busy || !status}>
        <div className="workspace-section workspace-network">
          <SettingsGroup title={t("网络代理")} description={t("配置模型请求与 Agent 会话的网络出口。")}>
            {status && (
              <div className="general-network-status-bar" aria-hidden="true">
                <span className={`general-status-badge${status.proxyEnvActive ? " active" : ""}`}>
                  <i />
                  {status.proxyEnvActive ? t("出口状态：已按配置覆盖") : t("出口状态：沿用系统环境")}
                </span>
                <span className="general-status-meta">{t("对新启动会话立即生效")}</span>
              </div>
            )}
            <div className="general-network-fields">
              <div className="runtime-field">
                <div><label htmlFor="general-http-proxy">{t("HTTP 代理")}</label><small id="general-proxy-hint">{t("留空时沿用系统环境变量。")}</small></div>
                <input id="general-http-proxy" aria-describedby="general-proxy-hint" value={draft.httpProxy} placeholder="http://127.0.0.1:7890" onChange={e => setDraft({ ...draft, httpProxy: e.target.value })} />
              </div>
              <div className="runtime-field">
                <label htmlFor="general-http-no-proxy">{t("不使用代理的地址")}<small>{t("多个地址用逗号分隔，本机回环始终直连。")}</small></label>
                <input id="general-http-no-proxy" value={draft.httpNoProxy} placeholder="localhost, 127.0.0.1, .internal" onChange={e => setDraft({ ...draft, httpNoProxy: e.target.value })} />
              </div>
              <div className="runtime-field">
                <label htmlFor="general-http-ca-cert">{t("CA 证书路径")}<small>{t("可选，用于信任企业代理的 PEM 根证书。")}</small></label>
                <input id="general-http-ca-cert" value={draft.httpCaCertPath} placeholder={t("PEM 文件的绝对路径")} onChange={e => setDraft({ ...draft, httpCaCertPath: e.target.value })} />
              </div>
            </div>
          </SettingsGroup>
        </div>
        <div className="workspace-section workspace-terminal">
          <SettingsGroup title={t("终端")} description={t("定制终端工作区的等宽字体与字号排版，保存后即时生效。")}>
            <div className="general-terminal-controls">
              <div className="runtime-field">
                <label htmlFor="general-terminal-font">{t("字体")}<small>{t("留空使用默认字体，备用字体以逗号分隔。")}</small></label>
                <TerminalFontPicker value={draft.terminalFontFamily} preview={UI_PREVIEW}
                  onChange={terminalFontFamily => setDraft(current => ({ ...current, terminalFontFamily }))} />
              </div>
              <div className="runtime-field general-font-size-field">
                <label htmlFor="general-terminal-font-size">{t("字号")}<small>{t("保存后即时应用到终端。")}</small></label>
                <select id="general-terminal-font-size" value={draft.terminalFontSize} onChange={e => setDraft({ ...draft, terminalFontSize: Number(e.target.value) })}>
                  {FONT_SIZE_OPTIONS.map(size => <option key={size} value={size}>{size} px</option>)}
                </select>
              </div>
            </div>
            <TerminalFontPreview fontFamily={draft.terminalFontFamily} fontSize={draft.terminalFontSize} theme={theme} />
          </SettingsGroup>
        </div>
        <div className="workspace-section workspace-window">
          <SettingsGroup title={t("窗口与电源")} description={t("控制系统托盘驻留与长任务休眠策略。")}>
            <SettingSwitch title={t("关闭窗口时隐藏到托盘")} hint={t(GENERAL_TRAY_DESCRIPTION)} checked={draft.closeToTray} onChange={closeToTray => setDraft({ ...draft, closeToTray })} />
            <SettingSwitch title={t("保持电脑运行")} hint={IS_MAC ? t("macOS 暂不支持保持电脑运行") : t(GENERAL_KEEP_AWAKE_DESCRIPTION)} disabled={IS_MAC} checked={!IS_MAC && draft.keepAwake} onChange={keepAwake => setDraft({ ...draft, keepAwake })} />
          </SettingsGroup>
        </div>
        <div className="workspace-section workspace-advanced">
          <SettingsGroup title={t("高级")} description={t("底层 WebView 硬件渲染加速选项。")}>
            <SettingSwitch title={t("WebView2 硬件加速")} hint={IS_MAC ? t("macOS 使用系统 WebKit，此项仅适用于 Windows WebView2") : t(GENERAL_HW_ACCEL_DESCRIPTION)}
              disabled={IS_MAC} checked={!IS_MAC && draft.webview2HardwareAcceleration} onChange={webview2HardwareAcceleration => setDraft({ ...draft, webview2HardwareAcceleration })} />
          </SettingsGroup>
        </div>
      </fieldset>
      )}
      {status && <details className="runtime-details general-runtime-details">
        <summary>{t("查看运行状态")}</summary>
        <dl><div><dt>{t("代理出口")}</dt><dd>{status.proxyEnvActive ? t("已按配置覆盖") : t("沿用系统环境变量")}</dd></div>
          <div><dt>{t("保持运行")}</dt><dd>{IS_MAC || status.keepAwakeActive === null ? t("当前平台不支持") : status.keepAwakeActive ? t("正在阻止休眠") : t("待命")}</dd></div>
          <div><dt>{t("关闭行为")}</dt><dd>{status.settings.closeToTray ? t("隐藏到托盘") : t("退出应用")}</dd></div></dl>
        <p>{t(GENERAL_PROXY_COVERAGE)} {t(GENERAL_PROXY_RESTART_HINT)}</p>
      </details>}
      {status?.warning && <p className="runtime-warning" role="alert"><LocalizedErrorText source={status.warning} /></p>}
      <SettingsActions ready={!!status} loading={loading} busy={busy} dirty={dirty} invalid={errors.length > 0} message={errors[0] ? t(errors[0]) : notice && t(notice.text, { reason: notice.reason ?? "" })} onSave={save} onRetry={() => setReload(n => n + 1)} />
    </div>
  );
}

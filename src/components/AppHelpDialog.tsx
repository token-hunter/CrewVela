import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowSquareOut, ArrowClockwise, X, ShieldCheck, DownloadSimple } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";
import { APP_RELEASES_URL, trustedReleaseUrl, type AppHelpSection, type AppUpdateCheck } from "../appHelp";
import appLogo from "../../src-tauri/icons/icon.png";
import privacyPolicy from "../../docs/privacy.md?raw";
import englishPrivacyPolicy from "../../docs/privacy.en.md?raw";
import { useInterfaceLanguage } from "../useInterfaceLanguage";
import "../styles/app-help.css";

const titles = { privacy: "隐私政策", updates: "检查更新", about: "关于 CrewVela" };
export function AppHelpDialog({ section, onClose }: { section: AppHelpSection; onClose: () => void }) {
  const policy = useInterfaceLanguage() === "en" ? englishPrivacyPolicy : privacyPolicy;
  const t = useTranslation();
  const [version, setVersion] = useState<string | null>(null);
  const [versionError, setVersionError] = useState("");
  const [revision, setRevision] = useState(0);
  const [check, setCheck] = useState<AppUpdateCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState("");
  const [linkError, setLinkError] = useState("");
  const [linkTarget, setLinkTarget] = useState("");
  const mounted = useRef(true), inFlight = useRef(false), opening = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    if (section === "privacy") return;
    let alive = true; setVersionError("");
    void getVersion().then(value => { if (alive) setVersion(value); }).catch(() => { if (alive) setVersionError("无法读取当前程序版本，请重试。"); });
    return () => { alive = false; };
  }, [section, revision]);
  const runCheck = async () => {
    if (inFlight.current) return;
    inFlight.current = true; setChecking(true); setError(""); setCheck(null);
    try {
      const result = await invoke<AppUpdateCheck>("app_check_update");
      const valid = result.status === "unavailable"
        ? result.releaseUrl === null && result.latestVersion === null
        : ["available", "current", "ahead"].includes(result.status) && trustedReleaseUrl(result.releaseUrl);
      if (!valid) throw Error("发行信息无效，未打开未知链接。");
      if (mounted.current) { setCheck(result); setVersion(result.currentVersion); setVersionError(""); }
    } catch (reason) { if (mounted.current) setError(String(reason)); }
    finally { inFlight.current = false; if (mounted.current) setChecking(false); }
  };
  useEffect(() => { if (section === "updates") void runCheck(); }, [section]);
  const external = async (url: string) => {
    if (opening.current) return;
    opening.current = true; setLinkError(""); setLinkTarget(url);
    try { await openUrl(url); }
    catch { if (mounted.current) setLinkError("无法打开浏览器，请重试或复制下方官方地址。"); }
    finally { opening.current = false; }
  };
  if (section === "about") return createPortal(<Modal className="app-help-dialog app-help-about" overlayClassName="dialog-overlay app-help-overlay" ariaLabel={titles.about} onClose={onClose} closeOnOverlayClick>
    <button type="button" className="btn-icon app-about-close" aria-label={t("关闭帮助窗口")} title={t("关闭")} onClick={onClose}><X size={19} aria-hidden="true" /></button>
    <div className="app-help-content app-about-content"><div className="app-about-identity">
      <span className="app-about-logo-frame"><img className="app-about-logo" src={appLogo} alt="CrewVela Logo" /></span>
      <h2 className="app-about-name">CrewVela</h2>
      <p className="app-help-version">{t("版本")} {version ?? (versionError ? t("未取得") : t("正在读取…"))}</p>
      {versionError && <div className="app-about-error" role="alert"><p>{versionError}</p><button type="button" className="app-help-link" onClick={() => setRevision(value => value + 1)}>{t("重新读取版本")}</button></div>}
    </div></div>
  </Modal>, document.body);
  const Icon = section === "privacy" ? ShieldCheck : DownloadSimple;
  return createPortal(<Modal className={`app-help-dialog app-help-${section}`} overlayClassName="dialog-overlay app-help-overlay" labelledBy="app-help-title" onClose={onClose} closeOnOverlayClick>
    <header><div><Icon size={21} /><h2 id="app-help-title">{titles[section]}</h2></div><button className="btn-icon" aria-label={t("关闭帮助窗口")} onClick={onClose}><X size={19} /></button></header>
    <div className="app-help-content">
      {section === "privacy" && <article className="app-privacy-copy">{policy.trim().split(/\r?\n\s*\r?\n/).filter(block => !block.startsWith("# ")).map((block, i) => block.startsWith("## ") ? <h3 key={i}>{block.slice(3)}</h3> : <p key={i}>{block}</p>)}</article>}
      {section !== "privacy" && <p className="app-help-version">{t("当前版本：")}{version ?? (versionError ? t("未取得") : t("正在读取…"))}</p>}
      {versionError && <div role="alert"><p>{versionError}</p><button className="btn" onClick={() => setRevision(value => value + 1)}>{t("重新读取版本")}</button></div>}
      {section === "updates" && <>
        {checking && <p role="status" className="app-help-checking"><ArrowClockwise size={17} />{t("正在检查 CrewVela 更新…")}</p>}
        {error && <div className="app-help-error" role="alert"><strong>{t("未能完成更新检查")}</strong><p>{error}</p></div>}
        {check && <section className={`app-update-result ${check.status}`} aria-label={t("更新检查结果")}>
          {check.status === "unavailable" ? <h3>{t("暂无适用于当前平台和更新通道的安装包")}</h3> : <>
            <h3>{check.status === "available" ? t("发现新版本") : check.status === "ahead" ? t("当前版本高于最新兼容版本") : t("已是最新兼容版本")}</h3>
            <p>{t("最新兼容版本：")}{check.latestVersion}</p>
            {check.status === "available" && <button className="btn primary" onClick={() => void external(check.releaseUrl)}>{t("前往发布页下载")}<ArrowSquareOut size={15} /></button>}
            {check.releaseNotes && <details><summary>{t("发行说明")}</summary><pre>{check.releaseNotes}</pre></details>}
          </>}
        </section>}
        <p className="app-help-note">{t("仅检查 CrewVela 应用版本。更新由你在官方发布页下载，本次检查不会自动下载或安装。")}</p>
        <button className="app-help-link" onClick={() => void external(check?.releaseUrl ?? APP_RELEASES_URL)}>{t("查看官方发布页")}<ArrowSquareOut size={14} /></button>
      </>}
      {linkError && <div className="app-help-error" role="alert"><p>{linkError}</p><code>{linkTarget}</code></div>}
    </div>
    <footer>{section === "updates" && <button className="btn" disabled={checking} onClick={() => void runCheck()}><ArrowClockwise size={15} />{checking ? t("检查中…") : t("重新检查")}</button>}<button className="btn" onClick={onClose}>{t("关闭")}</button></footer>
  </Modal>, document.body);
}

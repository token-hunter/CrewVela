import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowSquareOut, ArrowClockwise, X, ShieldCheck, Info, DownloadSimple } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";
import { APP_GUIDE_URL, APP_RELEASES_URL, APP_REPOSITORY_URL, trustedReleaseUrl, type AppHelpSection, type AppUpdateCheck } from "../appHelp";
import privacyPolicy from "../../docs/privacy.md?raw";
import "../styles/app-help.css";

const titles = { privacy: "隐私政策", updates: "检查更新", about: "关于 CrewVela" };
export function AppHelpDialog({ section, onClose }: { section: AppHelpSection; onClose: () => void }) {
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
      if (!trustedReleaseUrl(result.releaseUrl) || !["available", "current", "ahead"].includes(result.status)) throw Error("发行信息无效，未打开未知链接。");
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
  const Icon = section === "privacy" ? ShieldCheck : section === "updates" ? DownloadSimple : Info;
  return createPortal(<Modal className={`app-help-dialog app-help-${section}`} overlayClassName="dialog-overlay app-help-overlay" labelledBy="app-help-title" onClose={onClose} closeOnOverlayClick>
    <header><div><Icon size={21} /><h2 id="app-help-title">{titles[section]}</h2></div><button className="btn-icon" aria-label="关闭帮助窗口" onClick={onClose}><X size={19} /></button></header>
    <div className="app-help-content">
      {section === "privacy" && <article className="app-privacy-copy">{privacyPolicy.trim().split(/\r?\n\s*\r?\n/).filter(block => !block.startsWith("# ")).map((block, i) => block.startsWith("## ") ? <h3 key={i}>{block.slice(3)}</h3> : <p key={i}>{block}</p>)}</article>}
      {section === "about" && <><h3 className="app-help-product">CrewVela</h3><p>多个 Agent，一个工作台。</p><p>使用本地 Agent CLI 进行会话、任务协作与工作区管理。</p></>}
      {section !== "privacy" && <p className="app-help-version">当前版本：{version ?? (versionError ? "未取得" : "正在读取…")}</p>}
      {versionError && <div role="alert"><p>{versionError}</p><button className="btn" onClick={() => setRevision(value => value + 1)}>重新读取版本</button></div>}
      {section === "about" && <dl className="app-help-facts"><div><dt>开源许可</dt><dd>MIT License</dd></div><div><dt>项目地址</dt><dd><button className="app-help-link" onClick={() => void external(APP_REPOSITORY_URL)}>GitHub · token-hunter/CrewVela<ArrowSquareOut size={14} /></button></dd></div><div><dt>使用手册</dt><dd><button className="app-help-link" onClick={() => void external(APP_GUIDE_URL)}>查看使用手册<ArrowSquareOut size={14} /></button></dd></div></dl>}
      {section === "updates" && <>
        {checking && <p role="status" className="app-help-checking"><ArrowClockwise size={17} />正在检查 CrewVela 的公开稳定发行版…</p>}
        {error && <div className="app-help-error" role="alert"><strong>未能完成更新检查</strong><p>{error}</p></div>}
        {check && <section className={`app-update-result ${check.status}`} aria-label="更新检查结果"><h3>{check.status === "available" ? "发现新版本" : check.status === "ahead" ? "当前版本高于公开稳定版" : "已是公开最新稳定版"}</h3><p>公开稳定版：{check.latestVersion}</p>{check.status === "available" && <button className="btn primary" onClick={() => void external(check.releaseUrl)}>前往发布页下载<ArrowSquareOut size={15} /></button>}{check.releaseNotes && <details><summary>发行说明</summary><pre>{check.releaseNotes}</pre></details>}</section>}
        <p className="app-help-note">仅检查 CrewVela 应用版本。更新由你在官方发布页下载，本次检查不会自动下载或安装。</p>
        <button className="app-help-link" onClick={() => void external(check?.releaseUrl ?? APP_RELEASES_URL)}>查看官方发布页<ArrowSquareOut size={14} /></button>
      </>}
      {linkError && <div className="app-help-error" role="alert"><p>{linkError}</p><code>{linkTarget}</code></div>}
    </div>
    <footer>{section === "updates" && <button className="btn" disabled={checking} onClick={() => void runCheck()}><ArrowClockwise size={15} />{checking ? "检查中…" : "重新检查"}</button>}<button className="btn" onClick={onClose}>关闭</button></footer>
  </Modal>, document.body);
}

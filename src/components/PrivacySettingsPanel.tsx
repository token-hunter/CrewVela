import { LocalizedErrorText } from "./LocalizedErrorText";
import { useInterfaceLanguage, useTranslation } from "../useInterfaceLanguage";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { CaretRight, Copy, FolderOpen, Info, Key, X } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";
import { SettingsSection } from "./ui/SettingsSection";
import { PRIVACY_DATA_ROWS, PRIVACY_EXPLANATIONS, privacySections, type PrivacyDestination, type PrivacyExplanation } from "../privacySettings";
import type { PrivacySettingsStyle } from "../types";
import privacyPolicy from "../../docs/privacy.md?raw";
import englishPrivacyPolicy from "../../docs/privacy.en.md?raw";
import "../styles/privacy-settings.css";
import "../styles/settings-creative-modes.css";

function PrivacyDirectoryDialog({onClose}:{onClose:()=>void}) {
  const t = useTranslation();
  const [directory,setDirectory]=useState<string|null>(null),[loading,setLoading]=useState(true),[error,setError]=useState(""),[notice,setNotice]=useState(""),[busy,setBusy]=useState(false),[revision,setRevision]=useState(0);
  const mounted=useRef(true),inFlight=useRef(false);
  useEffect(()=>{mounted.current=true;return()=>{mounted.current=false;};},[]);
  useEffect(()=>{
    let alive=true;setLoading(true);setDirectory(null);setError("");
    void invoke<string>("app_data_directory").then(path=>{
      if(!path || typeof path!=="string")throw Error("应用未返回有效的数据目录。");
      if(alive)setDirectory(path);
    }).catch(reason=>{if(alive)setError(`读取数据目录失败：${String(reason)}`);}).finally(()=>{if(alive)setLoading(false);});
    return()=>{alive=false;};
  },[revision]);
  const act=async(copy:boolean)=>{
    if(!directory || inFlight.current)return;
    inFlight.current=true;setBusy(true);setError("");setNotice("");
    try {
      if(copy)await writeText(directory);else await invoke("workspace_open_path",{root:directory,relativePath:"."});
      if(mounted.current)setNotice(copy?"已复制目录路径。":"已请求在文件管理器中打开目录。");
    }catch(reason){if(mounted.current)setError(`${copy?"复制路径":"打开目录"}失败：${String(reason)}`);}
    finally{inFlight.current=false;if(mounted.current)setBusy(false);}
  };
  return <Modal className="privacy-dialog" overlayClassName="dialog-overlay privacy-overlay" labelledBy="privacy-location-title" onClose={onClose} closeOnOverlayClick>
    <header><h2 id="privacy-location-title">{t("应用数据目录")}</h2><button className="btn-icon" aria-label={t("关闭数据目录")} onClick={onClose}><X size={19}/></button></header>
    <div className="privacy-dialog-content"><p>{t("这是当前 CrewVela 实际使用的应用数据目录。")}</p>
      {loading&&<p role="status">{t("正在读取目录…")}</p>}{directory&&<code className="privacy-directory-path">{directory}</code>}
      <p className="privacy-note">{t("CLI 原生历史与共享记忆服务数据可能位于其他位置，需要分别管理。")}</p>
      {error&&<p className="privacy-error" role="alert"><LocalizedErrorText source={error} /></p>}{notice&&<p role="status">{t(notice)}</p>}
      {!loading&&!directory&&<button className="btn" onClick={()=>setRevision(value=>value+1)}>{t("重新读取目录")}</button>}
    </div>
    <footer>{directory&&<><button className="btn" disabled={busy} onClick={()=>void act(true)}><Copy size={15}/>{t("复制路径")}</button><button className="btn" disabled={busy} onClick={()=>void act(false)}><FolderOpen size={15}/>{t("打开目录")}</button></>}<button className="btn" onClick={onClose}>{t("关闭")}</button></footer>
  </Modal>;
}

const VAULT_LOCKS = [
  {
    id: "local",
    angle: 0,
    title: "本机优先存储",
    description: "会话记录、协作快照、快捷指令与通知历史均持久化保存在本机磁盘，不上传中转云端。",
  },
  {
    id: "cli",
    angle: 120,
    title: "CLI 独立鉴权",
    description: "模型请求由各 Agent CLI 本地配置直连发起，CrewVela 不代管、不中转你的模型密钥。",
  },
  {
    id: "boundary",
    angle: 240,
    title: "透明数据边界",
    description: "应用层数据、CLI 原生历史与共享记忆彼此隔离，可按模块随时独立审阅与清理。",
  },
] as const;

export function PrivacySettingsPanel({
  onNavigate,
  viewStyle = "classic",
}: {
  onNavigate: (destination: PrivacyDestination) => void;
  viewStyle?: PrivacySettingsStyle;
}) {
  const t = useTranslation();
  const [detail, setDetail] = useState<PrivacyExplanation | "directory" | null>(null);
  const [activeLockId, setActiveLockId] = useState<(typeof VAULT_LOCKS)[number]["id"]>("local");
  const explanation = detail && detail !== "directory" ? PRIVACY_EXPLANATIONS[detail] : null;
  const policy = useInterfaceLanguage() === "en" ? englishPrivacyPolicy : privacyPolicy;
  const sections = explanation ? privacySections(policy, explanation.headings.map(heading => t(heading))) : [];
  const activeLock = VAULT_LOCKS.find((item) => item.id === activeLockId) ?? VAULT_LOCKS[0];

  return <section className="privacy-settings" aria-label={t("隐私设置")}>
    {viewStyle === "vault" ? (
      <>
        <div className="privacy-vault-hero">
          <div className="privacy-vault-dial-wrap">
            <svg viewBox="0 0 180 180" width="160" height="160" aria-hidden="true">
              <circle cx="90" cy="90" r="76" fill="var(--bg-sunken)" stroke="var(--border-strong)" strokeWidth="2" />
              <circle cx="90" cy="90" r="61" fill="none" stroke="var(--border)" strokeDasharray="4 6" />
              <g
                style={{
                  transformOrigin: "90px 90px",
                  transform: `rotate(${activeLock.angle}deg)`,
                  transition: "transform 420ms cubic-bezier(0.22, 1, 0.36, 1)",
                }}
              >
                <circle cx="90" cy="90" r="42" fill="var(--bg-elev)" stroke="var(--accent)" strokeWidth="2.2" />
                <line x1="90" y1="52" x2="90" y2="70" stroke="var(--accent)" strokeWidth="3.5" strokeLinecap="round" />
                <circle cx="90" cy="90" r="10" fill="color-mix(in srgb, var(--accent) 16%, transparent)" stroke="var(--accent)" />
              </g>
            </svg>
            <span className="privacy-vault-badge">{t("● 100% 本地落盘守护")}</span>
          </div>

          <div className="privacy-vault-copy">
            <div className="privacy-vault-locks" role="tablist" aria-label={t("本机隐私守护锁芯")}>
              {VAULT_LOCKS.map((lock) => (
                <button
                  key={lock.id}
                  type="button"
                  role="tab"
                  aria-selected={lock.id === activeLock.id}
                  className={`privacy-vault-lock-btn${lock.id === activeLock.id ? " active" : ""}`}
                  onClick={() => setActiveLockId(lock.id)}
                >
                  {lock.title}
                </button>
              ))}
            </div>
            <div className="privacy-vault-explain">
              <strong>{activeLock.title}</strong>
              <p>{activeLock.description}</p>
            </div>
            <div className="privacy-vault-policy-links">
              {(["storage", "services"] as const).map((id) => (
                <button key={id} type="button" className="btn secondary" onClick={() => setDetail(id)}>
                  <span>{t(PRIVACY_EXPLANATIONS[id].title)}</span>
                  <CaretRight size={14} aria-hidden="true" />
                </button>
              ))}
            </div>
          </div>
        </div>

        <SettingsSection className="privacy-group privacy-data-group" title={t("你的数据抽屉")}>
          <div className="privacy-vault-drawers">
            {PRIVACY_DATA_ROWS.map((row) => (
              <article className="privacy-vault-drawer" key={row.title}>
                <div>
                  <div className="privacy-vault-drawer-head">
                    <Key size={15} aria-hidden="true" />
                    <span>{t(row.title)}</span>
                  </div>
                  <p>{t(row.description)}</p>
                </div>
                <button
                  type="button"
                  className="btn secondary"
                  aria-label={`${t(row.title)}：${t(row.action)}`}
                  onClick={() => (row.destination ? onNavigate(row.destination) : setDetail("directory"))}
                >
                  {t(row.action)}
                </button>
              </article>
            ))}
          </div>
        </SettingsSection>
      </>
    ) : (
      <>
    <div className="privacy-shield-pillars" aria-hidden="true">
      <div className="privacy-pillar-card">
        <strong>{t("本机优先存储")}</strong>
        <p>{t("会话记录、协作快照与通知历史均持久化保存在本机磁盘。")}</p>
      </div>
      <div className="privacy-pillar-card">
        <strong>{t("CLI 独立鉴权")}</strong>
        <p>{t("模型请求由各 Agent CLI 本地配置直连发起，应用不中转密钥。")}</p>
      </div>
      <div className="privacy-pillar-card">
        <strong>{t("透明数据边界")}</strong>
        <p>{t("应用层数据、CLI 原生历史与共享记忆可按模块独立审阅和清理。")}</p>
      </div>
    </div>
    <SettingsSection className="privacy-group" title={t("数据说明")}>
      <div className="privacy-disclosure-grid">
        {(["storage","services"] as const).map(id=><button key={id} type="button" className="privacy-disclosure" onClick={()=>setDetail(id)}><span>{t(PRIVACY_EXPLANATIONS[id].title)}{id==="services"&&<small>{t("请求由各 Agent CLI 与工具的配置决定。")}</small>}</span><CaretRight size={18} aria-hidden="true"/></button>)}
      </div>
    </SettingsSection>
    <SettingsSection className="privacy-group privacy-data-group" title={t("你的数据")}>
      <div className="privacy-data-card">
        {PRIVACY_DATA_ROWS.map(row=><div className="privacy-data-row" key={row.title}><div><h4>{t(row.title)}</h4><p>{t(row.description)}</p></div><button className="btn" aria-label={`${t(row.title)}：${t(row.action)}`} onClick={()=>row.destination?onNavigate(row.destination):setDetail("directory")}>{t(row.action)}</button></div>)}
      </div>
    </SettingsSection>
      </>
    )}
    <p className="privacy-boundary"><Info size={17} aria-hidden="true"/>{t("应用记录、CLI 历史与外部服务数据需分别管理。")}</p>
    {detail==="directory"&&createPortal(<PrivacyDirectoryDialog onClose={()=>setDetail(null)}/>,document.body)}
    {explanation&&createPortal(<Modal className="privacy-dialog privacy-explanation" overlayClassName="dialog-overlay privacy-overlay" labelledBy="privacy-explanation-title" onClose={()=>setDetail(null)} closeOnOverlayClick>
      <header><h2 id="privacy-explanation-title">{t(explanation.title)}</h2><button className="btn-icon" aria-label={t("关闭隐私说明")} onClick={()=>setDetail(null)}><X size={19}/></button></header>
      <article className="privacy-dialog-content">{sections.length?sections.map(section=><section key={section.title}><h3>{section.title}</h3>{section.paragraphs.map((p,i)=><p key={i}>{p}</p>)}</section>):<p>{t("请通过左上角“帮助 → 隐私政策”查看完整的数据说明。")}</p>}</article>
      <footer><button className="btn" onClick={()=>setDetail(null)}>{t("关闭")}</button></footer>
    </Modal>,document.body)}
  </section>;
}

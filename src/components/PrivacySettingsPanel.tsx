import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { CaretRight, Copy, FolderOpen, Info, X } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";
import { SettingsSection } from "./ui/SettingsSection";
import { PRIVACY_DATA_ROWS, PRIVACY_EXPLANATIONS, privacySections, type PrivacyDestination, type PrivacyExplanation } from "../privacySettings";
import privacyPolicy from "../../docs/privacy.md?raw";
import "../styles/privacy-settings.css";

function PrivacyDirectoryDialog({onClose}:{onClose:()=>void}) {
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
    <header><h2 id="privacy-location-title">应用数据目录</h2><button className="btn-icon" aria-label="关闭数据目录" onClick={onClose}><X size={19}/></button></header>
    <div className="privacy-dialog-content"><p>这是当前 CrewVela 实际使用的应用数据目录。</p>
      {loading&&<p role="status">正在读取目录…</p>}{directory&&<code className="privacy-directory-path">{directory}</code>}
      <p className="privacy-note">CLI 原生历史与共享记忆服务数据可能位于其他位置，需要分别管理。</p>
      {error&&<p className="privacy-error" role="alert">{error}</p>}{notice&&<p role="status">{notice}</p>}
      {!loading&&!directory&&<button className="btn" onClick={()=>setRevision(value=>value+1)}>重新读取目录</button>}
    </div>
    <footer>{directory&&<><button className="btn" disabled={busy} onClick={()=>void act(true)}><Copy size={15}/>复制路径</button><button className="btn" disabled={busy} onClick={()=>void act(false)}><FolderOpen size={15}/>打开目录</button></>}<button className="btn" onClick={onClose}>关闭</button></footer>
  </Modal>;
}

export function PrivacySettingsPanel({onNavigate}:{onNavigate:(destination:PrivacyDestination)=>void}) {
  const [detail,setDetail]=useState<PrivacyExplanation|"directory"|null>(null);
  const explanation=detail&&detail!=="directory"?PRIVACY_EXPLANATIONS[detail]:null;
  const sections=explanation?privacySections(privacyPolicy,explanation.headings):[];
  return <section className="privacy-settings" aria-label="隐私设置">
    <SettingsSection className="privacy-group" title="数据说明">
      {(["storage","services"] as const).map(id=><button key={id} type="button" className="privacy-disclosure" onClick={()=>setDetail(id)}><span>{PRIVACY_EXPLANATIONS[id].title}{id==="services"&&<small>请求由各 Agent CLI 与工具的配置决定。</small>}</span><CaretRight size={18} aria-hidden="true"/></button>)}
    </SettingsSection>
    <SettingsSection className="privacy-group privacy-data-group" title="你的数据">
      {PRIVACY_DATA_ROWS.map(row=><div className="privacy-data-row" key={row.title}><div><h4>{row.title}</h4><p>{row.description}</p></div><button className="btn" aria-label={`${row.title}：${row.action}`} onClick={()=>row.destination?onNavigate(row.destination):setDetail("directory")}>{row.action}</button></div>)}
    </SettingsSection>
    <p className="privacy-boundary"><Info size={17} aria-hidden="true"/>应用记录、CLI 历史与外部服务数据需分别管理。</p>
    {detail==="directory"&&createPortal(<PrivacyDirectoryDialog onClose={()=>setDetail(null)}/>,document.body)}
    {explanation&&createPortal(<Modal className="privacy-dialog privacy-explanation" overlayClassName="dialog-overlay privacy-overlay" labelledBy="privacy-explanation-title" onClose={()=>setDetail(null)} closeOnOverlayClick>
      <header><h2 id="privacy-explanation-title">{explanation.title}</h2><button className="btn-icon" aria-label="关闭隐私说明" onClick={()=>setDetail(null)}><X size={19}/></button></header>
      <article className="privacy-dialog-content">{sections.length?sections.map(section=><section key={section.title}><h3>{section.title}</h3>{section.paragraphs.map((p,i)=><p key={i}>{p}</p>)}</section>):<p>请通过左上角“帮助 → 隐私政策”查看完整的数据说明。</p>}</article>
      <footer><button className="btn" onClick={()=>setDetail(null)}>关闭</button></footer>
    </Modal>,document.body)}
  </section>;
}

import { useEffect, useId, useRef, useState } from "react";
import { Check, Copy } from "@phosphor-icons/react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import "../styles/conversation-message-copy.css";

/** Copy the authoritative message body, never DOM text containing status/actions. */
export function ConversationMessageCopy({body}: {body:string}) {
  const [copiedBody,setCopiedBody] = useState<string | null>(null);
  const [error,setError] = useState<{body:string;text:string} | null>(null);
  const [busy,setBusy] = useState(false);
  const current = useRef(body); current.current=body;
  const mounted = useRef(false);
  const pending = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const tooltip = useId();
  useEffect(()=>{
    mounted.current=true;
    return()=>{mounted.current=false;if(timer.current)clearTimeout(timer.current);};
  },[]);
  const copied = copiedBody === body;
  const copy = async()=>{
    if(pending.current || !body.trim())return;
    const content=body;
    pending.current=true;setBusy(true);setCopiedBody(null);setError(null);
    if(timer.current)clearTimeout(timer.current);
    try {
      await writeText(content);
      if(mounted.current && current.current===content){
        setCopiedBody(content);
        timer.current=setTimeout(()=>setCopiedBody(null),2000);
      }
    } catch(reason) {
      if(mounted.current && current.current===content)setError({body:content,text:`复制失败：${String(reason)}`});
    } finally {
      pending.current=false;
      if(mounted.current)setBusy(false);
    }
  };
  return <div className="conversation-message-actions">
    {error?.body===body && <span className="conversation-copy-error" role="alert">{error.text}</span>}
    <span className="conversation-copy-feedback" role="status">{copied ? "已复制" : ""}</span>
    <span className="conversation-copy-action"><button type="button" className="conversation-copy-button" aria-label="复制消息" aria-describedby={tooltip} aria-busy={busy} disabled={busy} onClick={()=>void copy()}>{copied ? <Check size={17} aria-hidden="true"/> : <Copy size={17} aria-hidden="true"/>}</button><span id={tooltip} role="tooltip" className="conversation-copy-tooltip">{busy ? "正在复制…" : copied ? "已复制" : "复制消息"}</span></span>
  </div>;
}

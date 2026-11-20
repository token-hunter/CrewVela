import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { Image, X } from "@phosphor-icons/react";
import { Modal } from "./ui/Modal";

export type ConversationAttachmentSource =
  | { kind: "draft"; path: string }
  | { kind: "message"; projectId: string; conversationId: string; messageId: string; attachmentId: string };

interface Props {
  name: string;
  source: ConversationAttachmentSource;
  onRemove?: () => void;
  removeDisabled?: boolean;
  onOpenFile?: () => void;
}

export function ConversationImageAttachment({ name, source, onRemove, removeDisabled, onOpenFile }: Props) {
  const tile = useRef<HTMLDivElement>(null);
  const [url, setUrl] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  const [open, setOpen] = useState(false);
  const kind = source.kind;
  const path = source.kind === "draft" ? source.path : undefined;
  const message = source.kind === "message" ? source : undefined;
  const { projectId, conversationId, messageId, attachmentId } = message ?? {};

  useEffect(() => {
    const element = tile.current;
    if (!element) return;
    let disposed = false;
    let imageUrl = "";
    const observer = new IntersectionObserver(([entry]) => {
      if (!entry.isIntersecting || disposed) return;
      observer.disconnect();
      const request = kind === "draft" ? { kind, path } : { kind, projectId, conversationId, messageId, attachmentId };
      void invoke<ArrayBuffer | number[]>("ma_conversation_preview_image", { source: request }).then(bytes => {
        if (disposed) return;
        imageUrl = URL.createObjectURL(new Blob([Array.isArray(bytes) ? Uint8Array.from(bytes) : bytes]));
        setUrl(imageUrl);
      }).catch(() => { if (!disposed) setFailed(true); });
    });
    observer.observe(element);
    return () => {
      disposed = true;
      observer.disconnect();
      if (imageUrl) URL.revokeObjectURL(imageUrl);
    };
  }, [kind, path, projectId, conversationId, messageId, attachmentId]);

  return <div ref={tile} className="conversation-image-attachment" data-draft={kind === "draft"}>
    <button type="button" className="conversation-image-thumbnail"
      aria-label={failed ? `${onOpenFile ? "打开文件" : "无法预览"} ${name}` : `预览图片 ${name}`}
      title={failed ? `无法预览 · ${name}` : name}
      disabled={!loaded && !(failed && onOpenFile)}
      onClick={() => { if (failed) onOpenFile?.(); else setOpen(true); }}>
      {url && !failed && <img src={url} alt="" decoding="async" onLoad={() => setLoaded(true)} onError={() => setFailed(true)} />}
      {(!loaded || failed) && <span className="conversation-image-placeholder"><Image size={18} /><span>{name}</span></span>}
    </button>
    {onRemove && <button className="conversation-image-remove" type="button" aria-label={`移除附件 ${name}`} title={`移除附件 ${name}`}
      disabled={removeDisabled} onClick={onRemove}><X size={12} /></button>}
    {open && createPortal(<Modal className="conversation-image-dialog" ariaLabel={`图片预览 ${name}`} closeOnOverlayClick onClose={() => setOpen(false)}>
      <header><strong title={name}>{name}</strong><button type="button" className="conversation-image-close" aria-label="关闭图片预览" onClick={() => setOpen(false)}><X size={19} /></button></header>
      <img src={url} alt={name} />
    </Modal>, document.body)}
  </div>;
}

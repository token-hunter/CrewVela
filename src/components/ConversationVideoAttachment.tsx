import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { FileVideo, X } from "@phosphor-icons/react";
import type { ConversationAttachmentSource } from "./ConversationImageAttachment";
import { Modal } from "./ui/Modal";

interface Props {
  name: string;
  source: ConversationAttachmentSource;
  onRemove?: () => void;
  removeDisabled?: boolean;
}

function VideoPreview({ name, source, onClose }: Pick<Props, "name" | "source"> & { onClose: () => void }) {
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  const [openError, setOpenError] = useState("");

  useEffect(() => {
    let closed = false;
    void invoke<string>("ma_conversation_preview_video", { source }).then(path => {
      if (!closed) setUrl(convertFileSrc(path));
    }).catch(() => { if (!closed) setError("无法读取视频附件。"); });
    return () => { closed = true; };
  }, [source]);

  const openOriginal = async () => {
    setOpenError("");
    try {
      await invoke("ma_conversation_open_video", { source });
    } catch (reason) {
      setOpenError(`打开视频失败：${String(reason)}`);
    }
  };

  return <Modal className="conversation-image-dialog conversation-video-dialog" ariaLabel={`视频预览 ${name}`} closeOnOverlayClick onClose={onClose}>
    <header><strong title={name}>{name}</strong><button type="button" className="conversation-image-close" aria-label="关闭视频预览" onClick={onClose}><X size={19} /></button></header>
    {error ? <p className="conversation-video-status" role="alert">{error}</p> : url
      ? <video src={url} controls preload="metadata" tabIndex={0} aria-label={name} onError={() => setError("内置播放器无法播放此视频，可用系统播放器打开原文件。")} />
      : <p className="conversation-video-status" role="status">正在加载视频…</p>}
    <button type="button" className="conversation-text-button" onClick={() => void openOriginal()}>用系统播放器打开</button>
    {openError && <p className="conversation-video-status" role="alert">{openError}</p>}
  </Modal>;
}

export function ConversationVideoAttachment({ name, source, onRemove, removeDisabled }: Props) {
  const [previewSource, setPreviewSource] = useState<ConversationAttachmentSource | null>(null);

  return <div className="conversation-video-attachment">
    <button type="button" className="conversation-video-preview" aria-label={`预览视频 ${name}`} title={name} onClick={() => setPreviewSource(source)}>
      <FileVideo size={15} aria-hidden="true" /><span>{name}</span>
    </button>
    {onRemove && <button type="button" className="conversation-video-remove" aria-label={`移除附件 ${name}`} title={`移除附件 ${name}`} disabled={removeDisabled} onClick={onRemove}><X size={12} /></button>}
    {previewSource && createPortal(<VideoPreview name={name} source={previewSource} onClose={() => setPreviewSource(null)} />, document.body)}
  </div>;
}

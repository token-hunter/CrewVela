import { useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderIcon } from "@phosphor-icons/react/dist/csr/Folder";
import type { Project } from "../types";
import { Modal } from "./ui/Modal";

interface Props {
  onConfirm: (name: string, path: string) => Promise<Project | null>;
  onCancel: () => void;
}

function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).pop() || path;
}

/** 添加项目对话框：选一个本地目录作为项目，会话可在其下分组启动。 */
export function AddProjectDialog({ onConfirm, onCancel }: Props) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const nameTouched = useRef(false);
  const operationInFlight = useRef(false);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const pickDir = async () => {
    if (operationInFlight.current) return;
    operationInFlight.current = true;
    setPicking(true);
    setError(null);
    try {
      const selected = await open({ directory: true, multiple: false, title: "选择项目目录" });
      if (typeof selected === "string") {
        setPath(selected);
        if (!nameTouched.current) setName(baseName(selected));
      }
    } catch (reason) { setError(`选择目录失败：${String(reason)}`); }
    finally { operationInFlight.current = false; setPicking(false); }
  };

  const submit = async () => {
    if (!path || operationInFlight.current) return;
    operationInFlight.current = true;
    setSubmitting(true);
    setError(null);
    // 失败时 onConfirm 返回 null（错误已由调用方弹出提示）。
    try {
      const created = await onConfirm(name.trim(), path);
      if (created) onCancel();
    } catch (reason) { setError(`添加项目失败：${String(reason)}`); }
    finally { operationInFlight.current = false; setSubmitting(false); }
  };

  return (
    <Modal
      overlayClassName="editor-overlay"
      className="editor"
      onClose={() => { if (!operationInFlight.current) onCancel(); }}
      closeOnOverlayClick
      style={{ width: 440 }}
    >
      <h2>添加项目</h2>
        <p className="hint" style={{ margin: "0 0 12px" }}>
          项目是一个本地目录；添加后可在项目下分组启动多个会话。
        </p>
        <div className="field">
          <label>项目目录</label>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn" type="button" disabled={picking || submitting} onClick={pickDir}>
              <FolderIcon size={15} style={{ marginRight: 6, verticalAlign: -2 }} />
              选择目录
            </button>
            <div
              style={{
                flex: 1,
                padding: "8px 10px",
                fontSize: 12,
                color: path ? "var(--text)" : "var(--text-dim)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                alignSelf: "center",
              }}
              title={path}
            >
              {path || "未选择"}
            </div>
          </div>
        </div>
        <div className="field">
          <label>项目名称</label>
          <input
            value={name}
            disabled={submitting}
            placeholder="默认取目录名"
            onChange={(e) => {
              setName(e.target.value);
              nameTouched.current = true;
            }}
          />
        </div>
        {error && <p role="alert" className="error-text">{error}</p>}
        <div className="actions">
          <button className="btn" disabled={picking || submitting} onClick={onCancel}>
            取消
          </button>
          <button
            className="btn primary"
            disabled={!path || picking || submitting}
            onClick={() => void submit()}
          >
            {submitting ? "添加中…" : "添加项目"}
          </button>
        </div>
    </Modal>
  );
}

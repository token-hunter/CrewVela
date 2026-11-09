import { useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Modal } from "./ui/Modal";

interface Props {
  count: number;
  /** 项目下新建会话时传入：目录锁定为项目路径，不可更改。 */
  lockedCwd?: string | null;
  onConfirm: (
    cwds: string[],
    options: { useWorktree: boolean; savePresetAs: string | null },
  ) => void;
  onCancel: () => void;
}

export function MultiDirPrompt({ count, lockedCwd = null, onConfirm, onCancel }: Props) {
  const [dirs, setDirs] = useState<string[]>(() =>
    Array(count).fill(lockedCwd ?? ""),
  );
  const [useWorktree, setUseWorktree] = useState(false);
  const [savePreset, setSavePreset] = useState(false);
  const [presetName, setPresetName] = useState("");
  const pickingRef = useRef(false);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pick = async (index: number | null) => {
    if (pickingRef.current) return;
    pickingRef.current = true;
    setPicking(true);
    setError(null);
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === "string") {
        setDirs(dirs => dirs.map((dir, i) => index === null || index === i ? selected : dir));
      }
    } catch (reason) { setError(`选择目录失败：${String(reason)}`); }
    finally { pickingRef.current = false; setPicking(false); }
  };

  const presetNameTrimmed = presetName.trim();

  return (
    <Modal
      overlayClassName="editor-overlay"
      className="editor"
      onClose={onCancel}
      closeOnOverlayClick
      style={{ width: 560 }}
    >
      <h2>
          {lockedCwd
            ? `在项目目录启动 ${count} 个会话`
            : `选择各会话工作目录（共 ${count} 个）`}
        </h2>
        {!lockedCwd && (
          <div className="field">
            <button className="btn" disabled={picking} onClick={() => void pick(null)}>
              全部使用同一目录
            </button>
          </div>
        )}
        <div className="env-list">
          {dirs.map((d, i) => (
            <div className="env-row" key={i} style={{ gridTemplateColumns: "auto 1fr" }}>
              {lockedCwd ? (
                <span
                  style={{
                    padding: "8px 10px",
                    fontSize: 12,
                    color: "var(--text-dim)",
                    alignSelf: "center",
                  }}
                >
                  会话 {i + 1}
                </span>
              ) : (
                <button className="btn" disabled={picking} onClick={() => void pick(i)}>
                  选目录
                </button>
              )}
              <div
                style={{
                  padding: "8px 10px",
                  fontSize: 12,
                  color: d ? "var(--text)" : "var(--text-dim)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={d}
              >
                {d || "home（默认）"}
              </div>
            </div>
          ))}
        </div>
        <div className="field">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={useWorktree}
              onChange={(e) => setUseWorktree(e.target.checked)}
            />
            为每个会话创建独立 git worktree（基于所选仓库的当前 HEAD，并行改动互不影响）
          </label>
        </div>
        {!lockedCwd && (
          <div className="field">
            <label className="checkbox">
              <input
                type="checkbox"
                checked={savePreset}
                onChange={(e) => setSavePreset(e.target.checked)}
              />
              同时保存为预设（新建菜单可一键拉起这组会话）
            </label>
            {savePreset && (
              <input
                className="preset-name-input"
                value={presetName}
                maxLength={40}
                placeholder="预设名称，如：前端三开调试"
                autoFocus
                onChange={(e) => setPresetName(e.target.value)}
              />
            )}
          </div>
        )}
        {error && <p role="alert" className="error-text">{error}</p>}
        <div className="actions">
          <button className="btn" onClick={onCancel}>
            取消
          </button>
          <button
            className="btn primary"
            disabled={picking || (savePreset && !presetNameTrimmed)}
            onClick={() =>
              onConfirm(dirs, {
                useWorktree,
                savePresetAs: savePreset && presetNameTrimmed ? presetNameTrimmed : null,
              })
            }
          >
            开始 {count} 个
          </button>
        </div>
    </Modal>
  );
}

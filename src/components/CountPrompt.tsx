import { useState } from "react";
import { Modal } from "./ui/Modal";

interface Props {
  providerName: string;
  /** 项目下新建时为 true：目录已锁定，下一步是确认而非选目录。 */
  dirLocked?: boolean;
  onConfirm: (
    count: number,
    options: { autoMode: boolean; yolo: boolean; initialPrompt: string },
  ) => void;
  onCancel: () => void;
}

export function CountPrompt({ providerName, dirLocked = false, onConfirm, onCancel }: Props) {
  const [val, setVal] = useState("4");
  const [yolo, setYolo] = useState(false);
  const [autoMode, setAutoMode] = useState(false);
  const [initialPrompt, setInitialPrompt] = useState("");
  const n = Number(val);
  const valid = Number.isInteger(n) && n >= 1 && n <= 9;
  return (
    <Modal
      overlayClassName="editor-overlay"
      className="editor"
      onClose={onCancel}
      closeOnOverlayClick
      style={{ width: 380 }}
    >
      <h2>多开 {providerName}</h2>
        <div className="field">
          <label>数量（1-9）</label>
          <input
            type="number"
            min={1}
            max={9}
            value={val}
            autoFocus
            onChange={(e) => setVal(e.target.value)}
          />
          {!valid && (
            <div className="hint" style={{ color: "var(--danger)" }}>
              请输入 1-9 的整数
            </div>
          )}
        </div>
        <div className="field">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={autoMode}
              onChange={(e) => {
                const checked = e.target.checked;
                setAutoMode(checked);
                if (checked) setYolo(false);
              }}
            />
            Auto 模式（--permission-mode auto，自动处理权限请求）
          </label>
        </div>
        <div className="field">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={yolo}
              onChange={(e) => {
                const checked = e.target.checked;
                setYolo(checked);
                if (checked) setAutoMode(false);
              }}
            />
            YOLO 模式（--dangerously-skip-permissions，跳过所有权限确认）
          </label>
        </div>
        <div className="field">
          <label>初始任务（可选，会话就绪后自动发送）</label>
          <textarea
            value={initialPrompt}
            rows={3}
            placeholder="例如：阅读这个仓库，修复登录接口的超时问题"
            onChange={(e) => setInitialPrompt(e.target.value)}
          />
        </div>
        <div className="actions">
          <button className="btn" onClick={onCancel}>
            取消
          </button>
          <button
            className="btn primary"
            disabled={!valid}
            onClick={() => onConfirm(n, { autoMode, yolo, initialPrompt })}
          >
            {dirLocked ? "下一步：确认" : "下一步：选目录"}
          </button>
        </div>
    </Modal>
  );
}

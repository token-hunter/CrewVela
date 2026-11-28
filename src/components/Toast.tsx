import { useEffect, useState, type ReactNode } from "react";
import { localizeError } from "../localizedError";
import { translate } from "../interfaceLanguage";
import { useInterfaceLanguage } from "../useInterfaceLanguage";

export interface ToastItem {
  id: number;
  message: string;
}

const AUTO_DISMISS_MS = 5000;
const EXIT_MS = 140;

function ToastCard({
  toast,
  onDismiss,
}: {
  toast: ToastItem;
  onDismiss: (id: number) => void;
}) {
  const [leaving, setLeaving] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const language = useInterfaceLanguage();
  const { message, details } = localizeError(language, toast.message);

  useEffect(() => {
    if (expanded) return;
    const timer = window.setTimeout(() => setLeaving(true), AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, [expanded]);

  useEffect(() => {
    if (!leaving) return;
    const timer = window.setTimeout(() => onDismiss(toast.id), EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [leaving, onDismiss, toast.id]);

  return (
    <div className={`toast${leaving ? " leaving" : ""}`} role="alert">
      <div className="toast-message">
        {message}
        {details && <details onToggle={event => setExpanded(event.currentTarget.open)}>
          <summary>{translate(language, "错误详情")}</summary>
          <div>{details}</div>
        </details>}
      </div>
      <button aria-label={translate(language, "关闭通知")} onClick={() => setLeaving(true)}>
        ×
      </button>
    </div>
  );
}

export function ToastStack({
  toasts,
  onDismiss,
  children,
}: {
  toasts: ToastItem[];
  onDismiss: (id: number) => void;
  children?: ReactNode;
}) {
  if (toasts.length === 0 && !children) return null;
  return (
    <div className="toast-stack">
      {toasts.map((toast) => (
        <ToastCard key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
      {children}
    </div>
  );
}

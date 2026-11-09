import { useEffect, useState } from "react";

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

  useEffect(() => {
    const timer = window.setTimeout(() => setLeaving(true), AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!leaving) return;
    const timer = window.setTimeout(() => onDismiss(toast.id), EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [leaving, onDismiss, toast.id]);

  return (
    <div className={`toast${leaving ? " leaving" : ""}`} role="alert">
      <span>{toast.message}</span>
      <button aria-label="关闭通知" onClick={() => setLeaving(true)}>
        ×
      </button>
    </div>
  );
}

export function ToastStack({
  toasts,
  onDismiss,
}: {
  toasts: ToastItem[];
  onDismiss: (id: number) => void;
}) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-stack">
      {toasts.map((toast) => (
        <ToastCard key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
    </div>
  );
}

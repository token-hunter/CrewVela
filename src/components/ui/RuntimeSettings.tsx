import type { ReactNode } from "react";
import { Switch } from "./Switch";

export function SettingsGroup({ title, description, children }: {
  title: string; description?: string; children: ReactNode;
}) {
  return <section className="runtime-group">
    <header><h3>{title}</h3>{description && <p>{description}</p>}</header>
    <div className="runtime-group-body">{children}</div>
  </section>;
}

export function SettingSwitch({ title, hint, checked, disabled, onChange, icon }: {
  title: string; hint: string; checked: boolean; disabled?: boolean;
  onChange: (checked: boolean) => void; icon?: ReactNode;
}) {
  return <Switch className="browser-control-toggle" checked={checked} disabled={disabled} onChange={onChange} childrenPosition="before">
    {icon}<span><strong>{title}</strong><small>{hint}</small></span>
  </Switch>;
}

export function SettingsActions({ ready, loading, busy, dirty, invalid, message, hint, onSave, onRetry }: {
  ready: boolean; loading: boolean; busy: boolean; dirty: boolean; invalid?: boolean;
  message: string | null; hint?: ReactNode; onSave: () => void; onRetry: () => void;
}) {
  return <footer className="runtime-actions">
    <div aria-live="polite">{message || hint}</div>
    {ready
      ? <button type="button" className="btn primary" disabled={busy || !dirty || invalid} onClick={onSave}>{busy ? "正在应用…" : "保存并应用"}</button>
      : <button type="button" className="btn secondary" disabled={loading} onClick={onRetry}>{loading ? "正在读取设置…" : "重新读取设置"}</button>}
  </footer>;
}

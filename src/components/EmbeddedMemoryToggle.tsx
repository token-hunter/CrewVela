import { useTranslation } from "../useInterfaceLanguage";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "./ui/Switch";

interface Status {
  available: boolean;
  enabled: boolean;
  running: boolean;
}

export function useEmbeddedMemory(preview: boolean) {
  const [status, setStatus] = useState<Status | null>(preview ? { available: true, enabled: false, running: false } : null);
  const [busy, setBusy] = useState(false);
  const [operationError, setOperationError] = useState("");
  const [statusError, setStatusError] = useState("");
  const [checking, setChecking] = useState(!preview);
  const [refreshVersion, setRefreshVersion] = useState(0);
  const operation = useRef(false);

  useEffect(() => {
    if (preview || busy) return;
    let disposed = false;
    let requesting = false;
    const refresh = async () => {
      if (requesting) return;
      requesting = true;
      try {
        const next = await invoke<Status>("ai_memory_embedded_status");
        if (!disposed && !operation.current) { setStatus(next); setStatusError(""); }
      } catch (reason) {
        if (!disposed && !operation.current) setStatusError(`读取内置记忆状态失败。${String(reason)}`);
      } finally {
        requesting = false;
        if (!disposed) setChecking(false);
      }
    };
    void refresh();
    const interval = window.setInterval(() => void refresh(), 15_000);
    return () => { disposed = true; window.clearInterval(interval); };
  }, [preview, busy, refreshVersion]);

  const refresh = () => {
    if (preview || operation.current) return;
    setChecking(true);
    setRefreshVersion(value => value + 1);
  };

  const setEnabled = async (value: boolean) => {
    if (operation.current) return;
    operation.current = true;
    setBusy(true);
    setOperationError("");
    try {
      const next = preview
        ? { available: true, enabled: value, running: value }
        : await invoke<Status>("ai_memory_embedded_set_enabled", { value });
      setStatus(next);
      setStatusError("");
    } catch (reason) {
      setOperationError(String(reason));
    } finally {
      operation.current = false;
      setBusy(false);
    }
  };

  const statusText = busy ? "正在处理…" : checking ? "正在检测…" : statusError || !status ? "状态未知"
    : status.enabled ? status.running ? "内置服务已就绪" : "已开启，服务尚未就绪"
    : status.available ? "尚未开启" : "当前构建未内置共享记忆";
  return { status, busy, checking, statusText, error: operationError || statusError, statusError, refresh, setEnabled };
}

export function EmbeddedMemoryToggle({ memory }: { memory: ReturnType<typeof useEmbeddedMemory> }) {
  const t = useTranslation();
  const { status, busy, checking, statusText, error, statusError, refresh, setEnabled } = memory;
  const needsRestart = Boolean(status?.available && status.enabled && !status.running);
  const unavailable = status && !status.available && !status.enabled;
  return <section className="sm-card sm-embedded-card" aria-label={t("内置共享记忆")}>
    <div className="sm-embedded-row">
      <div>
        <h3>{t("内置共享记忆")}</h3>
        <p className="sm-muted">记忆保存在本机，已打开的会话将在下次启动或恢复时接入。</p>
      </div>
      <Switch className="pet-switch sm-embedded-switch" label={t("内置共享记忆")} checked={Boolean(status?.enabled)}
        disabled={busy || checking || Boolean(statusError) || !status || Boolean(unavailable)}
        onChange={checked => void setEnabled(checked)} />
    </div>
    {error && <p role="alert" className="sm-error">{error}</p>}
    <p role="status" className={busy || needsRestart || unavailable ? "sm-muted" : "sm-sr-only"}>{statusText}</p>
    {(statusError || needsRestart) && <div className="sm-actions sm-embedded-recovery">
      <button className="sm-button" type="button" disabled={busy || checking}
        onClick={statusError ? refresh : () => void setEnabled(true)}>{statusError ? t("重新检测") : t("重试启动")}</button>
    </div>}
  </section>;
}

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowClockwise";
import { ClockIcon } from "@phosphor-icons/react/dist/csr/Clock";
import type { ProviderConfig } from "../types";
import {
  quotaFailure,
  quotaPercent,
  quotaPreview,
  quotaReset,
  quotaUpdated,
  type ProviderQuota,
} from "../providerQuota";

const PREVIEW =
  !("__TAURI_INTERNALS__" in window) &&
  new URLSearchParams(window.location.search).get("ui-preview") === "1";

export function ProviderQuotaCell({
  provider,
  hasKey,
  active,
}: {
  provider: ProviderConfig;
  hasKey: boolean;
  active: boolean;
}) {
  const [quota, setQuota] = useState<ProviderQuota | null>(null);
  const [busy, setBusy] = useState(false);
  const [clock, setClock] = useState(Date.now());
  const sequence = useRef(0);
  const inFlight = useRef(false);
  const refresh = useCallback(
    async (force = false) => {
      if (inFlight.current) return;
      const request = ++sequence.current;
      inFlight.current = true;
      setBusy(true);
      try {
        const next = PREVIEW
          ? quotaPreview(
              provider.baseUrl,
              new URLSearchParams(window.location.search).get("quota-state"),
            )
          : await invoke<ProviderQuota>("get_provider_quota", {
              id: provider.id,
              force,
            });
        if (request === sequence.current) setQuota(next);
      } catch {
        if (request === sequence.current) setQuota(quotaFailure);
      } finally {
        if (request === sequence.current) {
          inFlight.current = false;
          setBusy(false);
        }
      }
    },
    [provider, hasKey],
  );

  useEffect(() => {
    sequence.current++;
    inFlight.current = false;
    setBusy(false);
    setQuota(null);
    if (!active || provider.hidden) return;
    if (!document.hidden) void refresh();
    const onVisible = () => {
      if (!document.hidden) void refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    const poll = window.setInterval(() => { if (!document.hidden) void refresh(); }, 300_000);
    const tick = window.setInterval(() => setClock(Date.now()), 30_000);
    return () => { sequence.current++; inFlight.current = false; document.removeEventListener("visibilitychange", onVisible); window.clearInterval(poll); window.clearInterval(tick); };
  }, [refresh, active, provider.hidden]);

  return (
    <div
      className="provider-quota"
      aria-label={`${provider.name} 账户额度`}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="provider-quota-meta">
        <small>{quotaUpdated(quota?.updatedAt ?? null, clock)}</small>
        <button
          type="button"
          disabled={busy || provider.hidden || quota?.status === "unsupported"}
          aria-label={`刷新 ${provider.name} 账户额度`}
          title="手动刷新（页面打开时每 5 分钟自动更新）"
          onClick={() => void refresh(true)}
        >
          <ArrowClockwiseIcon
            size={14}
            className={busy ? "is-spinning" : ""}
          />
        </button>
      </div>
      {quota?.status === "error" && (
        <small className="provider-quota-error">
          {quota.message}
          {quota.updatedAt != null ? "（以下为上次成功数据）" : ""}
        </small>
      )}
      {busy && !quota ? (
        <small>查询中…</small>
      ) : quota && (quota.status === "ok" || quota.updatedAt != null) ? (
        <div className="provider-quota-values">
          {quota.windows.map((window, index) => (
            <div
              className="provider-quota-window"
              key={`${window.label}-${index}`}
            >
              <span>
                {window.label}：
                {window.usedPercent != null ? "已用 " : "剩余 "}
                <strong className={window.usedPercent != null && window.usedPercent >= 90 ? "provider-quota-error" : "provider-quota-percent"}>
                  {quotaPercent(window.usedPercent ?? window.remainingPercent)}
                </strong>
              </span>
              {window.resetAt != null && (
                <small className="provider-quota-reset">
                  <ClockIcon size={12} aria-hidden="true" />
                  {quotaReset(window.resetAt, clock)}
                </small>
              )}
            </div>
          ))}
          {quota.balances.map((balance, index) => (
            <span
              className={balance.amount < 0 ? "provider-quota-error" : ""}
              key={index}
            >
              余额：{balance.amount.toFixed(2)} {balance.currency}
            </span>
          ))}
        </div>
      ) : (
        quota?.status !== "error" && (
          <small>
            {provider.hidden ? "已隐藏，暂停查询" : quota?.message ?? "尚未查询"}
          </small>
        )
      )}
    </div>
  );
}

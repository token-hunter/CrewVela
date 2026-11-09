import { useCallback, useEffect, useMemo, useState } from "react";
import { GaugeIcon } from "@phosphor-icons/react/dist/csr/Gauge";
import {
  CODEX_ACCOUNTS_UPDATED_EVENT,
  loadCodexAccountSnapshot,
  type CodexAccountSnapshot,
} from "../codexAccounts";

interface Props {
  onOpen: () => void;
}

const PREVIEW_SNAPSHOT: CodexAccountSnapshot = {
  installed: true,
  warnings: [],
  message: null,
  accounts: [{
    index: 1,
    account: "preview@example.com",
    plan: "Team",
    nextUsedPercent: 24,
    nextReset: "16:18",
    totalUsedPercent: 88,
    totalReset: "08-23",
    current: true,
    localStatus: null,
    refreshError: null,
  }],
};

export function CodexQuotaSummary({ onOpen }: Props) {
  const [snapshot, setSnapshot] = useState<CodexAccountSnapshot | null>(null);
  const [failed, setFailed] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await loadCodexAccountSnapshot());
      setFailed(false);
    } catch {
      if (window.location.search.includes("ui-preview=1")) {
        setSnapshot(PREVIEW_SNAPSHOT);
        setFailed(false);
      } else {
        setFailed(true);
      }
    }
  }, []);

  useEffect(() => {
    void refresh();
    const accountListener = (event: Event) => {
      setSnapshot((event as CustomEvent<CodexAccountSnapshot>).detail);
      setFailed(false);
    };
    window.addEventListener(CODEX_ACCOUNTS_UPDATED_EVENT, accountListener);
    window.addEventListener("focus", refresh);
    return () => {
      window.removeEventListener(CODEX_ACCOUNTS_UPDATED_EVENT, accountListener);
      window.removeEventListener("focus", refresh);
    };
  }, [refresh]);

  const currentAccount = useMemo(
    () => snapshot?.accounts.find((account) => account.current) ?? null,
    [snapshot],
  );
  const fiveHourRemaining = currentAccount?.nextUsedPercent ?? null;
  const weeklyRemaining = currentAccount?.totalUsedPercent ?? null;
  const fiveHourReset = currentAccount?.nextReset ?? null;
  const weeklyReset = currentAccount?.totalReset ?? null;
  const quotaLow = [fiveHourRemaining, weeklyRemaining].some(
    (value) => value !== null && value <= 10,
  );
  const unavailable = failed || !snapshot?.installed || !currentAccount || Boolean(currentAccount.refreshError);
  const quotaLabel = unavailable
    ? "Codex 额度不可用，打开账户管理"
    : `5 小时剩余 ${fiveHourRemaining ?? "未知"}%${fiveHourReset ? `，${fiveHourReset} 重置` : ""}；每周剩余 ${weeklyRemaining ?? "未知"}%${weeklyReset ? `，${weeklyReset} 重置` : ""}`;

  return (
    <button
      type="button"
      className={`codex-quota-summary${unavailable ? " unavailable" : ""}${quotaLow ? " low" : ""}`}
      onClick={onOpen}
      title="打开 Codex 账户与额度"
      aria-label={quotaLabel}
    >
      <GaugeIcon size={15} weight="duotone" aria-hidden="true" />
      <span className="codex-quota-copy">
        {unavailable ? (
          <strong>额度不可用</strong>
        ) : (
          <>
            <span className="codex-quota-window">
              <strong>5 小时 {fiveHourRemaining === null ? "未知" : `${Math.round(fiveHourRemaining)}%`}</strong>
              <small>{fiveHourReset ?? "时间未知"}</small>
            </span>
            <i aria-hidden="true" />
            <span className="codex-quota-window">
              <strong>每周 {weeklyRemaining === null ? "未知" : `${Math.round(weeklyRemaining)}%`}</strong>
              <small>{weeklyReset ?? "时间未知"}</small>
            </span>
          </>
        )}
      </span>
    </button>
  );
}

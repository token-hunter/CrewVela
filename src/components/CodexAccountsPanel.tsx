import { IS_MAC } from "../platform";
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowClockwiseIcon } from "@phosphor-icons/react/dist/csr/ArrowClockwise";
import { CheckCircleIcon } from "@phosphor-icons/react/dist/csr/CheckCircle";
import { EyeIcon } from "@phosphor-icons/react/dist/csr/Eye";
import { EyeSlashIcon } from "@phosphor-icons/react/dist/csr/EyeSlash";
import { PlusIcon } from "@phosphor-icons/react/dist/csr/Plus";
import { SignInIcon } from "@phosphor-icons/react/dist/csr/SignIn";
import { TrashIcon } from "@phosphor-icons/react/dist/csr/Trash";
import { WarningCircleIcon } from "@phosphor-icons/react/dist/csr/WarningCircle";
import {
  loadCodexAccountSnapshot,
  publishCodexAccountSnapshot,
  remainingPercent,
  type CodexAccount,
  type CodexAccountActionReceipt,
  type CodexAccountSnapshot,
} from "../codexAccounts";

const PREVIEW_SNAPSHOT: CodexAccountSnapshot = {
  installed: true,
  message: null,
  warnings: [
    "续期失败：demo2@example.com：刷新接口返回 401",
    "续期失败：demo6@example.com：刷新接口返回 401",
  ],
  accounts: [
    [1, "demo1@example.com", "Team", 0, "08-18", false, null],
    [2, "demo2@example.com", null, null, null, false, "刷新接口返回 401"],
    [3, "demo3@example.com", "Team", 55, "08-18", true, null],
    [4, "demo4@example.com", "Team", 95, "08-18", false, null],
    [5, "demo5@example.com", "Team", 100, "08-19", false, null],
    [6, "demo6@example.com", null, null, null, false, "刷新接口返回 401"],
  ].map(([index, account, plan, used, reset, current, refreshError]) => ({
    index: index as number,
    account: account as string,
    plan: plan as string | null,
    nextUsedPercent: null,
    nextReset: null,
    totalUsedPercent: used as number | null,
    totalReset: reset as string | null,
    current: current as boolean,
    localStatus: null,
    refreshError: refreshError as string | null,
  })),
};

const RESTART_REQUIRED_KEY = "crewvela:codex-restart-required";

function accountState(account: CodexAccount) {
  if (account.refreshError) {
    if (account.refreshError.includes("401")) return { label: "401 登录失效", className: "error" };
    if (account.refreshError.includes("403")) return { label: "403 登录失效", className: "error" };
    return { label: "续期失败", className: "error" };
  }
  if (account.localStatus === "离线" || account.localStatus === "未登录") {
    return { label: account.localStatus, className: "error" };
  }
  if (account.localStatus === "未知") return { label: "状态未知", className: "warning" };
  const remaining = remainingPercent(account);
  if (remaining !== null && remaining <= 0) return { label: "额度用尽", className: "error" };
  if (remaining !== null && remaining <= 10) return { label: "额度紧张", className: "warning" };
  if (account.current) return { label: "当前账户", className: "current" };
  if (remaining === null) return { label: "额度未知", className: "warning" };
  if (remaining >= 100) return { label: "额度充足", className: "" };
  return { label: "可用", className: "" };
}

function privateError(error: unknown) {
  return String(error).replace(/[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/gi, "某个账户");
}

function successMessage(action: string) {
  if (action === "init") return "当前登录已同步。";
  if (action === "add") return "已打开 Codex 登录窗口；登录完成后点击“刷新登录态”。";
  if (action === "token") return "访问令牌已验证，账户已添加。";
  if (action === "auto") return "已完成可用账户选择；重启 Codex 后生效。";
  if (action.startsWith("delete-")) return "账户已删除。";
  return "账户已切换；重启 Codex 后生效。";
}

export function CodexAccountsPanel() {
  const [snapshot, setSnapshot] = useState<CodexAccountSnapshot | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [personalAccessToken, setPersonalAccessToken] = useState("");
  const [revealedAccounts, setRevealedAccounts] = useState<Set<number>>(() => new Set());
  const [message, setMessage] = useState<{ kind: "error" | "success"; text: string } | null>(null);
  const [autoSelectionFeedback, setAutoSelectionFeedback] = useState<{
    kind: "error" | "success";
    text: string;
  } | null>(null);
  const [restartState] = useState(() => {
    try {
      return { required: window.localStorage.getItem(RESTART_REQUIRED_KEY) === "1", error: null as string | null };
    } catch (error) {
      return { required: false, error: `读取重启提醒失败：${privateError(error)}` };
    }
  });
  const [restartRequired, setRestartRequired] = useState(restartState.required);
  const [storageError, setStorageError] = useState(restartState.error);

  const updateRestartRequired = (required: boolean) => {
    setRestartRequired(required);
    try {
      if (required) window.localStorage.setItem(RESTART_REQUIRED_KEY, "1");
      else window.localStorage.removeItem(RESTART_REQUIRED_KEY);
      setStorageError(null);
    } catch (error) {
      setStorageError(`保存重启提醒失败，重新打开此页后请自行确认是否需要重启 Codex：${privateError(error)}`);
    }
  };

  const refresh = async () => {
    setBusyAction("refresh");
    setMessage(null);
    try {
      const nextSnapshot = await loadCodexAccountSnapshot();
      setSnapshot(nextSnapshot);
      publishCodexAccountSnapshot(nextSnapshot);
      setRevealedAccounts(new Set());
    } catch (error) {
      if (window.location.search.includes("ui-preview=1")) {
        setSnapshot(PREVIEW_SNAPSHOT);
        publishCodexAccountSnapshot(PREVIEW_SNAPSHOT);
        setRevealedAccounts(new Set());
      }
      else setMessage({ kind: "error", text: privateError(error) });
    } finally {
      setBusyAction(null);
    }
  };

  useEffect(() => { void refresh(); }, []);

  const run = async (action: string, command: string, args?: Record<string, unknown>, requiresRestart = false) => {
    const beforeCurrentAccount = snapshot?.accounts.find((account) => account.current)?.account ?? null;
    setBusyAction(action);
    setMessage(null);
    if (action !== "auto") setAutoSelectionFeedback(null);
    try {
      const receipt = await invoke<CodexAccountActionReceipt>(command, args);
      const afterCurrentAccount = receipt.snapshot.accounts.find((account) => account.current)?.account ?? null;
      const accountChanged = beforeCurrentAccount !== afterCurrentAccount && afterCurrentAccount !== null;
      setSnapshot(receipt.snapshot);
      publishCodexAccountSnapshot(receipt.snapshot);
      setRevealedAccounts(new Set());
      if (action === "auto") {
        const text = accountChanged
          ? "已自动切换到可用账户；重启 Codex 后生效。"
          : receipt.message || "当前账户仍有剩余额度，无需切换。";
        setAutoSelectionFeedback({ kind: "success", text });
      } else {
        setMessage({ kind: "success", text: successMessage(action) });
      }
      if (requiresRestart && (action !== "auto" || accountChanged)) {
        updateRestartRequired(true);
      }
      if (action === "token") {
        setPersonalAccessToken("");
        setShowAdd(false);
      }
    } catch (error) {
      const text = privateError(error);
      if (action === "auto") setAutoSelectionFeedback({ kind: "error", text });
      else setMessage({ kind: "error", text });
    } finally {
      setBusyAction(null);
    }
  };

  const restartCodex = async () => {
    if (!window.confirm("将关闭当前所有 Codex App 窗口并重新打开。请先确认 Codex 中没有尚未保存的输入。是否继续？")) return;
    setBusyAction("restart-codex");
    setMessage(null);
    try {
      const result = await invoke<string>("restart_codex_app");
      updateRestartRequired(false);
      setMessage({ kind: "success", text: result });
    } catch (error) {
      setMessage({ kind: "error", text: privateError(error) });
    } finally {
      setBusyAction(null);
    }
  };

  const usable = snapshot?.accounts.filter((account) => {
    const remaining = remainingPercent(account);
    return !account.refreshError && remaining !== null && remaining > 0;
  }) ?? [];
  const failed = snapshot?.accounts.filter((account) => Boolean(account.refreshError)).length ?? 0;
  const remaining = useMemo(() => {
    if (!usable.length) return null;
    return usable.reduce((sum, account) => sum + (remainingPercent(account) ?? 0), 0) / usable.length;
  }, [usable]);

  return (
    <div className="cid-accounts-panel">
      <header className="cid-toolbar">
        <div>
          <h3>Codex 账户</h3>
          <p>管理本机 Codex 登录账户与额度。{IS_MAC && "切换后请手动重启 Codex App。"}</p>
        </div>
        <div>
          <button className={`btn secondary${restartRequired ? " cid-restart-required" : ""}`} disabled={IS_MAC || busyAction !== null} title={IS_MAC ? "macOS 请手动重启 Codex App" : undefined} onClick={() => void restartCodex()}>
            <ArrowClockwiseIcon size={15} className={busyAction === "restart-codex" ? "spinning" : ""} />重启 Codex
          </button>
          <button className="btn secondary" disabled={busyAction !== null} onClick={() => void refresh()}>
            <ArrowClockwiseIcon size={15} className={busyAction === "refresh" ? "spinning" : ""} />刷新登录态
          </button>
          <button className="btn secondary" disabled={busyAction !== null} onClick={() => void run("init", "cid_init_accounts")}>
            <CheckCircleIcon size={15} />同步当前登录
          </button>
          <button className="btn primary" disabled={busyAction !== null} aria-expanded={showAdd} onClick={() => setShowAdd((value) => !value)}>
            <PlusIcon size={15} />添加账户
          </button>
        </div>
      </header>

      {storageError && <div className="cid-warning" role="alert"><WarningCircleIcon size={16} />{storageError}</div>}
      {showAdd && (
        <section className="cid-add-panel">
          <div>
            <strong>Codex 登录</strong>
            <small>{IS_MAC ? "macOS 请在终端执行 cid add，完成后返回此页刷新登录态。" : "打开独立登录窗口，完成后返回此页刷新登录态。"}</small>
            <button className="btn secondary" disabled={IS_MAC || busyAction !== null} onClick={() => void run("add", "cid_add_account")}>
              <SignInIcon size={14} />打开登录窗口
            </button>
          </div>
          <div>
            <strong>访问令牌</strong>
            <small>沿用 cid 的令牌验证和账户覆盖规则。</small>
            <span className="cid-token-entry">
              <input type="password" value={personalAccessToken} placeholder="输入 personal_access_token" autoComplete="off" onChange={(event) => setPersonalAccessToken(event.target.value)} />
              <button className="btn secondary" disabled={busyAction !== null || personalAccessToken.trim().length < 16} onClick={() => void run("token", "cid_add_token", { token: personalAccessToken.trim() })}>验证并添加</button>
            </span>
          </div>
        </section>
      )}

      {snapshot && !snapshot.installed ? (
        <section className="cid-empty-state"><strong>未找到 cid</strong><span>{snapshot.message}</span><code>npm install -g codex-id</code></section>
      ) : (
        <>
          <section className="cid-summary" aria-label="账户概览">
            <div><span>账户总数</span><strong>{snapshot?.accounts.length ?? 0}</strong></div>
            <div><span>平均剩余额度</span><strong>{remaining === null ? "—" : `${remaining.toFixed(1)}%`}</strong></div>
            <div className={failed ? "attention" : ""}><span>需要重新登录</span><strong>{failed}</strong></div>
          </section>

          {failed > 0 && <div className="cid-warning"><WarningCircleIcon size={16} />{failed} 个账户续期失败，需要重新登录。</div>}
          {message && (
            <div className={`cid-message ${message.kind}`}>
              <span>{message.text}</span>
              {restartRequired && <button className="btn ghost" disabled={IS_MAC || busyAction !== null} title={IS_MAC ? "macOS 请手动重启 Codex App" : undefined} onClick={() => void restartCodex()}>立即重启</button>}
            </div>
          )}
          {!message && restartRequired && (
            <div className="cid-message success">
              <span>账户已切换；重启 Codex 后生效。</span>
              <button className="btn ghost" disabled={IS_MAC || busyAction !== null} title={IS_MAC ? "macOS 请手动重启 Codex App" : undefined} onClick={() => void restartCodex()}>立即重启</button>
            </div>
          )}

          <section className="cid-account-list">
            <div className="cid-account-head"><span>账户</span><span>套餐</span><span>5 小时额度</span><span>每周额度</span><span>状态</span><span>操作</span></div>
            {snapshot?.accounts.map((account) => {
              const remaining = remainingPercent(account);
              const fiveHourRemaining = account.nextUsedPercent;
              const weeklyRemaining = account.totalUsedPercent;
              const state = accountState(account);
              const isBusy = busyAction === account.account;
              const revealed = revealedAccounts.has(account.index);
              const privateLabel = `账户 ${account.index}`;
              return (
                <article key={account.account} className={`cid-account-row${account.current ? " current" : ""}`}>
                  <div className="cid-account-identity">
                    <button
                      type="button"
                      className="cid-account-reveal"
                      aria-label={`${revealed ? "隐藏" : "显示"}${privateLabel}信息`}
                      aria-pressed={revealed}
                      onClick={() => setRevealedAccounts((current) => {
                        const next = new Set(current);
                        if (next.has(account.index)) next.delete(account.index);
                        else next.add(account.index);
                        return next;
                      })}
                    >
                      {revealed ? <EyeSlashIcon size={13} /> : <EyeIcon size={13} />}
                      <strong>{revealed ? account.account : privateLabel}</strong>
                    </button>
                    {account.current && <small>当前</small>}
                  </div>
                  <span>{account.plan ?? "—"}</span>
                  <div className="cid-quota-cell">
                    <span>{fiveHourRemaining === null ? "—" : `剩余 ${fiveHourRemaining.toFixed(0)}%`}</span>
                    <i><b style={{ width: `${Math.min(100, Math.max(0, fiveHourRemaining ?? 0))}%` }} /></i>
                    <small>{account.nextReset ? `${account.nextReset} 重置` : "重置时间未知"}</small>
                  </div>
                  <div className="cid-quota-cell">
                    <span>{weeklyRemaining === null ? "—" : `剩余 ${weeklyRemaining.toFixed(0)}%`}</span>
                    <i><b style={{ width: `${Math.min(100, Math.max(0, weeklyRemaining ?? 0))}%` }} /></i>
                    <small>{account.totalReset ? `${account.totalReset} 重置` : "重置时间未知"}</small>
                  </div>
                  <span className={`cid-status ${state.className}`}>{state.label}</span>
                  <div className="cid-row-actions">
                    {account.refreshError ? (
                      <button className="btn ghost" disabled={busyAction !== null} onClick={() => void run(account.account, "cid_add_account")}><SignInIcon size={14} />重新登录</button>
                    ) : (
                      <button className="btn ghost" disabled={busyAction !== null || account.current} onClick={() => void run(account.account, "cid_use_account", { selector: account.account }, true)}>{isBusy ? "切换中…" : "切换"}</button>
                    )}
                    <button
                      className="cid-delete"
                      aria-label={`删除${privateLabel}`}
                      disabled={busyAction !== null}
                      onClick={() => {
                        if (window.confirm(`确认从 cid 账户库删除 ${revealed ? account.account : privateLabel}？`)) void run(`delete-${account.account}`, "cid_remove_account", { selector: account.account });
                      }}
                    ><TrashIcon size={14} /></button>
                  </div>
                </article>
              );
            })}
            {!snapshot && <div className="cid-loading">正在读取 cid 账户…</div>}
          </section>

          <section className="cid-auto-switch">
            <div>
              <strong>自动选择可用账户</strong>
              <small>仅当当前账户额度用尽或不可用时切换；候选账户优先选择重置时间最近的账户，时间相同时选择剩余额度更高的账户。</small>
              {busyAction === "auto" && (
                <span className="cid-auto-selection-feedback" role="status">正在检查账户额度…</span>
              )}
              {busyAction !== "auto" && autoSelectionFeedback && (
                <span className={`cid-auto-selection-feedback ${autoSelectionFeedback.kind}`} role="status">
                  {autoSelectionFeedback.text}
                </span>
              )}
            </div>
            <button className="btn primary" disabled={busyAction !== null} onClick={() => void run("auto", "cid_use_account", { selector: null }, true)}>{busyAction === "auto" ? "选择中…" : "立即自动选择"}</button>
          </section>
        </>
      )}
    </div>
  );
}

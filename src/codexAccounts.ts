import { invoke } from "@tauri-apps/api/core";

export interface CodexAccount {
  index: number;
  account: string;
  plan: string | null;
  nextUsedPercent: number | null;
  nextReset: string | null;
  totalUsedPercent: number | null;
  totalReset: string | null;
  current: boolean;
  localStatus: string | null;
  refreshError: string | null;
}

export interface CodexAccountSnapshot {
  installed: boolean;
  accounts: CodexAccount[];
  warnings: string[];
  message: string | null;
}

export interface CodexAccountActionReceipt {
  message: string;
  snapshot: CodexAccountSnapshot;
}

export const CODEX_ACCOUNTS_UPDATED_EVENT = "crewvela:codex-accounts-updated";

export function remainingPercent(account: CodexAccount): number | null {
  const windows = [account.totalUsedPercent, account.nextUsedPercent]
    .filter((value): value is number => value !== null);
  if (windows.some(value => !Number.isFinite(value) || value < 0 || value > 100)) return null;
  return windows.length ? Math.min(...windows) : null;
}

export function resetAt(account: CodexAccount): string {
  return account.totalReset ?? account.nextReset ?? "—";
}

export async function loadCodexAccountSnapshot(): Promise<CodexAccountSnapshot> {
  return invoke<CodexAccountSnapshot>("cid_list_accounts");
}

export function publishCodexAccountSnapshot(snapshot: CodexAccountSnapshot): void {
  window.dispatchEvent(
    new CustomEvent<CodexAccountSnapshot>(CODEX_ACCOUNTS_UPDATED_EVENT, {
      detail: snapshot,
    }),
  );
}

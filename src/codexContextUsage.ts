export interface CodexContextUsage { used: number; size: number }

export function contextUsageDetails(usage: CodexContextUsage | null | undefined) {
  if (!usage || !Number.isSafeInteger(usage.used) || !Number.isSafeInteger(usage.size) || usage.used < 0 || usage.size <= 0) return null;
  const percent = Math.round((usage.used / usage.size) * 100);
  return { ...usage, percent, ringPercent: Math.min(100, percent), remainingPercent: Math.max(0, 100 - percent) };
}

export function formatContextTokens(value: number): string {
  return value >= 1000 ? `${(value / 1000).toLocaleString("en-US", { maximumFractionDigits: 1 })}k` : value.toLocaleString("en-US");
}

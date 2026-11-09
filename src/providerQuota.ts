export interface QuotaWindow {
  label: string;
  usedPercent: number | null;
  remainingPercent: number | null;
  resetAt: number | null;
}

export interface ProviderQuota {
  status: "ok" | "unconfigured" | "unsupported" | "error";
  message: string | null;
  updatedAt: number | null;
  windows: QuotaWindow[];
  balances: { amount: number; currency: string }[];
}

export function quotaPercent(value: number | null): string {
  return value != null && Number.isFinite(value) ? `${Math.round(value * 10) / 10}%` : "—";
}

export function quotaFailure(previous: ProviderQuota | null): ProviderQuota {
  return { status: "error", message: "额度查询失败，请重试", updatedAt: previous?.updatedAt ?? null, windows: previous?.windows ?? [], balances: previous?.balances ?? [] };
}

export function quotaReset(resetAt: number | null, now = Date.now()): string {
  if (resetAt == null || !Number.isFinite(resetAt)) return "";
  const minutes = Math.ceil((resetAt - now) / 60_000);
  if (minutes <= 0) return "待重置";
  if (minutes < 60) return `${minutes} 分钟后重置`;
  const hours = Math.floor(minutes / 60);
  return hours >= 24 ? `${Math.floor(hours / 24)} 天 ${hours % 24} 小时后重置` : `${hours} 小时 ${minutes % 60} 分钟后重置`;
}

export function quotaUpdated(updatedAt: number | null, now = Date.now()): string {
  if (updatedAt == null) return "尚未查询";
  const minutes = Math.max(0, Math.floor((now - updatedAt) / 60_000));
  return minutes === 0 ? "刚刚更新" : `${minutes} 分钟前更新`;
}

export function quotaPreview(baseUrl: string, state: string | null = null): ProviderQuota {
  const now = Date.now();
  const result: ProviderQuota = { status: "ok", message: null, updatedAt: now - 180_000, windows: [], balances: [] };
  if (baseUrl.includes("deepseek.com")) result.balances = [{ amount: -0.10, currency: "CNY" }];
  else if (/kimi|bigmodel|minimaxi/.test(baseUrl)) result.windows = [
    { label: "5 小时", usedPercent: 2, remainingPercent: 98, resetAt: now + 480_000 },
    { label: "7 天", usedPercent: 58, remainingPercent: 42, resetAt: now + 180_000_000 },
  ];
  else return { ...result, status: "unsupported", message: "此供应商暂不支持账户额度查询", updatedAt: null };
  if (state === "error") return { ...result, status: "error", message: "额度服务连接失败" };
  if (state === "unconfigured") return { ...result, status: "unconfigured", message: "请先配置模型 API Key", updatedAt: null, windows: [], balances: [] };
  return result;
}

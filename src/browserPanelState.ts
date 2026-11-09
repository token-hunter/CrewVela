export interface BrowserPanelState {
  url: string;
}

const STORAGE_KEY = "crewvela-browser-panel-state-v1";

export const DEFAULT_SEARCH_BASE = "https://www.bing.com/search?q=";

/**
 * 把地址栏输入归一成可导航 URL：
 * 完整 http(s) 直通；localhost/127.0.0.1 补 http://；形似域名补 https://；
 * 其余（含空格、中文等）一律视为搜索词。
 */
export function normalizeAddressInput(
  input: string,
  searchBase: string = DEFAULT_SEARCH_BASE,
): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  if (/^(localhost|127\.0\.0\.1)(:\d+)?([/?#].*)?$/i.test(trimmed)) {
    return `http://${trimmed}`;
  }
  if (!/\s/.test(trimmed) && /^[a-z0-9-]+(\.[a-z0-9-]+)+([/:?#].*)?$/i.test(trimmed)) {
    return `https://${trimmed}`;
  }
  return `${searchBase}${encodeURIComponent(trimmed)}`;
}

export function loadBrowserPanelState(): BrowserPanelState {
  try {
    const saved = JSON.parse(
      localStorage.getItem(STORAGE_KEY) ?? "null",
    ) as Partial<BrowserPanelState> | null;
    return { url: typeof saved?.url === "string" ? saved.url : "" };
  } catch {
    return { url: "" };
  }
}

export function saveBrowserPanelState(state: BrowserPanelState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // 隐私模式等存储异常不阻塞浏览。
  }
}

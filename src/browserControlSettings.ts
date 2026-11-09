/** 浏览器控制设置：与 src-tauri/src/browser_control.rs 的
 * BrowserControlSettings/BrowserControlStatus 保持 serde camelCase 同构。 */

export type BrowserChannelId = "auto" | "chrome" | "msedge" | "custom";

export interface BrowserControlSettings {
  enabled: boolean;
  browser: BrowserChannelId;
  customPath: string;
  headless: boolean;
  preferPanel: boolean;
}

export interface BrowserControlStatus {
  settings: BrowserControlSettings;
  resolvedBrowser: string | null;
  engine: string;
  /** 内置面板接管状态（null = 未启用面板优先 / 平台不支持）。 */
  panel: string | null;
  warning: string | null;
}

export const BROWSER_CHANNEL_OPTIONS: Array<{
  id: BrowserChannelId;
  label: string;
  hint: string;
}> = [
  { id: "auto", label: "自动探测", hint: "优先 Chrome，其次 Edge" },
  { id: "chrome", label: "Chrome", hint: "使用系统安装的 Chrome" },
  { id: "msedge", label: "Edge", hint: "使用系统安装的 Edge" },
  { id: "custom", label: "自定义路径", hint: "指定浏览器可执行文件的绝对路径" },
];

export const DEFAULT_BROWSER_CONTROL_SETTINGS: BrowserControlSettings = {
  enabled: false,
  browser: "auto",
  customPath: "",
  headless: false,
  preferPanel: true,
};

/** 保存前的本地校验；返回中文错误文案或 null（后端仍会做存在性校验）。 */
export function validateBrowserControlSettings(
  settings: BrowserControlSettings,
): string | null {
  if (settings.browser === "custom") {
    const path = settings.customPath.trim();
    if (!path) return "选择自定义浏览器时必须填写可执行文件路径";
    if (!/^[A-Za-z]:[\\/]/.test(path) && !path.startsWith("/") && !path.startsWith("\\\\")) {
      return "自定义浏览器路径必须是绝对路径";
    }
  }
  return null;
}

/** 覆盖范围说明（面板固定文案，单独导出便于单测与未来扩展）。 */
export const BROWSER_CONTROL_COVERAGE =
  "开启后，原生聊天（Claude/Codex/Kimi/DSH）、多智能体与 Claude/Codex 终端会话中的 Agent 都可以驱动浏览器；Kimi 终端会话暂不支持。";

/** 面板优先模式说明。 */
export const BROWSER_CONTROL_PANEL_HINT =
  "Agent 实际调用浏览器工具时自动打开并接管右侧栏面板（操作实时可见）；普通聊天不会打开。面板有独立的持久登录态，无法连接时使用外部浏览器。";

/** UI 预览（?ui-preview=1）假数据。 */
export const BROWSER_CONTROL_PREVIEW: BrowserControlStatus = {
  settings: {
    enabled: true,
    browser: "auto",
    customPath: "",
    headless: false,
    preferPanel: true,
  },
  resolvedBrowser: "Chrome（C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe）",
  engine: "本地依赖 @playwright/mcp（…\\node_modules\\@playwright\\mcp\\cli.js）",
  panel: "内置面板已就绪（CDP 端口 52341，Agent 将接管面板）",
  warning: null,
};

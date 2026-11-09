/** 通用设置：与 src-tauri/src/general_settings.rs 的
 * GeneralSettings/GeneralSettingsStatus 保持 serde camelCase 同构。 */

export interface GeneralSettings {
  httpProxy: string;
  httpNoProxy: string;
  httpCaCertPath: string;
  closeToTray: boolean;
  keepAwake: boolean;
  terminalFontFamily: string;
  terminalFontSize: number;
  webview2HardwareAcceleration: boolean;
}

export interface GeneralSettingsStatus {
  settings: GeneralSettings;
  proxyEnvActive: boolean;
  keepAwakeActive: boolean | null;
  warning: string | null;
}

export const DEFAULT_GENERAL_SETTINGS: GeneralSettings = {
  httpProxy: "",
  httpNoProxy: "",
  httpCaCertPath: "",
  closeToTray: true,
  keepAwake: true,
  terminalFontFamily: "",
  terminalFontSize: 12,
  webview2HardwareAcceleration: true,
};

export const MIN_TERMINAL_FONT_SIZE = 8;
export const MAX_TERMINAL_FONT_SIZE = 28;

/** 面板固定文案（单独导出便于单测）。 */
export const GENERAL_PROXY_COVERAGE =
  "配置后，模型请求、额度监控与后续启动的 Agent 会话（ACP/终端/MCP 子进程）统一走该代理；留空则沿用系统环境变量。本机回环（localhost/127.0.0.1）永远直连。";

export const GENERAL_PROXY_RESTART_HINT =
  "新启动的会话立即继承；已运行会话与常驻监控重启应用后完全生效。";

export const GENERAL_KEEP_AWAKE_DESCRIPTION =
  "有 Agent 会话运行、多智能体控制面存活或存在已启用的自动化任务时，阻止系统空闲休眠（不影响手动睡眠/合盖）。";

export const GENERAL_TRAY_DESCRIPTION =
  "点击关闭按钮时隐藏窗口到系统托盘，Agent 会话继续运行；左键托盘图标唤回，右键菜单可真正退出。";

export const GENERAL_HW_ACCEL_DESCRIPTION =
  "遇到白屏、花屏或渲染异常时关闭；关闭后重启应用生效。";

/** 校验并规范化；返回错误文案数组（空 = 通过）。
 * 与后端 validate_at 保持同一规则，前端先拦一道。 */
export function validateGeneralSettings(settings: GeneralSettings): string[] {
  const errors: string[] = [];
  const proxy = settings.httpProxy.trim();
  if (proxy) {
    if (!/^https?:\/\/\S+$/.test(proxy)) {
      errors.push("代理地址必须是 http:// 或 https:// 开头的完整 URL，例如 http://127.0.0.1:7890");
    }
  }
  const size = settings.terminalFontSize;
  if (
    !Number.isFinite(size) ||
    !Number.isInteger(size) ||
    size < MIN_TERMINAL_FONT_SIZE ||
    size > MAX_TERMINAL_FONT_SIZE
  ) {
    errors.push(
      `终端字号必须是 ${MIN_TERMINAL_FONT_SIZE}-${MAX_TERMINAL_FONT_SIZE} 的整数`,
    );
  }
  return errors;
}

/** UI 预览（?ui-preview=1）假数据。 */
export const GENERAL_SETTINGS_PREVIEW: GeneralSettingsStatus = {
  settings: {
    ...DEFAULT_GENERAL_SETTINGS,
    httpProxy: "http://127.0.0.1:7890",
    terminalFontFamily: "MesloLGS NF",
    terminalFontSize: 13,
  },
  proxyEnvActive: true,
  keepAwakeActive: true,
  warning: null,
};

/** 电脑控制设置：与 src-tauri/src/computer_control.rs 的
 * ComputerControlSettings/ComputerControlStatus 保持 serde camelCase 同构。 */

export interface ComputerControlSettings {
  enabled: boolean;
}

export interface ComputerControlStatus {
  settings: ComputerControlSettings;
  runtime: string;
  warning: string | null;
}

export const DEFAULT_COMPUTER_CONTROL_SETTINGS: ComputerControlSettings = {
  enabled: false,
};

/** 覆盖范围与安全说明（面板固定文案，单独导出便于单测）。 */
export const COMPUTER_CONTROL_COVERAGE =
  "开启后，原生聊天（Claude/Codex/Kimi/DSH）、多智能体与 Claude/Codex 终端会话中的 Agent 都可以驱动本机 Windows 应用的图形界面（仅 Windows）。";

export const COMPUTER_CONTROL_SAFETY_NOTE =
  "Agent 的点击与输入会真实作用于你的电脑；密码管理器等敏感应用被硬性禁止，密码/验证码请手动完成。观察截图包含真实屏幕内容并会发送给模型提供方，请自行斟酌。";

/** UI 预览（?ui-preview=1）假数据。 */
export const COMPUTER_CONTROL_PREVIEW: ComputerControlStatus = {
  settings: { enabled: true },
  runtime: "PowerShell 运行时（C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe）",
  warning: null,
};

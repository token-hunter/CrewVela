/** kimi-computer-use 设置：与 src-tauri/src/kimi_computer_use.rs 的
 * KimiComputerUseSettings/KimiComputerUseStatus 保持 serde camelCase 同构。 */

export interface KimiComputerUseSettings {
  enabled: boolean;
}

export interface KimiComputerUseStatus {
  settings: KimiComputerUseSettings;
  installed: boolean;
  mcpRegistered: boolean | null;
  runtime: string;
  warning: string | null;
}

export const DEFAULT_KIMI_COMPUTER_USE_SETTINGS: KimiComputerUseSettings = {
  enabled: false,
};

/** 覆盖范围与安全说明（面板固定文案，单独导出便于单测）。 */
export const KIMI_COMPUTER_USE_COVERAGE =
  "开启后注册 kimi-cu MCP 服务器（KimiCU 官方客户端，stdio）。原生聊天（Claude/Codex/Kimi/DSH）、多智能体与 Claude/Codex 终端会话中的 Agent 都可以通过 KimiCU 截图并操控本机 Windows 应用（需已安装 KimiCU）。";

export const KIMI_COMPUTER_USE_SAFETY_NOTE =
  "KimiCU 为第三方实现，Agent 的点击与输入会真实作用于你的电脑；观察截图包含真实屏幕内容并会发送给模型提供方。密码/验证码等敏感操作请自行斟酌或手动完成。";

/** UI 预览（?ui-preview=1）假数据。 */
export const KIMI_COMPUTER_USE_PREVIEW: KimiComputerUseStatus = {
  settings: { enabled: true },
  installed: true,
  mcpRegistered: true,
  runtime:
    "KimiCU 运行时已就绪（C:\\Users\\demo\\AppData\\Local\\KimiCU\\kimi-cu.exe，stdio MCP）",
  warning: null,
};

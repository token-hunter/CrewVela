/** open-computer-use 设置：与 src-tauri/src/open_computer_use.rs 的
 * OpenComputerUseSettings/OpenComputerUseStatus 保持 serde camelCase 同构。 */

export interface OpenComputerUseSettings {
  enabled: boolean;
}

export interface OpenComputerUseStatus {
  settings: OpenComputerUseSettings;
  runtime: string;
  warning: string | null;
}

export const DEFAULT_OPEN_COMPUTER_USE_SETTINGS: OpenComputerUseSettings = {
  enabled: false,
};

/** 覆盖范围与安全说明（面板固定文案，单独导出便于单测）。 */
export const OPEN_COMPUTER_USE_COVERAGE =
  "开启后注册 open-computer-use MCP 服务器（开源 iFurySt/open-codex-computer-use，基于无障碍 API 的非侵入式操控，stdio）。原生聊天（Claude/Codex/Kimi/DSH）、多智能体与 Claude/Codex 终端会话中的 Agent 都可以通过它读取界面状态并点击、输入（需已执行 npm i -g open-computer-use 安装）。";

export const OPEN_COMPUTER_USE_SAFETY_NOTE =
  "open-computer-use 为第三方开源实现（MIT），Agent 的点击与输入会真实作用于你的电脑；观察截图包含真实屏幕内容并会发送给模型提供方。密码/验证码等敏感操作请自行斟酌或手动完成。";

/** UI 预览（?ui-preview=1）假数据。 */
export const OPEN_COMPUTER_USE_PREVIEW: OpenComputerUseStatus = {
  settings: { enabled: true },
  runtime:
    "Open Computer Use 运行时已就绪（C:\\Users\\demo\\AppData\\Roaming\\npm\\node_modules\\open-computer-use\\dist\\windows\\amd64\\open-computer-use.exe，stdio MCP）",
  warning: null,
};

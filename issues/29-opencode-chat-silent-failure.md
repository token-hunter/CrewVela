Status: ready-for-human

# 修复 OpenCode Chat 模型失败后无回复无提示

## 验收

- 定位截图所选模型未回复的原因；不自动更换模型、账号或权限。
- 上游模型失败及时反馈，不能仅等待通用 30 分钟超时；错误信息不泄露凭据。
- 区分请求执行、回复、工具调用及真实模型错误，保留取消与恢复路径。
- 验证错误反馈和已有 ACP 聊天回归，按指定命令打包。

## Comments

- 当前会话选中 opencode/glm-5.3-flash-free。对应 OpenCode 日志明确返回 Free usage limit reached；本地会话最终只有 ACP session/prompt 超时，消息历史仅有两条 user。
- 真实安装版复现 ACP 请求在额度错误后仍不结束，错误只出现在 stderr。已限定当前 OpenCode 主模型请求识别明确终止错误并反馈静态中文提示，不显示原始日志，不改账号、模型或权限。
- 新回归先失败后通过；完整 Rust 1041 通过 / 40 默认忽略。另行执行真实模型检查，9.17 秒内返回中文额度错误；OpenCode 的原有取消、恢复、权限及配置回归通过。
- CUA 运行真实 OpenCodeChat + 隔离事件，确认额度错误显示、结束 busy、保留模型与恢复入口。证据与限制见 `.scratch/opencode-chat-silent-failure/qa.md`。
- 模型免费用量限制仍存在；未声称已恢复服务额度。
- 已使用指定 `npm run tauri -- build --no-bundle` 打包成功，默认产物 `src-tauri/target/release/CrewVela.exe`。构建保留现有 Rust warnings 和大 chunk 提示。未提交或推送。

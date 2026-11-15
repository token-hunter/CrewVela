Status: ready-for-human

# 修复口语群聊指令误入工程计划

截图中的两条原始指令为 `@ClaudeCode @Codex @ZCodeCLI 你们三个聊下` 和 `@模型验收-Claude @模型验收-Codex @聊天-ZCodeCLI 你三聊个天`。原分流仅识别“聊天”等词，未识别“聊下、聊个天”，导致请求落入 LLM 工程规划。第一轮实际创建了工程任务和语义复核任务，在非 Git 目录执行失败；第二轮通信计划触发 `Agent 通信任务必须使用 orchestrator 角色` 校验。

补齐“聊下、聊一下、聊一聊、聊聊、聊个天”，复用已有直接通信通道。保留工程指令排除规则、计划角色校验、权限与隔离校验。不变更已有成员职责，不删除或重试历史任务。

## 验证

- 修复前新增测试实际失败：第一条原始指令返回 `None`，预期 `Broadcast`。
- 修复后两条原始指令和常用变体通过；“先聊下，然后开发游戏”“修复聊个天功能”等仍走工程计划。
- 三成员选择和投递内容测试通过，精确成员模式排除未选择的语义复核员；工程计划、通信任务免语义复核和后台信箱测试通过。
- 全量 Rust 库测试首次：1131 通过、2 失败、49 忽略。失败为共享记忆安装下载的本机超时/重试计数测试；独立串行重跑该组：8 通过、2 忽略。不调整这些检查或超时。
- 按默认命令 `npm run tauri -- build --no-bundle` 打包成功，隐私路径扫描通过。Windows EXE SHA256：`6C579EF60BADC5D3B52174725951F583BAE6C1B1D54BE7543D63030C5530C7D1`。
- 本轮未完成新版 EXE 的三 Runtime 真实互聊验收；不将单元测试、排队或成功打包视为该验收通过。

证据：`.scratch/chat-role-validation/` 中的 `before.json`、`red.log`、`rust-tests.log`、`installation-recheck.log` 和 `build.log`。

## Comments

2026-11-15 后续正常模式实测又修复了长指令分流、Claude 完成 Hook 和 Codex 收件就绪问题，Claude Code ↔ Codex 两轮真实收发通过；ZCodeCLI 仍有可用模型与账号组件定位阻塞，三方验收未完成。最终交付及完整证据见 [正常模式通信验证](../docs/communication-normal-verification-20261115.md)，使用其中的新 EXE。

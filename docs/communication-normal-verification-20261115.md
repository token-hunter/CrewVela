# 正常模式原生会话通信验收（2026-11-15）

本轮结论：**Claude Code 与 Codex 双向实收、回复及再次收件通过；ZCodeCLI 未通过，三方验收尚未完成。**

## 运行环境与范围

- 使用 Windows x64 正式构建，直接运行 `output/CrewVela-communication-20261115/CrewVela.exe`，无 `--test-data-dir`，无 Mock。
- 使用正式 `%APPDATA%/CrewVela` 数据和现有项目“模型设置实测-20261115”，复用已有成员。Claude Code、Codex 保持 worker，ZCodeCLI 保持 orchestrator；成员职责、模型、权限配置未改动。
- 实际模型：Claude Code 使用已配置的 `glm-5.3-flash`；Codex 使用 `gpt-6.1-terra`。这里验证的是这两个 Runtime 的原生会话通信。
- 最终一轮只在启动时按 Escape 跳过 Codex 官方更新提示一次。规则下发后，不调整窗口，不手工向终端粘贴提示或提交回车，不手工写队列。
- 双方先收到本轮明确的回信范围与工具要求；收到 RESULT 后结束，避免互相确认形成循环。

## 最终两轮实测

| 链路 | 唯一问题 | 对方实际回传 | 原发送者实收 |
| --- | --- | --- | --- |
| Codex → Claude Code → Codex | `Q_CHAT3_80303ac970_R4_X_C_25d9c5`，23+64 | `RESULT:Q_CHAT3_80303ac970_R4_X_C_25d9c5:87` | 通过 |
| Claude Code → Codex → Claude Code | `Q_CHAT3_80303ac970_R4_C_X_a64908`，16×13 | `RESULT:Q_CHAT3_80303ac970_R4_C_X_a64908:208` | 通过 |

每轮同时检查：发送工具调用及同一非空调用 ID 的返回、消息库中真实双方身份和目标、接收方原生转录中的问题、原发送者原生转录中的完整 RESULT。四条发送各一次，正文匹配；不能用模型正文声称“已发送”替代这些证据。

最终两轮期间身份保持稳定：

- Claude Code：应用会话 `8dac07ca-ecef-4366-bad6-53edc6567e66`，原生会话 `898113fa-4aec-4a62-8f03-c28a891759ec`。
- Codex：应用会话 `8f43171b-4fd0-4d19-b278-dd399c7ff998`，原生会话 `01a1ddd1-8dd9-7c73-a349-38dec43f0ff0`。

Claude Code 使用 Skill 提供的原会话 CLI；Codex 使用绑定原会话身份的 `crewvela_sessions` MCP。双方最终均结束本轮，队列为空。

![正常 EXE 双向实收](../.scratch/chat-live-verification/normal-r4-roundtrips.png)

## 实测暴露并修复的问题

1. 长群聊指令中的后续“向原发送方回复”被当作发送者/接收者子句。定向识别不再跨越句号、分号和换行，普通定向发信与工程指令检查保留。
2. Claude CLI 2.1.319 的长粘贴提示带 `<pasted_content id="…">`；提交 Hook 与转录读取保留的标签、外层空白不同。提交与完成两侧统一提取完整匹配的外层标签正文，继续校验当前提示，旧轮次不能结束新任务。
3. Codex 工具轮次结束后的游标归位被当作整屏替换，撤销收件就绪。现在游标移动保持已确认的完成态；显示擦除、忙碌及权限对话框仍阻止投递。旧版出现过必须调整窗口才收件的情况，最终候选的两轮不依赖此操作。

根因位置：`src-tauri/src/multi_agent/direct_message.rs`、`src-tauri/src/sessions.rs`；对应回归位于这两个模块及 `src-tauri/src/lib.rs`。

早期短 ACK 测试中，Codex 曾只在本地显示答案，没有发送工具记录；这些轮次不计通过。早期一轮 Codex → Claude → Codex 依赖窗口重绘后才收到 RESULT，也不计入最终自动收件验收。

## ZCodeCLI 未通过及待处理项

首次三成员请求确实通过新版界面进入直接通信通道。ZCodeCLI 的真实后台运行失败，保留任务 `cli-message-d1a70580-de9c-41fe-b950-dcd3a1c1765d`，错误为“ZCode Desktop 会话尚未选择可用模型，请先选择 Personal API 模型”。此错误来自共用客户端，发生的 Runtime 为 `zcode_cli`。

进一步核对发现两层条件：

- 本机 CLI 是 `F:/github/ZCode` 的源码构建；账号桥的 `nativeProjectTokenClient` 仅从基于 Built-in 配置路径推导出的桌面版 `app.asar/out/host` 查找组件，该 CLI 布局没有此目录。这一账号组件定位兼容缺口**尚未接入修复**。
- 为区分组件路径和真实权益，诊断脚本复用了已安装的 `D:/ZCode` 原生 SDK。现有凭据刷新与个人 Coding Plan 订阅查询均返回 HTTP 200，订阅列表为 `[]`，对应模型状态为 `not-entitled`；当前也没有配置 Personal API 模型。结论仅对应当前接入读取的个人 Coding Plan 连接，不推断其他套餐或账号状态。

诊断脚本没有发起模型推理，不能视为 ZCodeCLI 互聊通过。需要可用的 Personal API 模型或可验证的账号模型连接；选择账号模型时，还需补齐上述 CLI 凭据组件兼容性，再验证 Claude ↔ ZCodeCLI、Codex ↔ ZCodeCLI 的双向实际收发。本轮没有放宽权益、许可或权限检查，也没有借用其他 Runtime 的密钥或伪造 ZCode 回复。

## 回归、交付与收尾

- 最终 Rust 全量：**1135 通过，0 失败，49 忽略**。真实模型测试保持显式运行，不混入普通回归。
- 构建命令：`npm run tauri -- build --no-bundle`，构建通过。版本命令正常；发布隐私扫描无命中。既有编译警告保留。
- EXE：`output/CrewVela-communication-20261115/CrewVela.exe`，46,398,976 字节。
- SHA256：`9CB296B1840B55E37AE401D496AA67A264162530156C25E3424F9A0D7B5D6C81`。
- 测试会话已停止，消息队列为空，无运行中的测试任务，业务目录为空；原成员配置和失败历史保留。“关闭窗口隐藏到托盘”已恢复原值。

主要证据在 `.scratch/chat-live-verification/`：`normal-process.json`、`verified-roundtrips.json`、`live-evidence.json`、`hook-capture.jsonl`、`zcode-sdk-status-final.json`、`rust-complete.log`、`build-complete.log`、`cleanup.json`。中间版本和失败证据保留，最终交付以本节 EXE 与哈希为准。

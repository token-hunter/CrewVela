# 多智能体通信验收记录

2026-11-06，Windows。实际联调版本：Claude Code 2.1.304、Codex CLI 0.162.1。

已完成通信链路修复、真实双模型 MCP 闭环和实际 CLI 连续收件验证；续验又覆盖 Claude、Codex、OpenCode、DeepSeek Harness、Pi、ZCode Desktop、ZCode CLI 七种 Runtime。Kimi 已连接，但模型请求被账号订阅权限拒绝，用户选择记录阻塞。没有把模拟进程、浏览器 Mock 或消息入队当作真实模型回复。

## 已修复的问题

| 问题 | 修复后的行为 |
| --- | --- |
| 私信能通过知识搜索或共享记忆间接泄漏 | thread、knowledge、memory 及任务初始记忆注入统一按成员可见范围过滤；验证记忆不会公开私信 |
| 部分 Runtime 的后台消息任务被只读能力检查永久拒绝 | Claude/Codex 保留强制只读；其余受支持 Runtime 使用独立消息目录和成员原权限，明确其只读能力边界 |
| 已执行消息在暂停、确认停止或恢复后再次运行 | 已开始且中断的消息轮次失败，不自动重放；未开始的排队消息仍可恢复 |
| 粘贴后延迟 Enter 确认了权限或更新菜单 | 独立记录 TUI 阻塞状态；菜单及人工菜单 Enter 不会确认待发消息，等待新的输入框就绪信号 |
| 粘贴多行文本被误判为人工提交 | 仅单独 CR/LF/CRLF 视为提交；多行及 bracketed paste 不误确认 |
| 广播、直接协作、自动化落盘失败仍可能部分投递或报告成功 | 整批持久化成功后才发布队列；写盘失败明确返回错误 |
| 已经接收消息，但协作历史投影失败，被当作发送失败重试 | 返回已入队回执和明确警告，避免用户重复发送 |
| 自动唤醒通信会话时提升到 YOLO | 保留原会话权限选择 |
| 旧 PTY 输出改变替代进程状态；审批与投递存在锁顺序风险 | 输出校验当前 PTY 绑定，统一审批与投递锁顺序 |
| Codex 紧凑 UI 首轮之后不再识别就绪，或工作时误判完成 | 支持无启动标题的输入框；工作提示阻止收件及完成兜底；尾随 ANSI/空白/分块页脚不清除已知就绪帧 |
| Codex 后台标题通知覆盖主会话 ID 和完成摘要 | 首次及异线程绑定验证精确原生 metadata 的 ID、交互来源和目录；先验证当前任务输入，再写身份、locator 和状态 |
| Windows 本机共享记忆未启动，却被超时分类阻止自动恢复 | 对托管回环地址进行独占 bind 探测，确认未监听才按离线恢复；已有监听、认证、HTTP 错误仍如实返回 |
| 本机记忆请求受出站代理影响，就绪轮询实际可能远超标称 15 秒 | 回环 HTTP 绕过出站代理；就绪请求与轮询共享 15 秒绝对期限，失败仅清理本次持有的子进程；不包含此前独立的锁等待和 init 预算 |
| Pi 项目任务没有接入协作 MCP | 通过生产私有扩展调用 Pi 官方注册 API，地址和临时身份由子进程环境传入，不写用户全局 MCP 配置；旧版缺少 API 时明确告知兼容边界 |
| 本机 OpenCode 默认模型不存在，实际请求报 APIError 2013 | 精确修正本机模型 ID 为目录已列出的 `minimax/MiniMax-M3`，保留原参数及凭据，并另测无模型覆盖的默认配置 |
| Codex 的 Impeccable Windows hook 使用 CMD 语法，却由 PowerShell 执行 | 修复本机缓存的两个 Windows command，保留真实退出码及标准流；删除 CLI 0.162.1 不支持的两个配置字段，通过原生接口信任已审查的当前哈希，不禁用 hook |

消息目录另增加路径组件、真实路径和 Windows junction 检查。未知或重叠的业务目录不能获得消息任务 Auto 权限。

## 续验：真实 Runtime 和共享记忆

| 链路 | 实际结果 | 证据 |
| --- | --- | --- |
| Claude / Codex / OpenCode / DeepSeek Harness | 四方全互通：12 条方向边，24 条 Agent 消息，12 轮模型请求；逐项核对读信、ACK、回执、身份、任务和稳定原生会话 | `output/runtime-communication-acp-four-evidence.json`、`output/runtime-communication-acp-four.log` |
| OpenCode 默认配置 ↔ DeepSeek Harness | 去掉测试模型覆盖后通过，默认即 `minimax/MiniMax-M3`；6 轮、4 条消息 | `output/runtime-communication-opencode-default-evidence.json` |
| Pi → Claude → Pi | Pi 1.1 / MiniMax-M2.7：3 轮、2 条消息；实际工具同 ID 成功，Pi 读信工具结果包含完整 ACK，原生会话保持 | `output/pi-live-acceptance.json`、`output/pi-live-real-communication-final.log` |
| ZCode Desktop ↔ ZCode CLI | 原生协议、既有 GLM-5.3 账号：6 轮、4 条消息，4 个真实读信轮次；双向发送和回执准确，原生会话保持 | `output/runtime-communication-zcode-evidence.json`、`output/runtime-communication-zcode-final.log` |
| Kimi Code | ACP 连接成功，实际模型请求 403：当前订阅没有 Kimi Code 使用权限；未宣称通过，按用户决定保留账号阻塞 | `output/runtime-communication-acp-three.log` |
| ai-memory 2.6.0 | 当前托管地址 `127.0.0.1:47374` 恢复运行，MCP 初始化和 23 工具可用；两个隔离项目同名页分别写入、读取、范围隔离和删除通过 | `output/memory-timeout-root-cause.json`、`output/memory-timeout-live-read-write.json` |
| Codex / Impeccable hook | 修复后唯一真实模型轮次准确回复随机 marker，10 项 hook 全部 completed、0 failed；此前失败的 Design deep pass Stop 实际完成，两条未知配置字段警告消失 | `output/communication-hook-repair-final.json` |

真实验收覆盖七种 Runtime 的上述链路，不代表七种两两排列的全部组合均执行。Pi 测试显式加载官方 `builtin:mcp`，生产仍使用自身正常的扩展加载机制；Pi 全局 settings/MCP 文件前后哈希不变。共享记忆保留活跃服务，未为验收停止服务再执行应用冷启动；停止后恢复路径由 Windows 独占端口、健康分类和启动预算回归覆盖。

验收自身的工具识别也做了修正：仅实际工具初始帧可建立读信身份，更新帧必须匹配同一非空 toolCallId；ToolSearch、Guardian 和正文中的工具名不算执行读信。既有四方原始事件按精确工具标题再次核验，每方都有两次实际读信成功，见 `output/runtime-communication-acp-strict-read-validation.json`。

OpenCode 与 Codex 的原配置已在修改前本地备份，具体目录和时间戳不纳入公开记录。凭据保留核验与语义差异记录仅保存在未发布的本地验收材料中；差异涉及两项兼容字段和两项审查后信任哈希。修复工具 `tools/repair-codex-plugin-hooks.ps1` 默认只展示方案，应用要求匹配审查哈希；仅针对当前已验证插件版本，插件更新后需重新核查。

Impeccable 官方 engine 0.1.5 已安装，14,720,392 字节，SHA-256 `477e544fc8880a5e82e427490cb9a5f5adab480c0e02966d33fd50772b531c71` 与官方下载的校验文件一致。当前网络下整文件下载超时，最终使用同一官方 URL 的有界 HTTP Range 下载，整文件通过校验后才执行；首轮失败、有限重试和最终结果分别保留在 `output/communication-impeccable-engine-*.json`。真实模型验收覆盖 Stop；PostToolUse 没有通过真实编辑触发，已验证相同 Windows 命令逐字传递 UTF-8 标准流及真实失败退出码，不以关闭 hook 或吞掉异常取得通过。

## 验证结果

| 验证 | 结果 | 证据与边界 |
| --- | --- | --- |
| Windows Rust 库完整回归 | 962 passed，0 failed，32 ignored | `output/communication-rust-continuation-accepted.log`；相关真实模型测试另行显式执行，其他环境检查未宣称通过 |
| Windows exe 命令入口 | 1 passed，0 failed | `output/communication-rust-bins-continuation.log` |
| Node 完整测试集 | 384 passed，0 failed，1 skipped | `output/communication-node-continuation-final.log`，含 Pi 注册与 Windows hook 兼容回归 |
| 浏览器协作交互 | 三组脚本通过 | 发送防重复、草稿、审批、项目隔离、群发、成员筛选；后端为隔离 Mock |
| CLI 投递矩阵 | 五种 sender × 五种 target 通过 | Claude、Codex、Kimi、Pi、OpenCode 的隔离 outbox/mailbox 和假 PTY；不代表五种真实模型均联调 |
| 后台消息轮次 | 八类 Runtime 的准入/权限回归通过 | 包括 DeepSeek、ZCode、ZCode CLI；不等于其真实外部服务验收 |
| 实际 HTTP MCP 隐私测试 | 通过 | 真实本机 HTTP 请求验证跨任务私信、知识及记忆搜索范围 |
| 真实 MCP 双模型通信 | 3 个模型轮次、2 条 Agent 消息通过 | Codex 工具发信 → Claude 工具读信回信 → Codex 工具读取回执；检查身份、任务、marker 和真实工具完成事件 |
| 实际 Claude/Codex CLI | 连续 4 轮通过 | 每个 CLI 两次收件并返回准确随机 marker；原生会话 ID 和 PTY 不变；普通权限，无绕过审批 |
| Windows release | 构建、版本探测通过 | `output/communication-release-build-continuation.log`；前端 tsc/Vite 随 Tauri 构建执行 |

真实 CLI 测试由测试驱动写生产 outbox，再观察实际 CLI 的模型完成 hook；没有声称该阶段是模型自己调用 `send`。模型自主调用消息工具由独立 MCP 测试覆盖。应用会话、邮箱和项目库均使用隔离目录；CLI 仍按自身机制记录测试用原生历史。

真实证据保存在 `output/communication-acceptance/`：`mcp-result.json`、`cli-result.json`、四个 `cli-round-*.json`、`cli-summary.json` 和 `build.json`。原始测试日志保留失败尝试，没有覆盖或隐藏失败。

## 失败记录与剩余边界

- 初次受限沙箱测试出现系统访问错误；最终回归在 Windows 宿主执行。没有删除有效检查来制造通过。
- 续验全量 Rust 曾出现 961 passed / 1 failed：HTTP 隐私测试的非阻塞 listener 在 Windows 使接受的连接继承非阻塞模式。独立探测复现为立即 WouldBlock（0ms），恢复阻塞后按 200ms 超时正常等待。修复测试连接以符合生产阻塞 IO，并刻意延迟发送负载覆盖竞争窗口，原来的完整 HTTP/身份/私信断言保持；根因及专项日志为 `output/communication-http-socket-root-cause.json`、`output/communication-http-socket-regression.log`。失败的完整日志保留为 `output/communication-rust-continuation-final.log`。
- 真实 CLI 测试曾发现 Windows 扩展路径不兼容、首轮之后收件停滞和后台标题污染身份；修复后四轮真实模型验收通过。
- 更新菜单测试曾触发 CLI 自更新；已恢复并重新核验 Claude Code 2.1.304、Codex CLI 0.162.1。测试进程现在关闭自动更新检查，若仍遇更新菜单会明确失败，不自动按键跳过。该开关只用于测试进程。[Codex 官方配置参考](https://developers.openai.com/ja-JP/docs/config-file/config-reference)
- MCP 首次加强断言时误把两条协调器任务通知计入 Agent 消息数量；改为按角色和真实工具事件核验。一次回归的临时数据库清理因 Tauri 仍持有句柄失败，已显式释放测试 managed state 后清理。
- 首轮的共享记忆超时已定位到 Windows 未监听回环端口的连接行为，恢复后实际跨项目读写通过；Codex 额外 hook 失败已定位到 Impeccable 的 Windows shell 不兼容。具体续验结果与本机配置备份见上文。
- OpenCode 初次因不存在的模型 ID 失败，保留原记录；修复后默认模型真实通信通过。Pi 首次隔离测试的 `--no-extensions` 禁用了官方 MCP，显式加载官方 MCP 后通过。ZCode 首次因模型在 ACK 中额外插入空格未通过严格断言，明确无空格指令后重测通过，没有放宽内容比较。
- Kimi 账号权限阻塞仍未解决；其余需要特定环境的 ignored/skipped 检查未宣称通过。
- 原生 metadata 验证证明线程身份、交互来源和目录，并不是完整的进程代次认证；不能据此声称排除了任意迟到外部 hook 的风险。
- 遗留内部初始提示和系统通知助手仍有只记录落盘错误的路径；本次已加固的用户消息入口与这些路径不同。
- 构建保留现有 Rust warning 和 Vite chunk 大小提示，未把它们隐藏成零警告。

## 复现

以下命令在仓库根目录执行；真实模型测试需要本机已安装并登录相应 CLI，会产生真实模型调用。

```powershell
rtk proxy npm test
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --bins

$env:CREWVELA_RUN_REAL_COLLABORATION = '1'
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --lib real_models_exchange_scoped_mcp_messages_between_two_agents -- --ignored --nocapture

$env:CREWVELA_ACCEPTANCE_EXE = (Resolve-Path src-tauri/target/release/CrewVela.exe).Path
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --lib real_claude_codex_bidirectional_pty -- --ignored --nocapture

$env:CREWVELA_RUN_REAL_RUNTIME_MATRIX = '1'
$env:CREWVELA_REAL_RUNTIME_SET = 'claude_code,codex,opencode,deepseek_harness'
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --lib real_acp_runtime_direct_message_matrix -- --ignored --nocapture

$env:CREWVELA_RUN_REAL_PI_COMMUNICATION = '1'
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --lib real_pi_claude_mcp_message_roundtrip -- --ignored --nocapture
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --lib real_zcode_desktop_cli_message_roundtrip -- --ignored --nocapture

rtk proxy npm run tauri -- build --no-bundle
```

浏览器回归先启动独立 Vite 端口，再设置 `CREWVELA_UI_URL`，执行 `tests/collaborationAsync.ui.mjs`、`tests/collaborationMemberCounts.ui.mjs`、`tests/retainedControlAsync.ui.mjs`。

最新 exe：`src-tauri/target/release/CrewVela.exe`，2026-11-06 19:07:51 +08:00，42,929,664 字节，版本输出 `CrewVela · 协帆 0.1.0`。SHA-256：`4180C97961354A15520A09810941F4EF26CD62311663570C1E51FA920E446D55`。构建和回归元数据见 `output/communication-acceptance/build-continuation.json`。版本探测使用隐藏的 `--version`，没有启动 GUI；未关闭用户原运行实例，前一轮构建前的文件仍保留为同目录 `CrewVela.before-communication-20261106-142505.exe`。

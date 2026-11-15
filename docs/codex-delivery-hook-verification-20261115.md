# Codex 自动投递及完成 Hook 修复验收（2026-11-15）

结论：正常 Windows 模式连续两轮自动投递、真实模型回复与完成回流通过；原生 Hook 请求触发的 10 个 Hook 均完成。交付 EXE 与实际验收候选二进制哈希一致。

## 自动投递根因与修复

Codex 0.167.1 更新菜单退出后会发出原生 OSC idle 信号，但可能只增量重绘页脚，不再完整重绘输入框。原实现把更新菜单记成普通对话框阻塞，随后拒绝使用原生 idle 清除它，导致消息已排队却不能提交。

- 将更新菜单阻塞与登录/权限对话框分开；后续原生 idle 可以解除更新或忙碌状态，不能解除登录/审批阻塞，idle 后出现的新阻塞仍优先。
- 识别更新菜单的操作项和继续/跳过提示，兼容增量绘制造成的标题残缺；不把纯更新通知当成交互菜单。
- 保留现有队列、提交确认、会话生命周期、模型参数及权限实现，没有另建投递通道。

失败反馈：`real_codex_startup_becomes_ready_after_update_skip` 修复前 25 秒超时，捕获到原生 idle 但仍 blocked；修复后 8 秒通过。确定性回归 `codex_update_skip_releases_on_fresh_native_idle_without_retyping` 修复前失败、修复后通过，并验证权限对话框仍保持阻塞。

## 完成 Hook 根因与本机处理

原生 `hook/completed` 明确定位失败源为本机 Impeccable 4.3.1 Stop Hook：`commandWindows` 使用 CMD 的 `if exist ... exit /b`，实际由 PowerShell 执行，退出码 1。

使用已有修复工具改为兼容 PowerShell 的调用，保留真实 stdin/stdout/stderr 和退出码、启用状态及超时。备份保存在 `C:\Users\PC\.codex\crewvela-hook-repairs\20261115T0300190184416Z`。

修复后内容指纹变化曾被原生保护拒绝，未关闭或绕过该检查。按 Codex 0.167.1 的正式流程，使用 `hooks/list` 的 `cwds` 清单计算完整插件内容指纹，经审阅后向 `config/batchWrite` 的 `hooks.state` upsert 审核结果，再验证两项 Hook 均 `enabled=true`、`trustStatus=trusted`。加载线程的运行时清单只返回声明哈希，不能代替完整内容复核。

修复工具现在默认保留现代 Codex 的 `trusted_content_hash`；旧版读取器兼容需要显式 `-RemoveLegacyContentHash`。没有禁用插件、吞掉真实退出错误或添加 Hook 信任绕过参数。

接口语义核对使用了同版本的上游源码：[Hook 内容保护](https://github.com/openai/codex/blob/rust-v0.167.1/codex-rs/hooks/src/engine/plugin_trust.rs)、[清单完整指纹](https://github.com/openai/codex/blob/rust-v0.167.1/codex-rs/hooks/src/engine/discovery.rs)、[原生信任写入](https://github.com/openai/codex/blob/rust-v0.167.1/codex-rs/tui/src/hooks_rpc.rs)。

此部分是本机插件兼容修复，存放在用户插件缓存和配置中；其它机器或插件更新后的内容需要独立复核，不能视为 EXE 自动修复第三方插件。

## 正常模式两轮实测

- 使用真实候选 EXE，未使用 `--test-data-dir` 或 Mock。
- 项目 `模型设置实测-20261115`；通过正式模型设置给验收成员保存 `gpt-6.1-terra`，本机默认仍是 Sol。
- 从工作台发送 `@codex` 请求；只用 Escape 跳过启动更新菜单，没有在终端补打提示词或 Enter。
- 第一轮自动返回 `CODEX_AUTO_R1_f75570eef4`，完成状态回流，队列清零。
- 同一运行会话第二轮自动返回 `CODEX_AUTO_R2_f75570eef4`，再次完成并清空队列。
- 原生会话 ID 两轮一致：`01a1dd3b-9d3c-7ae3-915a-7ee978e1c25b`；原生 `turn_context.model` 两轮均为 Terra，每个标记各有一次 `task_complete`。
- 原生 Hook 诊断请求 `01a1dd2e-e2b8-78f1-8f9b-7a26613fdae4`：10 个 Hook completed，0 failed；正常桌面两轮也未再显示 Hook failed。

证据在 `.scratch/codex-delivery-hook/`：`live-evidence.json`、`normal-two-rounds-passed.png`、`native-hook-final.json`、`native-trust-verified.json`、`cleanup.json`。

## 回归、恢复与交付

- Rust 全量：1131 passed、49 ignored；安装版 Codex 就绪验收已单独显式运行并通过。
- Codex 相关回归：95 passed、13 ignored；Hook 修复工具 4 项通过；Node 全量 495 passed、1 skipped。
- 按 `npm run tauri -- build --no-bundle` 构建；使用独立 Cargo 目录保留旧产物。构建保留 50 项编译警告及原有大 chunk 提示。
- 界面给验收成员执行「恢复继承」，81 名既有成员的名称/Runtime/模型及连接记录与测试前一致；默认覆盖未改，关闭到托盘偏好已恢复。
- 测试会话停止、待发消息为零，测试目录仍为空，项目已完成。没有修改业务代码或升级 Codex CLI。
- Codex 页脚仍有既有配置/Skill 警告，本次未宣称全部诊断和所有 Hook 事件种类都已验收；未做 macOS 原生验收。
- 交付：`output/CrewVela-codex-fix-20261115/CrewVela.exe`，46,349,824 字节；SHA256 `99284AF4AF8EE364C1B8BB77FBD7C4E1A680198DDD3AA62721EFC2B4D9368368`。与正常模式实测二进制完全一致，路径隐私检查及版本检查通过。

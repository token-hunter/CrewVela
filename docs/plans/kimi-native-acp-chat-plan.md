# Kimi Code 原生 ACP 聊天会话 · 改动方案

日期：2026-10-04 · 状态：已实施完成（后端+前端+测试；前端基于 claude-chat 会话落地后的 ClaudeChat.tsx 基座）
关联：[claude-code-native-acp-chat-plan.md](claude-code-native-acp-chat-plan.md)（flavor 泛化基座）、[codex-native-acp-chat-plan.md](codex-native-acp-chat-plan.md)（同模式第三 flavor）
调研：kimi-code 官方仓库原生支持 ACP——`kimi acp` 子命令（packages/acp-server，@agentclientprotocol/sdk ^1.3.0），零适配器；能力矩阵见 kimi 仓库 docs/zh/reference/kimi-acp.md

## 0. 需求与边界（已与用户对齐）

- 新增与 DeepSeek Harness / ZCode / Claude Chat 同级的**原生聊天会话：Kimi Code Chat**（kind `kimi_chat`），底层直连 `kimi acp`（stdio JSON-RPC）。
- 现有 `kimi_code` PTY+stream-json 协作链路完全不动（双轨并存，沿用 zcode/zcode_cli、claude_code/claude_chat 先例）。
- 登录：假设本机 kimi CLI 已登录（共享 `~/.kimi-code` 凭据）；未登录时 `session/new` 返回 `auth_required(-32000)`，错误透出并附"终端运行 kimi login"引导。
- 权限：`session/request_permission` 审批卡片（kimi 规范选项 approve_once/approve_always/reject，kind 匹配走现有 `choose_permission_option`）；初始模式 `default`，用户可通过 configOptions 的 `mode` 项切换 plan/auto/yolo。
- kimi 特性适配：**session/load 响应前回放历史通知**（启用 claude 已铺好的 `replaying` 护栏防本地历史重复）；**恢复不换发 session id、load 失败直接报错不回退**（保留"预期恢复但实际新建则拒绝启动"守卫）。

## 1. 后端改动（已完成，cargo check + 单测通过）

- `acp.rs`：`ManagedAcpKind::Kimi`（label/binary/package）+ `adapter_command` Kimi arm（`which kimi` → `.arg("acp")`，**不走 npx 兜底**）；`kimi_acp_command()` 辅助；`spawn_native_kimi_chat()`（自建命令作 command_override，`mode_candidates=["default"]`、strict=false）；租约映射 `Kimi => AgentRuntimeKind::KimiCode`（与 PTY 版共用启停开关）。
- `native_acp_sessions.rs`：`NativeChatFlavor::KimiChat`（label "Kimi Code Chat"、事件 `kimi-chat-update`/`kimi-chat-error`、permission_mode "default"、`is_standalone_kimi_chat`）；`NativeKimiSessions` newtype；spawn 分派 arm（`KimiCode.ensure_available()` + 恢复失败守卫 + `kimi_chat_spawn_error` 登录引导映射）；其余 `if dsh / else` 分支自动落入 claude 侧（回放护栏、cancel_handle、watch_claude_liveness、直接 set_config_option、允许 resume-id 替换）。
- 运行时身份：`AgentRuntimeKind::KimiChat`（serde `kimi_chat`）+ 全部机械 arm（executor×4/model_eval/coordinator/workflow_canvas/direct_message 别名/runtime UpdateSpec/sessions label、create_runtime 映射、`is_standalone_kimi_chat`、mark_native_runtime_status 守卫、configure_yolo 空臂）。
- `lib.rs`：`app.manage(NativeKimiSessions)`（存储 `kimi-chat-sessions.json`）；命令组 `create_kimi_chat_session`（前置 `KimiCode.ensure_available()`）/ `kimi_chat_snapshot` / `start_kimi_chat_session` / `set_kimi_chat_config` / `send_kimi_chat_message` / `answer_kimi_chat_permission` / `cancel_kimi_chat_turn`；生命周期钩子（resume/stop/archive/archive_project/delete/delete_all/pty_kill_all/窗口关闭/启动恢复循环）全部加 kimi 分支；**顺手补齐 claude 会话遗漏的 `resume_session` 分支**。
- 测试：`tests/fixtures/kimi-acp-agent.mjs` 假对端（initialize 能力面+authMethods、new 带 model/thinking/mode 三选择器、load 同 id+响应前回放、流式+工具+权限同号反向请求、cancel、set_config_option/set_mode、available_commands_update）；Rust 单测 3 个全过：`kimi_chat_streams_approves_commands_and_config_options`、`kimi_chat_resume_drops_replayed_history_and_keeps_session_id`（回放护栏）、`kimi_chat_cancel_interrupts_inflight_turn`。

## 2. 前端改动（已实施）

以 ClaudeChat.tsx 为基座：
- `types.ts`：`AgentRuntimeKind` 加 `"kimi_chat"`（复用 claude 已加的 `HarnessSessionSnapshot.commands`）。
- `sessionCapabilities.ts`：`isStandaloneKimiChatSession` 并入 `isNativeChatSession`。
- `runtimeLogos.ts`：`kimi_chat` 复用 `agent-kimi-code.svg`，label "Kimi Code Chat"。
- `nativeAgentSwitch.ts` + `NativeAgentWelcome.tsx`：`NativeAgentKind` 加 `"kimi_chat"`，欢迎页复用 kimi logo（不新增动画资产）。
- `KimiChat.tsx`（新）：消息流/工具卡/权限卡/取消/附件/configOptions 驱动的 model+thinking+mode 选择器/命令菜单（snapshot.commands）；事件 `kimi-chat-update`/`kimi-chat-error`；invoke kimi 命令组。
- `App.tsx`（openKimiChat/switchNativeAgent/enabledAgents/渲染分支/新建路由）、`NewSessionDialog.tsx`（agents 列表 + kimi_code 开关映射）、`AppSidebar.tsx`（"Kimi Code Chat"入口，保留 kimi_code PTY 双轨）。
- 附带：`CollaborationSettingsPanel.tsx` / `MultiAgentControl.tsx` 的 RUNTIME_LABELS 补 kimi_chat（Record 穷举键强制）。
- 测试：`sessionDisplayNames.test.cjs` 补 kimi_chat 判定/显示名/PTY 区分用例（4/4 过）。

## 2.1 验证结论（2026-10-04）

- `cargo check` 零错误零警告；`native_acp_sessions` 模块 20/20 通过（17 dsh + 3 kimi）。
- `npx tsc --noEmit` 零错误。
- 前端测试：kimi 相关全部通过；仓库另有 11 个失败测试文件（workflowCanvas/sidebarSettingsRoutes/runtimeUpgradeConcurrency 等）经干净 HEAD worktree 对照验证为**历史遗留失败**，与本方案及 claude-chat 在途改动均无关。

## 3. 风险与对策

| 风险 | 对策 |
| --- | --- |
| kimi 未安装/未登录 | create 前置 ensure_available；auth_required 映射登录引导文案 |
| load 回放与本地历史重复 | 回放护栏 + 单测覆盖 |
| 与 claude-chat 并行会话同文件冲突 | 后端已按其泛化基座叠加并独立测试；前端排队至其收工（用户知情） |
| configOptions 渲染 | 前端 walker 已递归泛化；fixture 按 kimi 真实三选择器建模并断言 |

## 4. 后续候选（不阻塞）

协作任务 KimiCodeAdapter 升级 ACP 优先+PTY 回退；session/list 导入既有 kimi 会话；`kimi acp --login` 设备码引导；elicitation 表单；fs/terminal reverse-RPC 接管。

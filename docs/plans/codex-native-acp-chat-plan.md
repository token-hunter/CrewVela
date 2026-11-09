# Codex 原生 ACP 聊天会话 · 改动方案

日期：2026-10-04 · 状态：已实施（随 claude_chat flavor 泛化落地后叠加，全部单测/构建通过）
关联：[claude-code-native-acp-chat-plan.md](claude-code-native-acp-chat-plan.md)（本方案在其泛化架构上追加第四个 flavor：DeepSeek Harness / ClaudeChat / KimiChat / CodexChat）
调研结论：codex 官方仓库（REFERENCE_ROOT/codex）无内建 ACP；接入依赖 `@agentclientprotocol/codex-acp@1.11.0`（已随批式画布链路安装于 node_modules），适配器自行 spawn `codex app-server`。

## 0. 需求与边界（已与用户对齐）

- 在 CrewVela 中新增与 DeepSeek Harness、ZCode Desktop、Claude Code Chat 同级的**原生聊天会话：Codex**，底层走 ACP（codex-acp 适配器）。
- 现有 codex PTY 会话（`codex` runtime kind，「Codex CLI」终端 TUI）完全不动，双轨并存（沿用 zcode/zcode_cli 先例）。
- 登录：假设用户本机已安装并登录 codex CLI；未登录时把适配器错误清晰透出，不做账号管理。

## 1. 适配器能力面（codex-acp@1.11.0 本机产物核验）

| 能力 | 机制 | 现状 |
| --- | --- | --- |
| 会话生命周期 | `session/new` / `session/load`（声明 loadSession）/ `session/prompt` / `session/cancel` | `AcpConnection` 已全覆盖 |
| 流式输出 | `agent_message_chunk` + `agent_thought_chunk`（codex reasoning→reasoning 字段） | update 流路径复用；thought 需 claude 泛化后的回退路径确认覆盖 |
| 权限审批 | `session/request_permission`（allow_always/once、reject） | ManagedPermissionHandler 复用 |
| 模式 | read-only / agent / agent-full-access | spawn 传 `&["agent"]`、strict=false（容忍降级） |
| 配置切换 | `configOptions` + `session/set_config_option`（具体 id 以 session/new 实测为准） | `set_config_option` 通用封装已具备 |
| MCP | 声明 mcpCapabilities.http | 满足（独立聊天不注入 crewvela 控制面亦兼容） |
| 附件 | prompt content `resource_link` 块 | `prompt_content_interactive` 已支持 |
| 恢复回放 | `session/load` 后是否 replay 历史 | 实测；若 replay 则启用与 claude 相同的回放护栏 |

## 2. 实施清单（在 claude_chat 泛化落地后执行）

### 后端
- `multi_agent/domain.rs`：`AgentRuntimeKind` 新增 `CodexChat`（id `codex_chat`，显示 "Codex Chat"）；各标签映射（model_eval/coordinator/workflow_canvas/executor/sessions/direct_message）补 arm；`adapter_for`/`resolve_runtime` 走「仅独立原生聊天」unreachable 分支（同 ClaudeChat）。
- `sessions.rs`：`create_runtime` 映射 `("codex_chat", "Codex Chat")`；`is_standalone_codex_chat` 判定；`mark_native_runtime_status` 守卫加入。
- `acp.rs`：新增 `spawn_native_codex_chat`（镜像 `spawn_native_claude_chat`：`ManagedAcpKind::Codex` + `&["agent"]` + strict=false）。
- `native_acp_sessions.rs`：`NativeChatFlavor` 追加 `CodexChat`——独立存储 `codex-chat-sessions.json`、事件名 `codex-chat-update`/`codex-chat-error`、spawn 分派、轻量存活监控（无控制面）、reasoning 流（agent_thought_chunk）。
- `lib.rs`：`app.manage(NativeCodexSessions::open_at(...))`；命令组 `create_codex_chat_session` / `codex_chat_snapshot` / `start_codex_chat_session` / `send_codex_chat_message` / `answer_codex_chat_permission` / `cancel_codex_chat_turn` / `set_codex_chat_config`；`resume_session`/`stop`/`archive`/`delete`/`pty_kill_all`/启动恢复循环补 codex 分支；`create` 前置 `AgentRuntimeKind::Codex.ensure_available()`（与 codex PTY 共用启停开关）。

### 前端
- `types.ts`：`AgentRuntimeKind` 加 `"codex_chat"`。
- `sessionCapabilities.ts`：`isStandaloneCodexChatSession` 并入 `isNativeChatSession`；显示名 "Codex Chat"。
- `runtimeLogos.ts`：`codex_chat` 复用 codex 资产。
- `nativeAgentSwitch.ts` / `NativeAgentWelcome.tsx`：`NativeAgentKind` 加 `"codex_chat"`，切换导航扩展。
- `CodexChat.tsx`（新，以 ClaudeChat.tsx 为基座）：消息流（reasoning 折叠）、工具调用、权限卡片、取消、附件、configOptions 选择器、斜杠命令（若有）；事件 `codex-chat-update`。
- `App.tsx` / `NewSessionDialog.tsx` / `AppSidebar.tsx`：`openCodexChat` 入口与三/四档切换。

### 测试
- `tests/fixtures/codex-acp-agent.mjs`（已就位）：initialize/new/load/thought+message 流/工具/权限/cancel/set_config。
- `native_acp_sessions` codex flavor 单测：会话流式落库（含 reasoning）、权限问答、取消中断、恢复校验、模型切换。
- `tests/sessionDisplayNames.test.cjs` 补 codex_chat 用例。
- 手工 QA：未登录 codex CLI 错误透出；模式降级提示；恢复会话历史不重复。

## 3. 风险与对策

| 风险 | 对策 |
| --- | --- |
| codex-acp 依赖本机 codex CLI（spawn codex app-server） | 未安装/未登录错误透出 + 引导文案 |
| codex-acp 升级 breaking（1.11.0 pinned） | 仅用稳定 ACP 表面；升级独立验证 |
| load 回放重复历史（若存在） | 复用 claude 回放护栏 |
| 与 claude_chat 并行实施的文件冲突 | 本方案排队至其落地后执行（用户已确认） |

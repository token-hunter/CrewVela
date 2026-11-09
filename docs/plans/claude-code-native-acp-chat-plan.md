# Claude Code 原生 ACP 聊天会话 · 改动方案

日期：2026-10-04 · 状态：**已实施**（全量测试通过；同日另一会话在本方案之上复用 flavor 泛化扩展了 kimi_chat / codex_chat）
前置调研：[docs/research/claude-code-acp-support.md](../research/claude-code-acp-support.md)

## 0. 需求与边界（已与用户对齐）

- 在 CrewVela 中新增与 DeepSeek Harness、ZCode Desktop 同级的**原生聊天会话：Claude Code**，底层走 ACP。
- 体验对齐 DeepSeek Harness，但按 Claude Code 原生能力裁剪；**模型切换第一期做**。
- 登录：**假设用户本机已安装并登录 claude CLI**，CrewVela 不做账号管理；未登录时把错误清晰透出。
- 权限：走 **ACP `session/request_permission` 审批卡片**（模式默认 `default`），不放行 bypassPermissions。
- 适配器：沿用已内置的 `@agentclientprotocol/claude-agent-acp@0.63.0`（node_modules/.bin 已装，批式画布已在用，Windows 已验证可跑）。

## 1. 适配器能力面（本机 0.63.0 源码核验）

| 能力 | 机制 | 对应实现 |
| --- | --- | --- |
| 会话生命周期 | `session/new` / `session/load(sessionId)` / `session/prompt` / `session/cancel` | `AcpConnection` 现有代码已全覆盖（acp.rs:427-456 走 `session/load` 恢复） |
| 流式输出 | `session/update` → `agent_message_chunk` / `tool_call` / `tool_call_update` | native_acp_sessions 现有 update_handler 回退路径可直接复用 |
| 权限审批 | `session/request_permission`（options 含 allow_always 等） | ManagedPermissionHandler 模式复用（harness 同款） |
| 模型切换 | `session/set_config_option`，configId=`model`（还有 `mode`/`effort`/`agent`/`fast`） | `set_deepseek_config` 泛化为 `set_config_option` 即可 |
| 权限模式 | `session/set_mode` + session/new 返回 `modes`（default/acceptEdits/plan/bypassPermissions…） | acp.rs:468-522 模式协商逻辑已存在，非 dsh 分支可用 |
| 斜杠命令 | `available_commands_update` → `AcpCommand` 列表；命令以 prompt 文本发送 | acp.rs 已解析 commands（含测试） |
| 历史回放 | `session/load` 响应后 replay 全部历史通知 | 需加"回放护栏"丢弃，避免与本地持久化历史重复（见 §3.4） |
| 附件 | prompt content `resource_link` 块 | `prompt_content_interactive` 已支持 |
| 未登录 | 合成 login 消息 / authRequired | 第一期仅透出为聊天错误提示 |

已知限制：0.63.0 比 npm latest（0.85.1）旧；本方案用到的全部接口 0.63.0 已具备，升级作为独立后续项。

## 2. 总体思路

**后端**：把 `native_acp_sessions.rs`（1671 行，目前写死 DeepSeek Harness）按「flavor」泛化，新增 `ClaudeChat` flavor；dsh 专属能力（TCP 控制插件、/goal、控制面 busy 轮询）保留在 dsh 分支，Claude 分支只用标准 ACP。两个 flavor 各自独立存储文件与 Tauri State（newtype 包装），互不污染。
**前端**：`ClaudeChat.tsx` 以 `DeepSeekHarnessChat.tsx` 为基座裁剪：去掉 goal/控制插件 UI，命令菜单改用 ACP `available_commands`，模型/模式/effort 等选择器直接吃 `configOptions`（现有代码已支持渲染任意非 model 配置项）。
**运行时身份**：新增 `AgentRuntimeKind::ClaudeChat`（id `claude_chat`，显示 "Claude Code Chat"），沿用 zcode/zcode_cli 的「原生聊天 vs PTY」双轨先例——现有 `claude_code` PTY+Provider 链路完全不动。

## 3. 后端改动清单

### 3.1 运行时身份（src-tauri/src/multi_agent/domain.rs、sessions.rs）

- `AgentRuntimeKind` 新增 `ClaudeChat`，`as_str()` → `"claude_chat"`；serde snake_case 自动兼容。
- `sessions.rs create_runtime` 映射表加 `("claude_chat", "Claude Code Chat")`（provider_id 自动成为 `local-runtime-claude_chat`）。
- 新增 `is_standalone_claude_chat(record)`：kind==Terminal && agent_runtime_kind==Some("claude_chat") && provider_id=="local-runtime-claude_chat" && 无 multi-agent 归属（与另外两个判定完全同构）。
- `mark_native_runtime_status` 的 standalone 守卫（sessions.rs:1211）加入 claude 分支。

### 3.2 acp.rs：连接层小改

- `AcpConnection::set_deepseek_config` 泛化重命名为 `set_config_option`（逻辑不变：校验值在已公布选项中 → `session/set_config-option` → 更新 config_options）；`set_deepseek_model` 保留 dsh 的模型路由解析，Claude 的 model 值直接透传（适配器自行校验）。
- 新增 `spawn_native_claude_chat(cwd, resume_session_id, permission_handler, process_id)`：复用 `spawn_for_with_cli_identity(ManagedAcpKind::ClaudeCode, …, mode_candidates=&["default"], strict=false, interactive=true, app_session_id)`——恢复走已有 `session/load` 分支；未协商到 mode 时容忍继续（受限模式）。
- `cancel`（session/cancel 通知）开放为 `pub(crate)`，供 Claude 分支取消本轮。
- dsh 专属（HarnessControl、canonical_harness_directory 的 dsh 分支）不动。

### 3.3 native_acp_sessions.rs：flavor 泛化（核心工作量）

- 新增 `NativeChatFlavor { DeepSeekHarness, ClaudeChat }`；`NativeAcpSessions` 构造时绑定 flavor（含：存储路径、事件名 `claude-chat-update`/`claude-chat-error`、错误文案前缀）。
- `SessionLocator.permission_mode`：claude 存 ACP mode id（初始 `"default"`）；启动一致性校验按 flavor 解释。
- **spawn 分派**：dsh → `spawn_native_deepseek_harness`（控制插件）；claude → `spawn_native_claude_chat`。claude 分支不设 control_handle、不开 TCP watch_stream。
- **update_handler**：两 flavor 共用现有逻辑（claude 恒走 `stream_authoritative=false` 的 chunk 回退路径 + `finish_assistant_message` 收尾）。
- **busy/存活监控**：dsh 保留 `watch_connection` 的控制面轮询；claude 新增轻量版：仅 `has_exited()` 轮询 + 进程退出置 Error，busy 完全由 `prompt_inflight` 生命周期驱动（send 路径已具备）。
- **send 预检**：dsh 查控制面 state/skill；claude 只查 busy 标志（已在 compare_exchange 处兜底）。
- **cancel_turn**：dsh → 控制 `cancel`；claude → `AcpConnection::cancel()`（session/cancel）+ cancelled 标志（`request_cancellable` 已联动）。
- **control/set_config**：claude 无 control 动作；`set_config` 走 `set_config_option`（model/mode/effort/agent/fast 通用）。
- **快照**：`HarnessSessionSnapshot` 增加 `commands: Vec<AcpCommand>`（claude 的斜杠命令菜单数据源；dsh 恒空）。`config_option_update` 通知同样入 commands（`available_commands_update` 已有解析）。
- **stop/archive/delete/stop_all**：按 flavor 分派到对应 store 与判定函数；`ensure_record` 同理。
- 测试：沿用 `test_peer` 假 ACP agent 机制补 claude flavor 用例（见 §6）。

### 3.4 历史回放护栏（claude 专属，新增）

适配器在 `session/load` 响应**之后**replay 全部历史通知；本地已持久化历史会与之重复。方案：`LiveSession` 加 `replaying: AtomicBool`——claude 恢复启动后置 true，期间 update_handler 丢弃 `agent_message_chunk`/`tool_call`（仅放行 `config_option_update`/`available_commands`），首次 `send_message` 成功提交后置 false。dsh 不受影响。

### 3.5 lib.rs：命令与生命周期接线

- 管理 State：`app.manage(NativeClaudeSessions(NativeAcpSessions::open_at(config_dir.join("claude-chat-sessions.json"), ClaudeChat)))`（newtype，避免与 dsh State 类型冲突）。
- 新增 tauri 命令（镜像 deepseek 组）：`create_claude_chat_session`、`claude_chat_snapshot`、`start_claude_chat_session`、`send_claude_chat_message`、`answer_claude_chat_permission`、`cancel_claude_chat_turn`、`set_claude_chat_config`。
- `stop_session`/`archive_session`/`pty_kill_all`/窗口关闭/启动恢复循环（lib.rs:5226-5234、5262-5274）加 claude 分支。
- `create_claude_chat_session` 前置检查：`AgentRuntimeKind::ClaudeChat.ensure_available()`；适配器二进制存在性沿用 `adapter_command`（本地 node_modules → 全局 → npx 兜底）。claude CLI 未安装/未登录不做前置探测，由首轮交互的错误透出（提示语引导 `claude` 登录）。

## 4. 前端改动清单

- `types.ts`：`AgentRuntimeKind` 加 `"claude_chat"`；`HarnessSessionSnapshot` 加 `commands?: { name; description; inputHint? }[]`。
- `sessionCapabilities.ts`：`isStandaloneClaudeChatSession` 并入 `isNativeChatSession`；`sessionDisplayTitle`/`sessionDisplayProvider` 认识 "Claude Code Chat"。
- `runtimeLogos.ts`：`RUNTIME_LOGOS`/`RUNTIME_LABELS` 加 `claude_chat`（logo 复用 claude_code 资产）。
- `nativeAgentSwitch.ts` + `NativeAgentWelcome.tsx`：`NativeAgentKind` 加 `"claude_chat"`；三档切换导航；欢迎页吉祥物/文案占位（第一期复用现有视觉体系，不新增动画资产）。
- **`ClaudeChat.tsx`（新组件，以 DeepSeekHarnessChat 为基座）**：
  - 保留：连接/恢复头部、消息流（reasoning 折叠、流式状态）、工具调用列表、权限审批卡片、取消本轮、附件（resource_link）、configOptions 驱动的模型选择器 + 任意非 model 配置选择器（effort/agent 等自动出现）。
  - 变更：事件监听 `claude-chat-update`/`claude-chat-error`；invoke 换 claude 命令组；命令菜单数据源 `snapshot.commands`（斜杠命令按 prompt 文本发送）；权限模式选择器改吃 `mode` 配置项（`set_claude_chat_config` configId="mode"）；去掉 goal/控制插件执行结果区块。
  - `mode` 选择器展示 ACP mode 元数据（default/acceptEdits/plan…，适配器在 configOptions.options 里带 name/description）。
- `App.tsx`：`openClaudeChat`（选目录 → `create_claude_chat_session` → 选中）；渲染分支改三路 `isStandaloneHarnessSession ? <DeepSeekHarnessChat/> : isStandaloneZCode ? <ZCodeChat/> : <ClaudeChat/>`；`switchNativeAgent` 泛化到第三 kind。
- `NewSessionDialog.tsx`：agents 列表加 `claude_chat`（直选目录，不进 Provider 步骤；描述「本机 Claude Code CLI 账号 · ACP 原生聊天」）。
- `SessionList.tsx`/`AppSidebar.tsx`：显示名与图标随上述类型联动（`tests/sessionDisplayNames.test.cjs` 补用例）。

## 5. 分期

**第一期（本方案主体，可用即交付）**：§3、§4 全部——聊天/流式/工具调用/权限卡片/取消/恢复/模型与配置切换（含 mode）/斜杠命令/附件/会话生命周期/三档切换。
**第二期候选（另行立项，不阻塞）**：适配器升级 0.85.x；elicitation 卡片（AskUserQuestion）；排队与 steering（busy 时入队而非拒绝）；子智能体嵌套 transcript（clientCapabilities `_meta`）；CrewVela Provider → ANTHROPIC_BASE_URL 桥接；/compact 等内置命令的白名单菜单化。

## 6. 测试计划

- Rust：`native_acp_sessions` claude flavor 单测（test_peer 假 agent）：register/start/send 流式落库/权限问答/取消/回放护栏丢弃/恢复校验；`AgentRuntimeKind` serde 用例；`is_standalone_claude_chat` 判定用例；`set_config_option` 校验用例。注意 zcode 会话内跑 cargo test 需 `env -u` 净化的既有约定。
- 前端：`sessionDisplayNames.test.cjs` 补 claude_chat 显示名；`sessionCapabilities` 判定纳入三 flavor。
- 手工 QA 清单：未登录 claude CLI 时的错误提示；Windows 路径（非 `\\?\` verbatim）cwd；恢复会话历史不重复；切模型/切 mode 生效；审批卡片允许/拒绝/超时；应用重启后 pending→stopped 恢复。

## 7. 风险与对策

| 风险 | 对策 |
| --- | --- |
| 0.63.0 适配器与 npm latest 行为差异 | 仅用稳定 ACP 表面；升级独立验证后再换 pinned 版本 |
| 上游 CI 无 Windows 保证 | 本地 node_modules 已在 Windows 批式路径验证；QA 清单覆盖 |
| load 回放与本地历史重复 | §3.4 回放护栏 + 手工 QA |
| AgentRuntimeKind 扩枚举波及画布/管理页 | 新 kind 只进「独立会话」链路，画布 agent 编辑器白名单不加；管理页沿用现有偏好开关机制 |
| claude CLI 未登录体验 | 首轮错误透出 + 引导文案；不做静默重试 |

## 8. 触达文件一览（预估）

后端：`multi_agent/domain.rs`、`sessions.rs`、`acp.rs`、`native_acp_sessions.rs`、`lib.rs`；
前端：`types.ts`、`sessionCapabilities.ts`、`runtimeLogos.ts`、`nativeAgentSwitch.ts`、`NativeAgentWelcome.tsx`、`ClaudeChat.tsx`（新）、`App.tsx`、`NewSessionDialog.tsx`、`SessionList.tsx`、`AppSidebar.tsx`；
测试：`tests/sessionDisplayNames.test.cjs`、`src-tauri` 内嵌单测。

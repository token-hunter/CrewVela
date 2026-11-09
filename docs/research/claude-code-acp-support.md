# Claude Code 是否支持 ACP（Agent Client Protocol）调研

> 调研日期：2026-10-04
> 调研问题：Claude Code 是否支持 ACP？能否通过 ACP 接入一个自定义客户端实现原生聊天对话？
> 方法：只采用一手来源（协议官方站点、Anthropic 官方文档与仓库、适配器源码/npm 包、GitHub API 元数据）。

---

## 1. 结论速览（TL;DR）

1. **Claude Code 没有原生 ACP 支持**。Anthropic 官方文档（code.claude.com 全站 sitemap 共 219 个英文页面及全部多语言页面）中 **0 处** 出现 "ACP"；anthropics/claude-code 的 CHANGELOG（8200+ 行）中也 **0 处** 提及 ACP。官方的程序化接入方式是 **Claude Agent SDK** 与 **headless CLI（--print / stream-json）**。
2. **但可以通过官方 ACP 生态中的适配器获得完整 ACP 语义的 Claude Code 会话**：[`@agentclientprotocol/claude-agent-acp`](https://github.com/agentclientprotocol/claude-agent-acp)（前身为 Zed 的 `zed-industries/claude-code-acp`，仓库已 301 迁移并更名）。它是一个 TypeScript 包装器，内部调用**官方** `@anthropic-ai/claude-agent-sdk`（package.json 依赖版本 0.3.287），对外暴露 ACP agent（stdio JSON-RPC）。
3. 该适配器**活跃维护**（npm `@agentclientprotocol/claude-agent-acp` latest 0.85.1，发布于 2026-10-02；仓库最后 push 2026-10-02，2608 stars，未归档），实现了 ACP 的 `session/new`、`session/load`、`session/list`、`session/prompt`、`session/update`（流式通知）、`session/cancel`、权限请求、elicitation 等完整会话语义。
4. **Windows 可用性**：适配器是纯 Node 包（无 node-pty 等原生依赖，终端由 ACP 客户端侧创建），要求 **Node >= 22**；Claude Code 本身官方支持原生 Windows（Windows 10 1809+）。未发现"无法在 Windows 运行"的 issue；但其 CI 只跑 ubuntu-latest，Windows 不在官方测试矩阵内，属于"事实可用、未经上游保证"。
5. **生态方向参照**：Gemini CLI 已官方内置 ACP 模式（`gemini --acp`，文档 `docs/cli/acp-mode.md`），ACP 官方 agents 注册表中 Gemini CLI 直接列出，而 Claude 只以"Claude Agent — via Zed's SDK adapter"（适配器）形式出现。ACP 侧对 Claude 的定位就是适配器方案。

---

## 2. ACP 协议是什么

**定义与定位**：ACP（Agent Client Protocol）是"代码编辑器/IDE 与编码 agent 之间通信"的开放标准，类比 LSP 之于语言服务器——agent 实现一次即可对接所有 ACP 兼容编辑器。本地 agent 作为客户端（编辑器）的**子进程**运行，通过 **stdio 上的 JSON-RPC 2.0** 通信；远程（HTTP/WebSocket）传输仍在推进中。协议尽量复用 MCP 的 JSON 表示，为 agent 编码场景增加自定义类型（如 diff 展示），默认用户可读文本格式为 Markdown。（来源：[ACP 官方介绍](https://agentclientprotocol.com/)、[Introduction](https://agentclientprotocol.com/get-started/introduction)）

**核心方法（v1 稳定版规范）**（来源：[Protocol v1 Overview](https://agentclientprotocol.com/protocol/v1/overview)）：

| 方向 | 方法 | 作用 |
|---|---|---|
| Client → Agent | `initialize` | 版本/能力协商 |
| Client → Agent | `authenticate` / `logout` | agent 需要鉴权时 |
| Client → Agent | `session/new` / `session/load` | 新建 / 恢复会话（`load` 需 `loadSession` 能力） |
| Client → Agent | `session/prompt` | 发送用户输入，开启一个 turn，直到 agent 返回 stop reason |
| Client → Agent | `session/set_mode` | 切换会话模式（可选） |
| Client → Agent | `fs/read_text_file`、`fs/write_text_file` | agent 请求客户端读写文件（可选） |
| Client → Agent | `terminal/create`、`terminal/output`、`terminal/wait_for_exit` 等 | agent 请求客户端操作终端（可选） |
| Client → Agent | `session/cancel`（notification） | 取消进行中的 turn |
| Agent → Client | `session/update`（notification） | **流式进度通知**：消息块（agent/user/thought）、工具调用与更新、计划（plan）、可用命令更新、模式变更 |
| Agent → Client | `session/request_permission` | **权限请求**：工具调用前向用户请求授权 |
| Agent → Client | `elicitation/create` / `elicitation/complete` | 结构化用户输入 / 带外 URL（如 OAuth）完成 |

规范分 v1（稳定）与 v2（草案，含权限请求增强、会话恢复回放等 RFD）。（来源：[文档索引 llms.txt](https://agentclientprotocol.com/llms.txt)、[v1 规范目录](https://agentclientprotocol.com/protocol/v1/overview)、[v2 迁移](https://agentclientprotocol.com/protocol/v2/migration)）

ACP 由 Zed 团队发起（适配器最初作者署名 "Zed Industries"，见 [npm 包元数据](https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp)），现由 agentclientprotocol 组织维护，生态内有官方 SDK（TS/Python/Rust/Kotlin/Java）。（来源：[SDK 列表](https://agentclientprotocol.com/libraries/typescript)）

---

## 3. Claude Code 是否原生支持 ACP —— 否

以下均为一手核验结果：

- **官方文档零提及**：抓取 `https://code.claude.com/sitemap.xml`（Claude Code 官方文档站全量 sitemap，含 219 个英文页面及 de/fr/ja 等多语言镜像），对 `acp`（不区分大小写）与 `agent-client-protocol` 的匹配数为 **0**。Claude Code 与 Claude Agent SDK 文档（[Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview)、[Headless 模式](https://code.claude.com/docs/en/headless)）中均无 ACP 字样。
- **官方 CHANGELOG 零提及**：`anthropics/claude-code` 仓库 [CHANGELOG.md](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)（8204 行）grep `acp` / `agent client protocol` 命中 0 条。
- **社区请求仍开放、无官方承诺**：anthropics/claude-code 中最初的 ACP 功能请求 [#6686](https://github.com/anthropics/claude-code/issues/6686) 已被关闭（2026-02-09），同日新开的 [#24411 "Feature Request: Add support for Agent Client Protocol (ACP)"](https://github.com/anthropics/claude-code/issues/24411) 目前仍为 open，标签 `area:ide` / `enhancement` / `external`，无 Anthropic 员工回应、无关联 PR。
- **ACP 官方注册表的定位**：[ACP agents 列表](https://agentclientprotocol.com/get-started/agents) 中 **"Claude Agent" 条目明确标注 "via Zed's SDK adapter"**（即 claude-agent-acp 仓库），与直接内建 ACP 的 Gemini CLI 等并列对比——ACP 生态自己也是把 Claude 归类为"经适配器接入"。

**结论：Claude Code（CLI 与 Agent SDK）本身不内置 ACP 服务端，Anthropic 也没有任何公开的原生 ACP 计划声明。**

---

## 4. Claude Code 的官方程序化接口（与 ACP 的关系）

Anthropic 官方给出的"把 Claude Code 嵌入自己应用"的路径是 **Claude Agent SDK**，以及同一能力的 CLI headless 形态：

### 4.1 Claude Agent SDK（适配器的地基）

- 定位：在自有应用中嵌入 Claude Code 的 agent 引擎，提供"与 Claude Code 相同的工具、agent 循环和上下文管理"。**SDK 本质上是驱动 Claude Code 本体的引擎**——即 headless 的 Claude Code。（来源：[Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview)）
- 形态：TypeScript（`@anthropic-ai/claude-agent-sdk`）与 Python 双语言包；其它语言建议以子进程方式跑 CLI `-p --output-format json`。（来源：同上）
- 关键能力（来源：[TypeScript API 参考](https://code.claude.com/docs/en/agent-sdk/typescript)、[overview](https://code.claude.com/docs/en/agent-sdk/overview)）：
  - `query()` 流式输出（async generator，逐消息产出 assistant/user 事件）；
  - **`canUseTool` 权限回调**：权限流落到"需要询问"时回调宿主决定（对应 ACP 的 `session/request_permission` 语义）；
  - `hooks`：agent 生命周期关键点执行自定义代码；
  - 会话管理：`resume`（按 session ID 恢复）、`forkSession`、`resumeSessionAt`、`sessionStore`（外部存储镜像会话以便跨主机恢复）；
  - 自定义工具、MCP、子 agent、skills/commands/memory、插件、system prompt 定制（`systemPrompt` / `appendSystemPrompt`）。
- **SDK 与 ACP 的关系：无官方关系**。SDK 文档不提 ACP；ACP 适配器是社区（Zed → agentclientprotocol org）在 SDK 之上的一层包装。

### 4.2 Headless CLI（对比基准）

（来源：[Headless 文档](https://code.claude.com/docs/en/headless)、[CLI 参考](https://code.claude.com/docs/en/cli-reference)）

- `claude -p "..."`（`--print`）非交互执行；`--bare` 精简启动模式。
- `--output-format text | json | stream-json`；`stream-json` 为换行分隔的实时事件流（`system/init`、`stream_event` 文本增量、assistant/user 消息、末行 `result` 含 cost 与 session 元数据），配合 `--verbose` 与 `--include-partial-messages` 可拿到 token 级流式。
- **`--input-format stream-json`**：print 模式下以换行分隔 JSON 从 stdin 送入用户消息，实现**多轮程序化对话**；`--replay-user-messages` 可回显确认。这是不依赖 SDK 的"穷人版双向流"。
- 会话续接：`--continue`（最近会话）、`--resume <session_id>`（指定会话）。
- 权限：`--allowedTools`、`--permission-mode`（`auto`/`dontAsk`/`acceptEdits`）、`--permission-prompts none`。
- 对比结论：headless CLI 提供流式与多轮，但**没有标准化的权限请求交互协议**（只能靠 allow-list/permission-mode 预决策）、没有结构化的 plan/工具展示语义；ACP 把这些 UX 语义标准化了。这正是适配器存在的价值。

### 4.3 IDE 集成

Claude Code 的官方 IDE 集成是 VS Code / JetBrains 专用扩展（私有协议），见 [VS Code 文档](https://code.claude.com/docs/en/vs-code) 与 [JetBrains 文档](https://code.claude.com/docs/en/jetbrains)——不是开放协议，自定义客户端无法使用。

---

## 5. 社区适配器：claude-code-acp → claude-agent-acp

### 5.1 身份与迁移历史

- 原 `zed-industries/claude-code-acp` 仓库已 **301 永久重定向** 至 [`agentclientprotocol/claude-agent-acp`](https://github.com/agentclientprotocol/claude-agent-acp)（curl 验证 `Location: https://github.com/agentclientprotocol/claude-agent-acp`），即项目从 Zed 组织移交给了 ACP 官方组织并更名。
- npm 两个包名并存：
  - [`@zed-industries/claude-code-acp`](https://www.npmjs.com/package/@zed-industries/claude-code-acp)：latest **0.16.2**，发布于 2026-02-17，包元数据最后更新 2026-03-26 —— **已停更的旧名字**；
  - [`@agentclientprotocol/claude-agent-acp`](https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp)：latest **0.85.1**（2026-10-02 发布），另有 `preview` dist-tag 跟随 main 分支每次 push 发布 —— **当前应使用的包**。

### 5.2 实现方式

- README 自述："This tool implements an ACP agent by using the official **Claude Agent SDK**"，即它是**官方 SDK 之上的 ACP 协议翻译层**，不是逆向 CLI。（来源：[仓库 README](https://github.com/agentclientprotocol/claude-agent-acp)）
- package.json 依赖（npm registry 一手数据）：
  - `@anthropic-ai/claude-agent-sdk: 0.3.287`（官方 TS SDK）
  - `@agentclientprotocol/sdk: 1.7.0`（ACP 官方 TS SDK）
  - `engines: { node: ">=22" }`；`bin: { "claude-agent-acp": "dist/index.js" }`
  - 无任何原生/native 依赖（无 node-pty；终端由 ACP 客户端按 `terminal/*` 方法代为创建）。

### 5.3 支持的 ACP 功能（源码级验证）

对发布的 0.85.1 tarball 内 `dist/` 做方法名扫描，实际注册/处理的 ACP 方法包括：`session/new`、`session/load`、`session/list`、`session/prompt`、`session/update`、`session/cancel`、`session/resume`、`session/replay`、`session/steering`（turn 中途追加用户消息）、`session/set_config_option`、`session/query`、`session/async_task`、elicitation 与 goal 扩展等，并声明 `loadSession: true` 能力。README 列出的用户侧功能：

- @-mentions 上下文、图片、**工具调用 + 权限请求**；
- 协商式 AIR diff-patch 扩展的紧凑文件变更展示、edit review、TODO 列表；
- 嵌套子 agent 会话/转录（需双侧能力协商）；
- 交互式与后台终端（经 ACP 客户端）；
- 自定义 slash 命令、客户端 MCP 服务器、`/mcp` 命令（含经 URL elicitation 的 MCP OAuth）；
- 会话失败/恢复、推荐配置等 AIR 扩展。

（来源：[README](https://github.com/agentclientprotocol/claude-agent-acp)、npm 包 tarball 检查、[AIR extensions 文档](https://github.com/agentclientprotocol/claude-agent-acp/blob/main/docs/air-extensions.md)）

**即：流式（session/update）、权限请求（session/request_permission）、会话管理（new/load/list/resume）均支持。**

### 5.4 维护状态

- 仓库：2608 stars、768 commits、最后 push **2026-10-02**（调研前 2 天）、未归档、Apache-2.0。（来源：[GitHub API](https://api.github.com/repos/agentclientprotocol/claude-agent-acp)）
- 发布节奏：main 每次 push 发 preview，稳定版高频迭代（0.16 → 0.85 半年内）。（来源：[RELEASES.md](https://github.com/agentclientprotocol/claude-agent-acp/blob/main/docs/RELEASES.md)）
- Zed 编辑器开箱内置此适配器（"New Claude Code Thread"）。（来源：旧包 README，npm `@zed-industries/claude-code-acp@0.16.2`）

### 5.5 启动方式

```bash
# 当前包（推荐）
npm install -g @agentclientprotocol/claude-agent-acp
ANTHROPIC_API_KEY=sk-... claude-agent-acp
# 等价于
npx @agentclientprotocol/claude-agent-acp
```

（bin 入口 `claude-agent-acp` 来自 package.json；全局安装 + env 用法来自旧 README，语义在新包一致）

---

## 6. Windows 支持评估（针对 crewvela：Tauri on Windows）

- **适配器本身**：纯 Node/TypeScript 包，无原生编译依赖；终端能力委托给 ACP 客户端（crewvela 作为 client 可以不实现 terminal/* 或自行实现）。硬性要求只有 **Node >= 22**。（来源：npm package.json）
- **Claude Code 侧**：官方支持**原生 Windows**（Windows 10 1809+ / Server 2019+），建议装 Git for Windows 以获得 Bash 工具（否则退回 PowerShell）；WSL 方案亦可。Agent SDK 在 Windows 上即驱动原生 Claude Code。（来源：[Setup 文档](https://code.claude.com/docs/en/setup)）
- **已知问题面**：搜索该仓库 issues 中 "windows" 关键词（54 个结果），仅 1 条真正 Windows 相关——[#1041](https://github.com/agentclientprotocol/claude-agent-acp/issues/1041)（WSL `/mnt/<drive>` 路径大小写敏感导致 session/list 匹配失败，2026-08-25）。**没有** "无法在 Windows 运行 / spawn 失败 / 必须 WSL" 类问题。（来源：[issue 搜索](https://github.com/agentclientprotocol/claude-agent-acp/issues?q=windows)）
- **风险点（如实陈述）**：CI（ci.yml）`runs-on: ubuntu-latest` 单平台，**Windows 不在上游测试矩阵**，出了平台问题只能提 issue 等社区修。（来源：[.github/workflows/ci.yml](https://github.com/agentclientprotocol/claude-agent-acp/blob/main/.github/workflows/ci.yml)）

**判断：在 Windows 上以 `npx @agentclientprotocol/claude-agent-acp` 方式随 Tauri 应用拉起子进程，技术上没有已知阻塞；但属于"社区适配器 + 未承诺 Windows"的双重非官方保证。**

---

## 7. 竞品参照：Gemini CLI 官方内置 ACP

- Gemini CLI 官方文档有专门的 [ACP Mode 页面](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/acp-mode.md)：`gemini --acp` 一条命令进入 ACP 模式，JSON-RPC 2.0 over stdio，实现位于 `packages/cli/src/acp/acpClient.ts`，并已进入 ACP Agent Registry。
- 对比：ACP 官方 agents 列表中 Gemini CLI 直接列出，Claude 需经适配器（见第 3 节）。（来源：[ACP agents](https://agentclientprotocol.com/get-started/agents)）
- 含义：主流编码 agent 向 ACP 收敛是明显趋势（Codex CLI 也是 via adapter；ACP 客户端生态已覆盖 Zed、JetBrains、VS Code 扩展、Neovim、移动端、IM 桥接等大量客户端，见 [clients 列表](https://agentclientprotocol.com/get-started/clients)）。Anthropic 未跟进原生 ACP，但其 Agent SDK 的能力模型（canUseTool、流式、resume）与 ACP 语义一一对应，适配器因此能长期稳定存在。

---

## 8. 对 crewvela 的可行路径与风险

**推荐路径（得到"ACP 语义的 Claude Code 聊天会话"）**：

1. crewvela（Tauri 主进程/Rust 侧或 sidecar Node）以子进程启动 `npx @agentclientprotocol/claude-agent-acp`（需预装 Node >= 22；或把适配器作为依赖打进 sidecar）。
2. 通过 stdio JSON-RPC 实现 ACP 客户端：`initialize` → `session/new` → `session/prompt`，消费 `session/update` 流式渲染，响应 `session/request_permission` 做权限 UI，`session/load`/`session/list` 做会话恢复。
3. 认证用 `ANTHROPIC_API_KEY`（SDK 场景官方要求 API key 而非 claude.ai 登录，见 [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview)）；模型等配置走适配器的 [model-configuration](https://github.com/agentclientprotocol/claude-agent-acp/blob/main/docs/model-configuration.md)。

**备选路径**：不经 ACP，直接用官方 `@anthropic-ai/claude-agent-sdk` 的 `query()` + `canUseTool` + `resume`（能力是 ACP 的超集来源，且是 Anthropic 一方保证）；ACP 的价值仅在于协议标准化与客户端生态复用。

**风险与限制**：

- 适配器是社区项目（虽已入 ACP 官方组织），版本迭代快（0.x），协议字段可能随 ACP v2 演进变化；
- Windows 不在其 CI 矩阵内；
- 依赖链两层（适配器 → Agent SDK → Claude Code CLI），版本兼容需跟着适配器走（它锁定的 SDK 0.3.287 需与本机 claude CLI 兼容）；
- 若 Anthropic 未来官方支持 ACP（issue #24411 仍开放），可平迁，ACP 客户端侧代码不用改。

---

## 9. 关键来源一览

| 主题 | 来源 |
|---|---|
| ACP 协议介绍 | https://agentclientprotocol.com/ |
| ACP v1 规范（方法清单） | https://agentclientprotocol.com/protocol/v1/overview |
| ACP 文档索引（v1/v2/RFD） | https://agentclientprotocol.com/llms.txt |
| ACP 官方 agents 列表（Claude via adapter） | https://agentclientprotocol.com/get-started/agents |
| ACP 客户端列表 | https://agentclientprotocol.com/get-started/clients |
| Claude Code 文档站 sitemap（无 ACP 页面） | https://code.claude.com/sitemap.xml |
| Agent SDK overview（无 ACP 提及） | https://code.claude.com/docs/en/agent-sdk/overview |
| Agent SDK TS API（canUseTool/resume） | https://code.claude.com/docs/en/agent-sdk/typescript |
| Headless / --print / stream-json | https://code.claude.com/docs/en/headless |
| CLI 参考（--input-format stream-json） | https://code.claude.com/docs/en/cli-reference |
| 系统要求（原生 Windows） | https://code.claude.com/docs/en/setup |
| anthropics/claude-code CHANGELOG（无 ACP） | https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md |
| ACP 功能请求 issue（open，无官方回应） | https://github.com/anthropics/claude-code/issues/24411 |
| 适配器仓库（zed-industries/claude-code-acp 301 迁移至此） | https://github.com/agentclientprotocol/claude-code-acp → https://github.com/agentclientprotocol/claude-agent-acp |
| 适配器 README / AIR 扩展 / 发布说明 | https://github.com/agentclientprotocol/claude-agent-acp |
| 适配器 npm 包（0.85.1，2026-10-02） | https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp |
| 旧包 @zed-industries/claude-code-acp（0.16.2 停更） | https://www.npmjs.com/package/@zed-industries/claude-code-acp |
| 适配器 Windows 相关 issue 搜索 | https://github.com/agentclientprotocol/claude-agent-acp/issues?q=windows |
| 适配器 CI（仅 ubuntu） | https://github.com/agentclientprotocol/claude-agent-acp/blob/main/.github/workflows/ci.yml |
| Gemini CLI ACP 模式官方文档 | https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/acp-mode.md |

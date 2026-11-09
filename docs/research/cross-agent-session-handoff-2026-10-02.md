# CrewVela 跨 Agent 会话接力调研

日期：2026-10-02。范围：当前 CrewVela 工作区源码、开源项目的一手文档和关键实现；本轮只读分析并新增本报告。

## 结论

**需求合理，CrewVela 已具备多项基础能力，建议在现有实现上补齐上下文接续。** 产品行为可以定义为：用户一直留在同一个可见聊天，点击“切换到 Codex 并继续”，CrewVela 停止 Claude 的执行，让实际 Codex Runtime 在原工作目录接手。底层允许创建新的 Codex 原生会话，保留 Claude 原始历史和两者的关联。

当前 CrewVela 已有实际 Runtime 启动、协作成员 Runtime 更换、显式交接、Git/终端证据采集、任务改派、共享记忆和跨 Runtime 任务 Prompt。缺口集中在：自动交接完整可用上下文、普通聊天的连续身份、原生历史导入，以及将这些能力串成一次操作。

最接近 Claude Code→Codex 这一具体场景的开源方案是 OpenAI 官方 `codex-plugin-cc` 的 `/codex:transfer`：把 Claude 对话转换成可继续的 Codex 线程。它不承诺所有内部状态无损延续，也不等于任意工具之间反复热切。Windows 有公开兼容性问题，且当前开发分支能力不等于已发布版本能力。[官方项目](https://github.com/openai/codex-plugin-cc)、[Windows 问题报告](https://github.com/openai/codex-plugin-cc/issues/618)

开源免费指宿主/迁移工具的代码许可和本地使用；Claude、Codex 的账号订阅额度、API 或模型费用仍分别适用。工具不会把用完的 Claude 额度变成可继续使用的额度。[官方插件说明](https://github.com/openai/codex-plugin-cc)

## 调研判断标准

- **实际 Agent 切换**：Claude Code 与 Codex 使用各自真实执行工具、认证和会话，而非同一个 Agent 框架仅换模型供应商。
- **上下文可继续**：接手者能获得需求、决策、完成项、剩余项、工作目录、未提交改动和验证结果；完整原始记录仍可回看。
- **同一个可见聊天**：界面历史连续，明确显示由谁执行；原生 session/thread ID 可以改变。
- **额度耗尽仍可交接**：基础交接不依赖来源 Agent 再调用一次模型；来源 CLI 无法接收命令时，宿主仍可读已有本地记录。
- **运行态边界清楚**：历史文本、工具结果、工具进程、待审批状态与原生内部状态分别判断，不能由“支持多 Agent”推断“支持迁移”。

## 开源候选对照

表中“未证实”表示在已核验文档与实现范围内，没有找到该能力的证据；不是断言项目所有分支永久不支持。候选均未在本机安装或实际迁移。

| 项目 | 真实能力与接力方式 | 与需求的匹配 | 许可与主要边界 |
| --- | --- | --- | --- |
| [OpenAI codex-plugin-cc](https://github.com/openai/codex-plugin-cc) | `/codex:transfer` 将当前 Claude transcript 导入 Codex 线程，可用 Codex App/TUI 继续。 | **最接近 CC→Codex 的现成方案**。底层成为 Codex 原生线程，界面未必仍是原 Claude 界面。 | Apache-2.0；需本机 Codex 登录和支持导入的 CLI；Windows 路径/导入记录问题有公开报告；转换有损，详见后文。 |
| [claude-codex-bridge](https://github.com/AndrewAvery7/claude-codex-bridge) | 基于官方 importer，增加会话选择、去重和打开 Codex App；独立 PowerShell launcher 可绕开 Claude 命令入口发起。 | 对“Claude 已不能再接收输入”有参考价值。 | MIT；README 声称验证 Windows/macOS/Linux，未由本轮复现；依赖导入 ledger、数据库和深链等细节，升级兼容性需验证。[设计与测试文档](https://github.com/AndrewAvery7/claude-codex-bridge/tree/main/docs) |
| [codex-claude-transfer / cct](https://github.com/ahmojo/codex-claude-transfer) | 本地导出/导入两边会话，支持跨格式文本转换；提供终端向导及本地浏览器入口。 | 适合迁移/备份与格式转换参考，**不是同聊天一键热切**。 | MIT；提供 Scoop；依赖本地会话格式，项目明确跨 Agent 的工具调用、输出、运行态和 provider ID 不会原样保留；导入后可能需重启工具扫描。[兼容说明](https://github.com/ahmojo/codex-claude-transfer/blob/Main/README.md) |
| [ai-sync](https://github.com/Oreolion/ai-sync) | 项目内 HANDOFF/PLAN/PROGRESS 等文件传递摘要与进度，Claude 插件/Codex 指令读共享状态。 | 可作为通用交接包参考，保留业务状态，不是原生聊天历史迁移。 | MIT；早期项目，Windows 专项证据有限；突然耗尽额度时取决于此前记录是否及时保存。[许可](https://github.com/Oreolion/ai-sync/blob/main/LICENSE) |
| [Continuum](https://github.com/AnasNafees1802/continuum) | `.aicontext` 保存状态、任务和决策；项目声称 hooks 自动维护，并能结合 transcript/Git 重建遗漏交接。 | 自动维护工作状态的设计与本需求相关，仍是新会话读共享状态。 | MIT；提供 Windows PowerShell 安装，hooks/parser 效果是项目说明，未由本轮实测；长期维护与复杂项目正确性需验证。[许可](https://github.com/AnasNafees1802/continuum/blob/main/LICENSE) |
| [Zed External Agents](https://zed.dev/docs/ai/external-agents) | 通过 ACP 在同一个 IDE 使用 Claude、Codex 等外部 Agent 线程。 | 能统一工作台；未证实把 Claude 外部线程自动转为 Codex 外部线程。 | 开源编辑器；外部 Agent 使用各自认证/费用；不要把多个线程或摘要建新线程理解为原生历史迁移。[Agent Panel](https://zed.dev/docs/ai/agent-panel) |
| [Vibe Kanban](https://github.com/BloopAI/vibe-kanban) | 为 Claude Code、Codex 等启动任务尝试，采用 worktree 管理执行。 | 任务编排与分工有参考价值；未找到跨 Agent 原生 transcript 自动转换证据。 | Apache-2.0；工具本地运行，模型另计；侧重任务尝试与工作树，不是当前聊天无缝接续。[支持的 Agent](https://github.com/BloopAI/vibe-kanban/blob/main/docs/supported-coding-agents.mdx)、[许可](https://github.com/BloopAI/vibe-kanban/blob/main/LICENSE) |

另行筛查：AionUi 支持多种 Agent，但项目说明不同会话的 context/history 独立；OpenCode 切模型供应商并不等于切到 Claude Code/Codex 原生 Runtime；HAPI 的同 Agent 会话在终端/Web/手机恢复能力，也不能直接证明跨 Agent 迁移。因此不将它们作为满足本需求的证据。[AionUi](https://github.com/iOfficeAI/AionUi)、[OpenCode providers](https://opencode.ai/docs/providers/)、[HAPI Agent 支持矩阵](https://github.com/tiann/hapi/blob/main/docs/guide/agents.md)

ACP 统一了会话建立/恢复和 prompt/tool 事件，但 session ID 由接入的 Agent 管理；已核验的协议并没有定义将 A 的原生历史与运行态自动转换成 B。CrewVela 使用 ACP 的代码基础可以复用，历史迁移仍需宿主或明确的转换器完成。此处是依据协议范围作出的判断。[ACP 会话建立](https://agentclientprotocol.com/protocol/v1/session-setup)、[ACP Prompt Turn](https://agentclientprotocol.com/protocol/v1/prompt-turn)

## CrewVela 当前工作区代码审计

审计对象是 `.` 的当前磁盘源码，分支 `kimicode`，HEAD `6403730ceec4b910b1986128ff30a24145df8794`。相关模块存在用户及其他任务的未提交改动，因此下述是“当前工作区已有实现”，不能据此认定已发布版本或正在运行的安装包具有相同行为。先使用仓库已有 CodeGraph，再核对未展示的源码及前后端调用；本轮没有启动、停止或切换任何用户 Agent，也没有修改业务源码。

### 审计结论

**已经具备相当一部分基础能力，但还不能等同于“在当前聊天里无缝从 Claude Code 切到 Codex”。** 当前实现有三条相关路径：修改协作成员 Runtime；现有交互会话之间的显式交接；项目任务改派并注入交接指令。它们解决运行时选择、执行所有权和部分业务上下文的延续，尚未串成一个稳定逻辑聊天中的自动上下文接续。

| 所需能力 | 当前证据与实际行为 | 判断 |
| --- | --- | --- |
| 启动实际 Claude Code / Codex 编码 Runtime | `open_runtime_session_with_yolo` 根据 Runtime 解析可执行文件，在会话 cwd 启动；任务执行器按 `agent.runtime_kind` 选择不同 adapter。[启动路径](/./src-tauri/src/lib.rs:3012)、[适配器选择](/./src-tauri/src/multi_agent/executor.rs:1368) | 已有。不是单纯换模型名。 |
| 给同一协作成员更换 Runtime | 前端有 Runtime 选择；活动任务/繁忙终端会拒绝修改。保存后保留成员 ID，旧会话退役归档，待发消息标为失败，创建并绑定新会话。[编辑说明](/./src/components/CollaborationAgentEditor.tsx:44)、[后端切换](/./src-tauri/src/lib.rs:1076)、[活动 Run 限制](/./src-tauri/src/multi_agent/store.rs:171) | 已有，但仅针对协作成员，且切换没有自动生成交接包。 |
| 保留旧会话供查阅 | `retire_agent_runtime_sessions` 将旧会话设为只读历史、归档并停止；新会话 UUID 与旧会话不同。[退役](/./src-tauri/src/sessions.rs:879)、[新会话](/./src-tauri/src/sessions.rs:700) | 保留历史记录，并非把旧原生会话变成 Codex 会话。 |
| Claude 会话向 Codex 会话交接 | 协作设置有“交接工作”；会话模式要求来源与目标均为可交接且正在运行的现有会话。确认后核验目标身份、停止来源进程树，再持久化排队交接文本。[入口](/./src/components/CollaborationSettingsPanel.tsx:420)、[类型与填写项](/./src/components/CollaborationSupportPanel.tsx:154)、[预览](/./src-tauri/src/collaboration_support.rs:244)、[确认](/./src-tauri/src/collaboration_support.rs:320) | 已有显式跨会话交接，仍需选另一现有会话。不是一个聊天内的原地切换。 |
| 自动收集交接证据 | 会话交接采集来源/目标目录、`git status --short`、`git diff --stat`、最近提交和原终端最近最多 12000 字节；用户填写目标、进度、剩余、阻塞、文件和验证结果；总交接包最多 32000 字符。[Git 采集](/./src-tauri/src/collaboration_support.rs:85)、[交接文本](/./src-tauri/src/collaboration_support.rs:107)、[采集与限额](/./src-tauri/src/collaboration_support.rs:252) | 部分自动。没有读取并转换完整原生对话，也没有自动提取完整开发决策；终端尾部会截断。 |
| 同一任务让另一成员继续 | 任务模式采集项目/任务/验收、Run、产物、日志和 Git；确认会暂停整个项目，核验执行已退出，改派负责人并保存 queued 人工指令，项目保持暂停，用户需继续项目。[任务采集](/./src-tauri/src/collaboration_support.rs:266)、[停止与改派](/./src-tauri/src/collaboration_support.rs:352)、[事务保存](/./src-tauri/src/multi_agent/store.rs:2907) | 已有业务任务接力。活动 ACP Run 缺乏可验证停止证据时会拒绝交接，不应称作覆盖所有执行通道。 |
| 接手 Agent 获得项目上下文 | `RuntimeRequest` 已包含依赖产物、人工指令、共享记忆和协作消息；下次执行加载这些数据，`render_task_prompt` 为不同 Runtime 构造项目目标、任务、验收、工作目录、产物摘要、记忆和反馈。[加载](/./src-tauri/src/multi_agent/executor.rs:1344)、[统一任务 Prompt](/./src-tauri/src/multi_agent/executor.rs:3553) | 已有可复用的任务上下文拼装器；业务状态延续不等于完整聊天历史延续。 |
| 一个界面向多个 Agent 发消息 | 主聊天支持 @ 定向调用；团队消息页聚合持久项目消息、运输证据和 Run 摘要。[定向发送](/./src/components/MultiAgentControl.tsx:2054)、[团队历史](/./src/teamMessages.ts:26) | 已有统一入口。定向路径传当前消息；主聊天最近十条历史用于协调器解读，不会自动变成新 Runtime 的完整上下文。[协调器历史](/./src/components/MultiAgentControl.tsx:2147) |
| 完整的跨 Runtime 聊天历史 | `read_transcript` 专门解析 Claude JSONL；项目成员的 appSession 与 Runtime 绑定，新 Runtime 创建新 ID；未在已审计切换/交接路径看到 Claude→Codex 原生历史转换或统一连续对话投影。[Claude 回看](/./src-tauri/src/transcript.rs:116)、[会话结构](/./src-tauri/src/sessions.rs:331) | 这是核心缺口。代码中有其他消息/事件存储，不代表它们已串成用户需要的连续聊天。 |

### 使用现有能力，今天能做到什么

1. **普通交互会话**：在同一个开发目录（若原会话使用 worktree，就必须是同一个 worktree）创建并运行 Codex，进入协作设置“交接工作”，选择原 Claude 与 Codex 会话，填写目标和剩余步骤，核对预览并确认。实现会停掉 Claude 并把交接包排到 Codex。来源已退出或目标未运行时，当前预览路径会拒绝；入队也不表示 Codex 已理解或完成接手。
2. **项目任务**：使用“项目任务 → 另一成员”把同一任务交给 Codex 成员。该操作影响整个项目执行，需要核对停止范围；交接后项目保持暂停，需用户继续。交接包是接手者下次执行的人工指令。
3. **仅编辑 Runtime**：适合改变该成员后续使用的工具，但不要期待旧聊天自动带过去。它与“交接工作”目前是两个独立路径，旧收件箱待发消息不会自动迁移。

这些流程是源码所表达的预期行为，本轮未通过真实账号和原生 Tauri 运行实测。

### 最小差距与建议

**产品目标建议定为：同一个可见聊天，允许多个不同工具的原生会话，由 CrewVela 负责上下文接续。** 优先让用户手动选择“切换到 Codex 并继续”；额度检测自动切换可放到后续，不应扩大本轮范围。

需要补齐的能力：

- **稳定的逻辑聊天身份与历史**：保留可见聊天及完整历史，记录每次 Runtime 附着/切换事件；原生 Claude sessionId、Codex threadId 与应用会话 ID 分开保存。可复用现有项目/成员历史，但不能仅更改 Runtime 字段就宣称上下文已迁移。
- **宿主生成交接包**：在用户点击切换时，自动提取明确的需求、约束、已定决策、任务状态、相关文件、验证结果和最近必要对话，附真实 Git/工作树证据；由 CrewVela 本地拼装基础包，必要时交给接手 Agent 阅读已有记录。不要要求额度已用完的 Claude 再付费生成最后一份总结。完整原始历史保留供追溯，默认发送精简工作上下文，避免每次重复灌入全部日志。
- **单执行者交接**：复用现有生命周期锁、进程树停止核验、持久化交接状态和幂等入队；切换失败留在可恢复状态，不能让两个 Agent 同时改同一个工作目录。
- **精确保持当前工作区**：延续实际 cwd、worktree 和未提交改动。成员 Runtime 切换当前创建新会话时使用项目 root；因此将它扩展为普通会话接力时必须显式保留原会话工作树，不能把项目根目录当作等价替代。
- **接手状态可验证**：界面区分“已生成交接包”“已入队/已送达”“Codex 已确认接手”。现有运输状态主要证明文本或提交动作，不应直接映射为上下文已理解。
- **同工具恢复与跨工具接力分开**：已存在按任务读取 locator 的恢复机制。进一步实现跨 Runtime 切换时，应核对 locator 的 Runtime/原生会话身份与工作目录，只在兼容的原工具恢复，切到另一工具则新建原生会话并注入工作上下文。[locator 读取](/./src-tauri/src/multi_agent/store.rs:1900)、[目录校验](/./src-tauri/src/multi_agent/executor.rs:1463)

此处为设计建议和差距分析，不是实施计划，也没有新增代码。

### 验证范围

- 已验证：当前源文件、前端调用与 Tauri 命令注册、Runtime 更换路径、交接包采集内容、停止/持久化投递路径、任务指令保存与 Prompt 注入路径。
- 未验证：正在运行的 CrewVela 安装包是否包含这些未提交实现；真实 Claude/Codex 登录及额度；真实原生进程停止、接手回复、工作树恢复；任何候选项目的实际安装与端到端切换。
- 本轮只新增调研报告；没有改业务源码、安装工具、改账号配置或进行真实交接。

## 官方导入机制的额外源码核验

主智能体补充核验了 OpenAI 官方仓库的当前源码：`openai/codex` main = `ca466061d64f0b44f416135c7fd06aa7af850bbc`，`openai/codex-plugin-cc` main = `db52e28f4d9ded852ab3942cea316258ae4ef346`。以下描述只证明该源码快照的机制，不能代替已发布版本和本机运行验收。

- 官方 `/codex:transfer` 命令设置 `disable-model-invocation: true` 并运行本地 companion 脚本，说明其设计不要求再向 Claude 模型请求一次总结。[命令定义](https://github.com/openai/codex-plugin-cc/blob/db52e28f4d9ded852ab3942cea316258ae4ef346/plugins/codex/commands/transfer.md)
- 当前 Codex converter 读取主线 user/assistant，排除 meta 和 sidechain；工具调用/结果转换为可读文本，工具结果会截断，thinking 被跳过，未支持的多模态块会变成提示标记。这是“可继续的已转换对话”，不是所有原生记录无损搬迁。[主线选择](https://github.com/openai/codex/blob/ca466061d64f0b44f416135c7fd06aa7af850bbc/codex-rs/external-agent-migration/src/sessions/records_cla.rs#L163-L210)、[内容转换](https://github.com/openai/codex/blob/ca466061d64f0b44f416135c7fd06aa7af850bbc/codex-rs/external-agent-migration/src/sessions/records_common.rs#L12-L55)
- 当前 main 已对唯一映射且内容变化的源会话准备 `SessionImportTarget::Existing`，由 app-server 调用 `append_existing_session`；因此不能把旧版本 issue 中“只能导入一次”推广到所有当前实现。[目标选择](https://github.com/openai/codex/blob/ca466061d64f0b44f416135c7fd06aa7af850bbc/codex-rs/external-agent-migration/src/sessions/mod.rs#L105-L130)、[app-server 调用](https://github.com/openai/codex/blob/ca466061d64f0b44f416135c7fd06aa7af850bbc/codex-rs/app-server/src/external_agent_migration/session_importer.rs#L278-L315)
- 官方追加路径要求目标非活动、非归档、历史未分叉，且目标模型可见对话必须是来源的精确前缀；它并不是任意 Claude↔Codex 双向同步器。[追加校验](https://github.com/openai/codex/blob/ca466061d64f0b44f416135c7fd06aa7af850bbc/codex-rs/external-agent-migration/src/sessions/append.rs#L23-L66)、[前缀判断](https://github.com/openai/codex/blob/ca466061d64f0b44f416135c7fd06aa7af850bbc/codex-rs/external-agent-migration/src/sessions/append.rs#L158-L185)
- 本机只读 `codex --version` 返回 `codex-cli 0.159.3`；未调用 import API，也未触碰本机真实历史、认证或账号额度，因此该版本是否包含上述 main 机制仍待针对性验证。
- CrewVela 当前 `src`、`src-tauri/src`、依赖清单未发现 `externalAgentConfig`、`importExternalAgentSession` 或 `external_agent_session_imports` 接入；这一官方迁移路径是可研究的新增接入点。

**据此调整实现方向（建议，不是已开发功能）**：Claude→Codex 可优先验证官方导入 API，保留转换后的历史，再补当前任务状态和真实工作树证据；其他方向或其他 Runtime 使用通用交接包。CrewVela 继续负责同一可见聊天、来源记录、单执行者切换和恢复。无需在第一版自建全部工具的原生文件格式转换器。

## 建议的实施顺序

建议先做一个范围明确的 CC→Codex 接力，而不是先实现所有 Runtime 的双向原生会话转换：

1. **验证官方导入链路**：用专门的测试会话验证本机 Codex 的导入能力、Windows 路径、导入后继续对话，以及来源进程已退出/额度不足时由宿主发起的路径。使用官方接口，避免直接写 Codex SQLite/rollout 文件。验证失败应展示原因并保留可恢复状态。
2. **补业务交接包**：复用现有 Git、任务、产物、共享记忆与验证信息；明确保持来源实际 cwd/worktree。即使原生历史已导入，仍补充当前待办和真实工作树状态。
3. **串成同一聊天中的切换操作**：逻辑聊天关联多个原生会话，沿用现有停止核验、幂等投递和持久化交接记录；旧历史继续可读，显示切换事件与接手结果。
4. **扩展其他方向**：Claude→Codex 之外先采用通用工作上下文包。需要反复切回时再分别验证对应方向的导入/恢复支持，不能假设第一次导入已提供双向同步。

第一版的合理验收是：在测试项目里由 Claude 完成一部分工作，模拟其不可再调用模型，点击切换；Codex 在相同实际工作树获取必要上下文，正确说明已完成/剩余步骤，并继续完成一个可验证的小任务；聊天历史可追溯，重复点击不重复启动或重复投递，来源与目标不会同时写同一目录，失败可恢复。

本轮没有实施上述功能，也没有迁移真实会话。当前报告是选型与代码能力评估；需要真实 Runtime 验收后，才能宣称“一键接力”已实现。

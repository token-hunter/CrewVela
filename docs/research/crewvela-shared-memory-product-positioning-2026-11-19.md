# CrewVela 的共享记忆与产品方向

2026 年 11 月 19 日。面向维护者的判断：**值得继续做，先争取那些已经交替使用 Claude Code 和 Codex、会在同一个代码库工作几周以上的人。** 他们可以自己装记忆服务。CrewVela 要帮他们省下的，是每次换人时重新解释项目、重新排除旧方案，以及最后亲自核实「到底做完没有」的时间。能否省下来，需要和用户现有做法同题比较。

## 一、共享记忆是已经存在的差异

相对于本次核查的 Orca，CrewVela 已经把跨 CLI 项目记忆的安装、项目接入、工具检查、故障恢复和 Wiki 入口做进了桌面产品。Orca 的原生工作流能搜索多个 Agent 的历史、传递交接上下文、用任务和消息协调工作，也尊重 CLI 自己的记忆文件。我没有在检查的 Orca 主线实现中找到与 CrewVela 等价的共享记忆配置流程。用户可以在其他工作台的 Claude/Codex 中另外配置记忆工具，所以这个差异体现为开通成本、项目路由和可验证性，很容易被其他产品追上。[CrewVela 快速接入](../shared-memory-quick-setup.md) · [Orca 记忆文件](https://github.com/stablyai/orca/blob/cdf90d9c72e40190fafc6cb343b601c6b482c88d/docs/site/content/docs/agents/hooks-memory.mdx) · [Orca 历史搜索](https://github.com/stablyai/orca/blob/cdf90d9c72e40190fafc6cb343b601c6b482c88d/skill-guides/orca-cli.md#agent-session-search)

### 用户遇到的两种“记忆”

| | ai-memory 提供的项目长期记忆 | CrewVela 工程任务里的协作记忆 |
|---|---|---|
| 一件具体的事 | Claude 把项目决策写入一页，另一个接入相同项目的客户端之后能查到、读到。 | 开发成员在任务中记录一个发现。后续工程成员能在相应项目、权限和运行状态允许时得到它。 |
| 如何进入系统 | 快速向导只给所选 CLI 配置 Hooks 和 MCP，并创建或校验项目标记。Hooks 可采集受支持的生命周期事件，MCP 提供显式读写。上游还可在符合条件的会话开始时提供交接提示，项目规则摘要有单独的开关。 | Agent 通过 `cc_memory_put` 写入。后端绑定项目、任务和运行 ID，写入 CrewVela 的 SQLite 表。带来源运行的记录初写时为待确认，运行成功收尾会提升记录，同时写入本次结果摘要。 |
| 下一位怎样拿到 | Agent 能主动查、读；符合条件的启动钩子也能递送上下文。是否发生要看客户端、钩子信任、项目坐标和配置。CrewVela 的接力界面能提示待接交接记录，让接手 Agent 消费。 | 普通工程任务构建请求时最多取 12 条可见记录。当前查询为空，按已验证标志、置信度和更新时间排序，再写进工程提示。公共讨论跳过这次自动装载。项目档案另有按来源搜索和引用。 |
| 用户可核实什么 | 可分别检查服务工具、MCP 注册、钩子配置和项目捕获，打开本地 Wiki 检视内容。服务健康和历史捕获都不能单独证明本次回答采用了那条记忆。 | 底层有来源任务和运行字段，可追溯参与者及结果。普通工程提示展示命名空间、键、内容和置信度，尚未直接展示每条记忆关联的提交、测试或复核状态。 |

两边各有存储和入口。沿当前读写路径没有看到两者逐条双向同步的实现。不能把 CrewVela 内部记录的状态当成 ai-memory Wiki 页已发布，也不能把上游可选的托管启动、全文交接或远端模型配置直接算作 CrewVela 默认功能。[本地向导](../../src-tauri/src/ai_memory_onboarding.rs#L679) · [共享记忆设置](../../src/components/SharedMemorySettingsPanel.tsx) · [会话接力入口](../../src/components/SessionSwitchDialog.tsx#L26) · [协作工具](../../src-tauri/src/multi_agent/bridge.rs#L473) · [工程任务注入](../../src-tauri/src/multi_agent/executor.rs#L2149) · [工程提示](../../src-tauri/src/multi_agent/executor.rs#L5436) · [上游会话启动语义](https://github.com/akitaonrails/ai-memory/blob/411d90d1ede070d874b1d5039b0ba0a08764eb49/docs/ARCHITECTURE.md#L316)

### 三处直接影响可信度的细节

1. **运行成功可能先于人工验收。** 普通工程路径允许任务进入 `in_review`、创建待审批记录，随后把来源运行的记忆置为 `verified`。因此产品里“已验证”的语义值得优先拆清。可以分别表达“成员已记录”“测试有证据”“审查已通过”；一个测试通过也只证明它覆盖的行为。只要待审结论提前出现在下一位的提示里，更多记忆就可能让错误传得更快。[任务状态与记忆提升](../../src-tauri/src/multi_agent/store.rs#L2696) · [提升发生的位置](../../src-tauri/src/multi_agent/store.rs#L2875)
2. **查得到和用得对有距离。** 现有 12 条自动装载并未按当前任务提问。一个经常更新的前端经验可能挤掉与本次数据库修复有关的旧决定。优先尝试用当前任务、前置交付和改动文件筛选少量候选，同时显示来源；测出比现有排序更有用，再考虑向量检索或复杂排名。[可见性与排序](../../src-tauri/src/multi_agent/store.rs#L1498) · [取数入口](../../src-tauri/src/multi_agent/executor.rs#L2149)
3. **同仓库的不同目录值得专门验。** 向导生成的新项目坐标含目录路径摘要，并保留已有标记。独立 worktree、第二份 clone、同名但无关的项目，不应全按“名字相同”处理。首轮验收应包括同项目预期共享、不同项目默认不串，以及哪个项目会收到纠正。上游允许显式跨项目检索，默认查询隔离不等于凭证层禁止所有跨项目访问。源码快速向导覆盖 Windows x64 和两种架构的 Mac、Claude Code 和 Codex；其他接入路径的覆盖要分别验证。已有记录仍把 Mac 原生首次启用和真实 Agent 捕获列为待验收。[项目标记](../../src-tauri/src/ai_memory_onboarding.rs#L426) · [桥接作用域](../../scripts/ai-memory-mcp-bridge.mjs) · [接入与既有验证](../shared-memory-quick-setup.md)

本地运行也是有条件的：默认本地存储、可用本地检索；若用户配置云端模型或远端 embedding，上游会按配置发送相应内容。产品文案适合明确显示当前配置的目的地。[上游数据处理说明](https://github.com/akitaonrails/ai-memory/blob/411d90d1ede070d874b1d5039b0ba0a08764eb49/DATA_HANDLING.md)

## 二、这些产品分别在哪些场景已经很好用

| 用户要完成的工作 | 已有替代品 | 对 CrewVela 的影响 |
|---|---|---|
| 让主 Agent 同时指挥几个执行者，甚至在另一台机器上开工 | Orca 有持久任务、派工、收件箱、决策关卡和远程执行。另有项目级历史搜索和从旧会话交接到新会话的入口。 | 仅靠更多终端、分屏、广播或“支持多模型”很难说服用户迁移。[Orca 编排](https://github.com/stablyai/orca/blob/cdf90d9c72e40190fafc6cb343b601c6b482c88d/docs/site/content/docs/cli/orchestration.mdx) |
| 从主对话跨 provider 委派任务，按需选模型、角色、执行环境 | T3 Code 已有编排 MCP 和 `delegate_task`。另有第三方项目记忆实现，能跨线程和 provider 记住、检索、纠正、删除项目条目；插件仓库明确要求原型宿主 fork，采用主动词法检索。 | 跨模型派工有直接竞品，连可纠正的共享记忆也已有公开实现。CrewVela 正式入口是否更易用、是否更可靠，值得单独测。[T3 编排](https://github.com/pingdotgg/t3code/blob/c350ee7e15d0f993b406d243d2b4685b10a437e0/docs/orchestration-v2/orchestrator-mcp-server.md) · [第三方插件的宿主要求](https://github.com/digitalxero/t3-project-memory/blob/7b0f68a3e848e434aa43c2ceab20b3932f7510f2/README.md) |
| 团队以工单管理开发、审查、退回和 QA | Multica 的 Delivery 工作流已经能把开发交给一个 Agent、审查交给另一个，并在驳回后退回；项目描述会进入相关任务。Hermes 有按 Agent、运行环境和 profile 保存的持久记忆，支持配置的 Agent 还可共用工作区 MCP 服务。 | “有角色、有审查、有共享上下文”在这里都不是空白。按工单运营的团队留在 Multica 很合理。CrewVela 可以服务想从本地原会话直接讨论、临时找人、再形成工程分工的人。[Multica 工作流](https://github.com/multica-ai/multica/blob/e18fe48271f1be0d642d64ea7b419c4f676b3c09/apps/docs/content/docs/workflows.mdx) · [项目上下文](https://github.com/multica-ai/multica/blob/e18fe48271f1be0d642d64ea7b419c4f676b3c09/apps/docs/content/docs/projects.mdx) · [Hermes 记忆](https://github.com/multica-ai/multica/blob/e18fe48271f1be0d642d64ea7b419c4f676b3c09/CLI_AND_DAEMON.md#hermes-agent-memory) · [共享 MCP](https://github.com/multica-ai/multica/blob/e18fe48271f1be0d642d64ea7b419c4f676b3c09/apps/docs/content/docs/agents-create.mdx) |
| 只想让两个 CLI 记住同一项目 | 直接使用 ai-memory 就能获得共用项目页与交接能力。更简单的基线是一份项目说明和一份交接 Markdown。 | 这是最重要的对照组。如果同样顺手、同样准确，用户就没有为这件事更换工作台的理由。CrewVela 增加的价值应体现在换人、验收、修正和下一次复用上。[ai-memory 上游](https://github.com/akitaonrails/ai-memory/tree/411d90d1ede070d874b1d5039b0ba0a08764eb49) |

**公开反馈解释了用户为什么在意这件事。** Orca 有用户说，已经能搜聊天，但仍要自己重建当初的决定、失效的结论和未解决的问题。T3 的讨论有人展示了自己的实现，也有人要求知道一条记忆从哪里来、怎样纠正。Multica 的用户分别要求跨工单复用经过验证的经验，以及让多个 Agent 在同一段公开讨论里互相回应。这些反馈足以支持做一次用户试验，不能推出市场规模、付费意愿或竞品一定没有对应能力。[Orca 的原始需求](https://github.com/stablyai/orca/issues/23610) · [T3 的讨论](https://github.com/pingdotgg/t3code/discussions/15858) · [Multica 跨工单记忆](https://github.com/multica-ai/multica/issues/838) · [Multica 群聊需求](https://github.com/multica-ai/multica/issues/3964)

## 三、我建议先做的一个体验

**让用户随时换 Agent，到第二天还能继续，而且能看出接手者依据什么在做。** 先面向已在同一个仓库长期混用两个 CLI 的独立开发者和小团队。偶尔问一次问题、从不换 Agent 的人，用单 CLI 往往更省事。依赖远程工人或已有严格工单制度的团队，也没有迫切理由迁移。

用一个普通的 bug 就能讲清楚。假设导出功能把带前导零的客户编号截短了。Claude 复现了问题，定位到 CSV 序列化，留下一条失败测试，做到一半额度用完。用户换到 Codex 后，希望它能找到**哪个样例失败、业务为什么要保留零、哪个方案已经排除、现在该跑哪条测试**。修完交给 Claude 审查时，希望它验证同一个样例。下周改批量导出时，相关约束应重新出现；若产品已经改了规则，就能找到替代它的新决定。这是可复现的产品验收情境，尚未作为本次实测结果。

| 优先级 | 接下来只解决一件什么事 | 怎样判断成功 |
|---|---|---|
| 先做 | 在现有“接力”入口带上最近失败的测试、当前改动、未做事项和少量带来源的项目决定。复用工程交付、运行记录、项目档案和 ai-memory。明确显示接手者实际回报了什么。 | 用户无需重述前情；接手者的第一次有效操作是复现或推进未完成工作。接手失败时能看见卡在哪一步。 |
| 随后做 | 把一条“曾经这样做过”的记录连到具体证据。标出提出者、来源运行、审查状态和替代它的新决定；新旧冲突时先核对源码或测试。允许用户纠正一次，并知道下轮用了哪个版本。 | 待审观点不会以已批准规则出现；注入一条过时规则，接手者能识别、解释并采用当前版本，或明确请求裁决。 |
| 再验证 | 让用户决定何时值得多请一个 Agent。小改动可由一个人直接完成；行为有争议时点名第二位只做复现或独立审查。已有多个实现和 Runtime 的工程分工可走系统校验的交叉审查与集成。 | 对简单任务，协作步骤、等待和成本可控；对有意埋入回归的任务，第二位能发现一位遗漏的具体问题。比较发现率、误报和额外时间。[现有分工校验](../../src-tauri/src/multi_agent/plan_validation.rs#L58) |

暂缓大规模扩充 Runtime、再做一套通用记忆引擎、强制每个任务都“开会”，以及单独建一个需要用户维护的记忆仪表盘。先把解释和纠正放进接力、开工和验收这三个用户已有动作里。只有用户在这个流程里反复遇见某个缺口，再为它增加专用界面。

## 四、两周验证建议

1. **第一周先做基线。** 找 5–8 位已经在同一项目里交替用 Claude/Codex 的人，观察他们上一次真实换人的过程。只问具体行为：重贴了什么、重跑了什么、哪次相信了错误记录。一并记录安装和首次成功共享所需时间。人数和时长是试验建议。
2. **第二周做配对对照。** 用两道难度相当的真实维护任务，交叉比较“两个 CLI + 一份交接 Markdown”和“CrewVela 的接力与记忆”。选一部分任务再与“两个 CLI 直接接同一个 ai-memory”比较，把引擎收益与工作台收益拆开。交替顺序，使用隔离项目、相同模型配置和相同材料，防止第二轮因做过题而显得更快。
3. **记录四个数。** 换人到第一次有效动作的分钟数；人工重述次数；重复排查已排除方案的次数；最终回归测试通过率。同时记录总耗时和可获得的 token/费用。另设一条过时结论和一个无关同名项目，观察是否误用、默认是否串项目。
4. **预先决定是否继续。** 可先约定探索性门槛：接手时间中位数缩短约 30%，多数参与者少重述至少一次，交付正确率不降，普通项目查询不串项目。若只胜过手写交接、与直接装 ai-memory 持平，则优先投资可验证交付或降低安装成本。若小改动成本显著上升，就保留单 Agent 快速路径。这个规模适合决定下一轮投入，无法证明市场普遍收益。

## 五、本次证据和验证范围

- 核查 CrewVela `fa8adfc` 所在工作区、ai-memory 源码 `411d90d1`（Cargo 版本 3.5.0）和 Orca `cdf90d9c72e`。CrewVela 工作区另有未提交的全自动托管实现，以上产品判断不依赖把它当成已发布功能。T3、Multica 采用 2026-11-19 调研时核对的官方仓库快照和公开讨论，仅做定向核查。
- **实际服务实验通过。** 使用本机 CrewVela 管理目录里的 ai-memory **3.4.7**，临时数据目录、独立回环端口、合成数据，关闭 embedding。第一个 MCP 客户端往两个项目中的相同页面路径写入不同内容并断开；第二个全新客户端从项目 A 读到首个客户端写下的随机标记，项目 B 同路径读到 B 自己的内容。全文查询在 A 命中 1 条、在 B 命中 0 条。验证的是两个客户端共用存储和显式项目查询，未使用 Claude/Codex 账号，也没有验证模型自动召回、worktree 间共享或下一轮任务质量。
- **CrewVela 定向回归。** 4 个 Rust 用例通过，覆盖运行成功后提升记忆、待人工审批的状态、私密内容的可见性，以及项目档案引用。3 个 Node 测试文件共 10 项，9 项通过、1 项 POSIX 用例在 Windows 跳过。另有一个 Windows 文件符号链接子场景因系统权限跳过。没有改动真实 Agent 配置或个人记忆。历史验证与本次真实模型验收的差别见[既有验收记录](../shared-memory-quick-setup.md#当前验证2026-11-11)。

对外可以这样说：**CrewVela 让 Claude 和 Codex 在本地共用同一项目的记忆。我们想解决的是，换 Agent 后还得重新讲一遍，以及上次的决定这次还能不能信。** 第一件事已经有实现；后半句是否做得比现有工具好，让真实接力的结果回答。

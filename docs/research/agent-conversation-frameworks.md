# 多 Agent 持续讨论框架调研

调研日期：2026-08-08  
调研范围：Microsoft Agent Framework、Microsoft AutoGen、AG2、CrewAI、LangGraph、MetaGPT、CAMEL、AgentScope，以及更贴近“聊天室式协作”的 AgentTeams。  
资料范围：仅使用官方 GitHub 仓库、官方文档、官方论文或官方 API。

## 1. 结论

GitHub 上确实已经有项目支持两个或多个 Agent 持续多轮讨论，但“支持聊天”至少有三种不同含义：

1. **调用栈内轮流发言**：一个 orchestrator 在当前进程中依次调用 Agent，例如 CAMEL RolePlaying、AgentScope MsgHub。
2. **活跃运行时内消息驱动**：消息到达后，运行中的 runtime 自动调用接收 Agent 的 handler，例如 AG2 Network、AutoGen Core。
3. **离线 Agent 的持久唤醒**：消息先可靠落盘；目标 Agent 已退出时，系统重新拉起进程、恢复会话、重放未确认消息，再继续讨论。

前两类已有成熟实现；第三类仍通常需要宿主应用补齐。对 CrewVela 而言，此前“主要缺收到消息后继续执行的调度循环”的判断成立，但应拆成两层：

- **运行内调度**：决定下一位发言者、投递消息、判断终止条件；
- **跨进程持久唤醒**：SQLite 事务落盘、生成 Wakeup、拉起或恢复 PTY/协议会话、ACK、重放和幂等处理。

综合结论：

| 项目 | 持续多轮讨论 | 入站消息自动执行 | 离线进程自动拉起 | 人工接管 | 精确运行恢复 | 对 CrewVela 的主要价值 |
|---|---:|---:|---:|---:|---:|---|
| **Microsoft Agent Framework** | 是 | 是，活跃 workflow | 否 | 强 | 强，checkpoint/resume | 当前主线的 Group Chat、Handoff、Magentic |
| **AG2 v1.0** | 是 | 是，在线 AgentClient | 否 | 强 | 强，Hub/WAL/checkpoint | 最值得移植的消息与投递模型 |
| **AutoGen** | 是 | 是，活跃 runtime | 否 | 强 | 中，需应用保存/重启 run | 发言者选择、handoff、终止条件 |
| **LangGraph** | 是，需显式建图 | 是，活跃 graph run | 否 | 强 | 强 | 状态图、每步 checkpoint、interrupt |
| **MetaGPT** | 是 | 是，Environment 循环内 | 否 | 中 | 强，Team 断点恢复 | 订阅路由、消息触发 Role、空闲终止 |
| **CAMEL** | 是 | 是，RolePlaying/Workforce 驱动 | 否 | 强 | 中 | 双 Agent 讨论协议、轮数/完成标记 |
| **AgentScope** | 是，需显式循环 | 仅自动 observe，不自动 reply | 否 | 强 | 中到强 | MsgHub、实时中断、会话状态接口 |
| **CrewAI** | 有限，工具式问答/委派 | 同步工具子调用 | 否 | 强 | 强 | checkpoint/HITL 可借鉴，聊天模型不合适 |
| **AgentTeams** | 是，真实 IM 房间 | 是，在线 Matrix 客户端 | 控制器管理容器，但不等于任意 CLI 恢复 | 很强 | 消息/文件强，Agent 运行态取决于 runtime | 最接近产品体验，部署过重 |

推荐组合不是直接嵌入其中一个框架，而是：

- 采用 **AG2 v1.0** 的 `Hub + Envelope + typed channel + WAL + cursor + ACK/NACK + replay`；
- 采用 **LangGraph** 的显式讨论状态图、每步 checkpoint、interrupt/resume 和副作用幂等；
- 采用 **Microsoft Agent Framework** 当前主线的 Group Chat、Handoff、Magentic 与 checkpoint 设计，并参考 **AutoGen** 的可组合终止条件；
- 由 CrewVela 的 Rust 后端自行实现 OS 进程监督、PTY/协议会话恢复和 SQLite 事务边界。

## 2. GitHub 热度与活跃度

以下 star 为 2026-08-08 通过 GitHub Repository API 查询的瞬时值。`pushed_at` 比网页的 `updated_at` 更能反映代码提交活动；release 日期来自官方 Releases API。

| 项目 | GitHub star | 最后 push | 最新 GitHub release | 活跃度判断 |
|---|---:|---|---|---|
| [Microsoft Agent Framework](https://github.com/microsoft/agent-framework) | 12.7k | 2026-08 活跃 | 1.0 生产版本 | 高；AutoGen 官方继任者 |
| [Microsoft AutoGen](https://github.com/microsoft/autogen) | 60,299 | 2026-04-15 | `python-v0.7.5`，2025-09-30 | 已进入维护模式，不宜作为新核心依赖 |
| [AG2](https://github.com/ag2ai/ag2) | 4,839 | 2026-08-07 | `v1.0.1`，2026-07-29 | 高；v1.0 Network 刚完成主线切换 |
| [CrewAI](https://github.com/crewAIInc/crewAI) | 56,756 | 2026-08-07 | `1.15.13`，2026-08-07 | 很高 |
| [LangGraph](https://github.com/langchain-ai/langgraph) | 39,148 | 2026-08-07 | 2026-08-07 仍发布组件 | 很高 |
| [MetaGPT](https://github.com/FoundationAgents/MetaGPT) | 69,700 | 2026-01-21 | `v0.8.1`，2024-04-22 | star 高，但主库和 release 活跃度明显下降 |
| [CAMEL](https://github.com/camel-ai/camel) | 17,563 | 2026-08-07 | `v0.2.90`，2026-03-22 | 高 |
| [AgentScope](https://github.com/agentscope-ai/agentscope) | 28,711 | 2026-08-07 | `v2.0.6`，2026-08-07 | 很高 |
| [AgentTeams](https://github.com/agentscope-ai/AgentTeams) | 5,334 | 2026-08-07 | `v1.2.1`，2026-08-06 | 很高，新项目增长快 |

数据源：[Microsoft Agent Framework 官方仓库](https://github.com/microsoft/agent-framework)、[AutoGen](https://api.github.com/repos/microsoft/autogen)、[AG2](https://api.github.com/repos/ag2ai/ag2)、[CrewAI](https://api.github.com/repos/crewAIInc/crewAI)、[LangGraph](https://api.github.com/repos/langchain-ai/langgraph)、[MetaGPT](https://api.github.com/repos/FoundationAgents/MetaGPT)、[CAMEL](https://api.github.com/repos/camel-ai/camel)、[AgentScope](https://api.github.com/repos/agentscope-ai/agentscope)、[AgentTeams](https://api.github.com/repos/agentscope-ai/AgentTeams)。

## 3. 项目逐项核验

### 3.0 Microsoft Agent Framework

**判断：这是微软当前生产主线，最适合参考“讨论工作流”的上层协议，但仍不能替代 CrewVela 的离线 CLI 进程唤醒。**

Microsoft Agent Framework 是 AutoGen 的官方继任者，官方仓库已声明 AutoGen 进入维护模式，新项目应使用 Agent Framework。它内置 Sequential、Concurrent、Handoff、Group Chat 和 Magentic 五类多 Agent 编排，并提供 checkpoint、streaming、human-in-the-loop、可观测性和 time-travel。来源：[官方仓库](https://github.com/microsoft/agent-framework)、[Workflow Orchestrations](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/)。

Group Chat 会在每轮后把响应广播给所有参与 Agent，各 Agent 保持独立 `AgentSession`，但 orchestrator 会在下一轮前同步完整对话历史；之后由 round-robin 或自定义 manager 选择下一位发言者。它明确面向“迭代修订、协同解决问题、多视角分析和质量审核”。来源：[Group Chat](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/group-chat)、[GroupChatBuilder](https://learn.microsoft.com/en-us/python/api/agent-framework-core/agent_framework.groupchatbuilder?view=agent-framework-python-latest)。

Handoff 允许 Agent 用工具调用把控制权交给另一个 Agent，并可配置允许的转移图、终止条件和 autonomous mode；Magentic 使用 manager 维护共享上下文、跟踪进度并动态选择下一 Agent。来源：[Handoff](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/handoff)、[Magentic](https://learn.microsoft.com/en-us/semantic-kernel/frameworks/agent/agent-orchestration/magentic)。

它对 CrewVela 的价值是：

1. 用 Group Chat manager 表达“谁下一位发言”；
2. 用 Handoff 表达 Agent 主动指定接收者；
3. 用 Magentic manager 表达动态讨论与重新规划；
4. 用 checkpoint/HITL 表达暂停、审批和恢复；
5. 保持每个 Agent 独立会话，通过 orchestrator 同步必要上下文。

边界是：官方现成实现主要面向 Python 与 .NET workflow；即使 workflow 可以 checkpoint，也不等于能自动拉起已经退出的 Claude Code/Codex CLI。CrewVela 仍应由 Rust supervisor 负责进程启动、PTY 恢复、SQLite inbox 和消息重放。

### 3.1 AG2 v1.0

**判断：目前最接近 CrewVela 所需“可靠 Agent 聊天内核”的项目。**

#### 消息模型

AG2 v1.0 当前主线使用 `Network`。中心 `Hub` 管理 Agent 注册表、channel、WAL 和审计日志；传输单元是 `Envelope`，包含 `event_type`、`event_data`、`sender_id`、`audience`、`causation_id` 等字段。官方定义四类 channel：

- `conversation`：双方自由聊天，任一方可主动发言；
- `consulting`：严格的一问一答，回复后自动关闭；
- `discussion`：多方 round-robin；
- `workflow`：基于 `TransitionGraph` 的条件路由和 handoff。

来源：[Network Overview](https://docs.ag2.ai/docs/user-guide/network/overview/)、[Channel Adapters](https://docs.ag2.ai/docs/user-guide/network/adapters_overview/)。

#### 自动触发与唤醒

`AgentClient` 收到 `EV_TEXT` 或 `EV_HANDOFF` 后，默认 handler 会检查是否轮到自己，从 WAL 投影历史，调用 `agent.ask()`，再发送回复。因此只要 AgentClient 在线，消息会自动引发下一轮，而不是依赖业务代码轮询消息表。跨进程部署使用 WebSocket `NotifyFrame` 推送。

断线后 Hub 提供 at-least-once 投递、收件游标、ACK/NACK、未确认消息重放以及 `causation_id` 去重；Agent 重连后可使用 `attach(... since_envelope_id=...)` 和 `resume_pending_turns()`。但 AG2 **不负责启动已经退出的 OS 进程**，仍需外部 supervisor。

来源：[Agent Clients and Handlers](https://docs.ag2.ai/docs/user-guide/network/agent_clients/)、[Distributed Deployment](https://docs.ag2.ai/docs/user-guide/network/distributed/)、[官方双 Agent 示例](https://github.com/ag2ai/ag2#orchestrating-multiple-agents)。

#### 循环与终止

- `conversation` 和 `discussion` 可以持续往返，通过显式 `close()` 或 TTL 结束；
- `workflow` 支持 `TerminateTarget` 和 `max_turns`；
- Hub 支持静默超时、回合超时、自动关闭和 inbox 上限。

这比单纯让模型输出“DONE”更适合工程控制。

#### 人工接管

`HumanClient` 与 LLM Agent 是同级 participant，能够接收 Hub 路由、等待下一条消息、异步流式读取以及主动发言，适合映射到 Tauri 聊天界面。

来源：[Human Clients](https://docs.ag2.ai/docs/user-guide/network/human_client/)。

#### 持久化与恢复

`KnowledgeStore` 保存注册表、审计日志和 channel WAL；`DiskKnowledgeStore` 可跨 Hub 重启。adapter 使用纯 `fold()` 从 WAL 重建 channel 状态；任务层还提供 `HubBackedCheckpointStore` 和 `resume_from()`。

来源：[Hub and Identity](https://docs.ag2.ai/docs/user-guide/network/hub_and_identity/)、[Distributed Deployment](https://docs.ag2.ai/docs/user-guide/network/distributed/)。

#### 对 CrewVela 的移植价值

应直接借鉴其协议设计，而不是引入 Python runtime：

1. SQLite append-only Envelope WAL；
2. channel adapter 只负责协议规则，运行状态由 WAL fold 得出；
3. 每个参与者维护收件 cursor 和未 ACK 队列；
4. 使用 `causation_id`/`reply_to` 保证回复幂等；
5. Tauri 接收 Rust push event，PTY 作为 Agent handler；
6. Rust supervisor 根据未完成 turn 拉起目标 runtime，再重放消息。

### 3.2 Microsoft AutoGen

**判断：确实支持持续多轮讨论，运行内调度完整；但已进入维护模式，适合参考，不适合成为新依赖。**

#### 消息模型与自动触发

AutoGen Core 是 actor/runtime 模型，支持直接消息和 topic 发布/订阅。消息到达已启动 runtime 后会调用目标 handler，首次投递还可通过工厂懒实例化 Agent。AgentChat 在其上提供 `RoundRobinGroupChat`、`SelectorGroupChat`、`Swarm` 和 `GraphFlow`。

来源：[Message and Communication](https://microsoft.github.io/autogen/stable/user-guide/core-user-guide/framework/message-and-communication.html)、[Runtime Architecture](https://microsoft.github.io/autogen/stable/user-guide/core-user-guide/core-concepts/architecture.html)。

实验性 gRPC distributed runtime 能在活动 host/worker 间跨进程投递，但没有持久离线 inbox，也不会自动拉起已退出的 Claude/Codex 进程。`SingleThreadedAgentRuntime.stop()` 会清空内存队列，这进一步说明它不是 durable queue。

来源：[Distributed Agent Runtime](https://microsoft.github.io/autogen/stable/user-guide/core-user-guide/framework/distributed-agent-runtime.html)、[SingleThreadedAgentRuntime 源码](https://microsoft.github.io/autogen/stable/_modules/autogen_core/_single_threaded_agent_runtime.html)、[跨语言 protobuf](https://github.com/microsoft/autogen/blob/main/protos/agent_worker.proto)。

#### 循环与终止

GroupChat 按 round-robin、模型选择器或 handoff 持续轮转；GraphFlow 可显式形成循环。内置终止条件包括文本命中、最大消息数、外部终止、超时、token 用量等，并可组合。

来源：[Teams](https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/teams.html)、[Selector Group Chat](https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/selector-group-chat.html)、[Swarm](https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/swarm.html)、[GraphFlow](https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/graph-flow.html)。

#### 人工接管

支持 `UserProxyAgent`、`HandoffTermination`、`ExternalTermination` 和取消令牌。官方推荐的异步人工流程是“终止当前 run → 保存团队状态 → 等待反馈 → 新 run 恢复”，因为在单次 run 中阻塞等待输入的状态无法可靠保存和恢复。

来源：[Human-in-the-Loop](https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/human-in-the-loop.html)。

#### 持久化与恢复

Agent、Team 和 termination condition 支持 `save_state()`/`load_state()`，返回可序列化字典；应用必须自行写入数据库、重新加载并再次调用 `run()`。

来源：[Managing State](https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/state.html)。

#### 对 CrewVela 的移植价值

适合移植发言者选择、直发与广播分离、handler、handoff、可组合终止条件和跨语言消息 schema。SQLite inbox、PTY supervisor、崩溃恢复仍必须由 Rust 实现。AutoGen 官方 README 已标明 Maintenance Mode，并建议新项目转向 Microsoft Agent Framework：[官方 README](https://github.com/microsoft/autogen#autogen)。

### 3.3 LangGraph

**判断：不是现成聊天室，但它是最成熟的持久讨论状态机参考。**

#### 消息模型与自动触发

LangGraph 没有独立 Agent mailbox。消息通常保存在共享 `State.messages`；Agent 是 graph node 或 subgraph。handoff 通过工具返回 `Command(goto=..., update=..., graph=Command.PARENT)`，同时更新消息状态并把控制权转给下一 Agent。

在活跃的 `invoke()`/`stream()` 内，edge 或 `Command` 会自动调度下一 node；图已结束或暂停时，必须由应用再次调用。它不负责启动离线进程。

来源：[Multi-agent Handoffs](https://docs.langchain.com/oss/python/langchain/multi-agent/handoffs)、[Graph API](https://docs.langchain.com/oss/python/langgraph/graph-api)。

#### 循环与终止

普通 edge、conditional edge 和 `Command.goto` 可以构成任意循环；通过 `END` 或条件路由结束，并使用 `recursion_limit` 防止无限循环。

来源：[Custom Workflow](https://docs.langchain.com/oss/python/langchain/multi-agent/custom-workflow)、[GRAPH_RECURSION_LIMIT](https://docs.langchain.com/oss/python/langgraph/errors/GRAPH_RECURSION_LIMIT)。

#### 人工接管

`interrupt()` 会保存图状态并无限期暂停；应用使用相同 `thread_id` 和 `Command(resume=...)` 恢复。节点恢复时从节点开头重跑，因此外部副作用必须幂等。

来源：[Interrupts](https://docs.langchain.com/oss/python/langgraph/interrupts)。

#### 持久化与恢复

每个 superstep 都可保存 checkpoint；支持 thread history、状态更新、time travel、pending writes 和故障后从上一成功 step 恢复。官方提供 `SqliteSaver`/`AsyncSqliteSaver`。

来源：[Persistence](https://docs.langchain.com/oss/python/langgraph/persistence)、[SQLite Checkpointer](https://reference.langchain.com/python/langgraph/checkpoint.sqlite)。

#### 对 CrewVela 的移植价值

应把讨论协议建成显式状态图，例如：

```text
proposal -> critique -> revise -> vote/approve -> done
                   \-> waiting_human -> resume
```

每轮 checkpoint 至少保存 `current_node`、`next_nodes`、`message_cursor`、`round`、`budget`、`termination_state`。任何写文件、发消息和创建任务的动作都必须带幂等键。

### 3.4 CrewAI

**判断：支持 Agent 协作和反复委派，但不是一等的对等聊天系统。**

#### 消息模型与自动触发

启用 `allow_delegation=True` 后，Agent 自动获得两个工具：

- `Delegate work to coworker(task, context, coworker)`；
- `Ask question to coworker(question, context, coworker)`。

调用工具时，目标 Agent 在当前调用链中同步执行并返回结果。它没有独立持久收件箱、在线 participant 或“收到消息后自行恢复”的 peer runtime。官方文档甚至单列“Delegation Loops”，建议通过清晰层级和禁止 specialist 再委派来避免无限往返。

来源：[Collaboration](https://docs.crewai.com/en/concepts/collaboration)。

#### 循环与终止

Crew 的任务流程主要是 sequential/hierarchical；Flow 可用 listener/router 显式形成循环。Agent 自身有迭代限制，但 Agent 间往返主要依赖工具调用和 prompt 约束，并非 channel protocol。

#### 人工接管

`@human_feedback` 可以暂停 Flow、等待审批、把自由文本折叠成 `approved/rejected/needs_revision` 等 outcome，并通过 `or_()` 形成修订循环。异步 provider 可返回 `HumanFeedbackPending`，状态自动持久化，反馈到达后通过 `from_pending(flow_id).resume(...)` 恢复。

来源：[Human Feedback in Flows](https://docs.crewai.com/en/learn/human-feedback-in-flows)。

#### 持久化与恢复

当前 CrewAI 的 checkpoint 已覆盖 Crew、Flow、Agent 的配置、memory、knowledge、task progress、中间输出和 lineage；提供 `JsonProvider` 和启用 WAL 的 `SqliteProvider`，可恢复或 fork。Flow 的 `@persist` 默认也使用 `SQLiteFlowPersistence`。

来源：[Checkpointing](https://docs.crewai.com/en/concepts/checkpointing)、[Flows](https://docs.crewai.com/en/concepts/flows)。

#### 对 CrewVela 的移植价值

值得借鉴 checkpoint inheritance、异步人工反馈、暂停/恢复和 fork；不应采用“把对方 Agent 包成工具”的通信模型，因为它会让长讨论被嵌套在一个不可审计的调用栈内。

### 3.5 MetaGPT

**判断：真正支持消息驱动的多轮 Role 协作，但更偏 SOP/环境仿真，而非自由聊天室。**

#### 消息模型与自动触发

`Message` 包含来源、`cause_by`、`send_to` 等路由信息。Role 通过 `_watch()` 订阅消息标签；`Environment.publish_message()` 根据订阅和目标广播。运行示例使用：

```python
while not env.is_idle:
    await env.run()
```

因此在 Environment 循环活跃时，新消息会被目标 Role `_observe`，随后触发 think/act 并发布下一条消息。它比 AgentScope 的仅广播 observe 更接近真正消息驱动。

来源：[Agent Communication](https://docs.deepwisdom.ai/en/guide/in_depth_guides/agent_communication.html)、[MultiAgent 101](https://docs.deepwisdom.ai/main/en/guide/tutorials/multi_agent_101.html)。

#### 循环与终止

可以构造 `B → C → D → B` 的持续消息环；停止条件通常是 `env.is_idle`、`Team.run(n_round=...)`、预算耗尽或场景环境自己的结束规则。官方示例用 `n_round` 防止无界运行。

#### 人工接管

Role 设置 `is_human=True` 后，轮到该 Role 时进程暂停等待终端输入，人的反馈会按原 SOP 继续流转。Data Interpreter 还支持计划确认、修改和 redo。

来源：[Human Engagement](https://docs.deepwisdom.ai/v0.6/en/guide/tutorials/human_engagement.html)、[Human Interaction](https://docs.deepwisdom.ai/main/en/guide/use_cases/agent/interpreter/human_interaction.html)。

#### 持久化与恢复

MetaGPT 能把 Team、Environment、Role、Action、memory 和当前 state 统一序列化到 `team.json`；正常结束、异常或 Ctrl+C 时保存，并通过 `--recover_path` 从未完成 Action 继续。

来源：[Serialization & Breakpoint Recovery](https://docs.deepwisdom.ai/main/en/guide/in_depth_guides/breakpoint_recovery.html)。

#### 对 CrewVela 的移植价值

可借鉴 `cause_by + send_to + watch` 的订阅模型、`env.is_idle`、Role 状态快照和环境规则；不建议采用其大型 Python Role/Action 体系。

### 3.6 CAMEL

**判断：双 Agent 或小组讨论能力成熟，主要是回合驱动，不是 durable mailbox。**

#### 消息模型与自动触发

`RolePlaying` 维护 assistant、AI user 和可选 critic。一次 `step(input_msg)` 会依次调用两侧 Agent，返回双方 response；官方示例在 `while n < chat_turn_limit` 中不断把上一轮 assistant 消息作为下一轮输入。`Workforce` 可把一个 RolePlaying session 包装成 worker，也有 task channel、coordinator 和动态 worker。

来源：[Societies / RolePlaying](https://docs.camel-ai.org/key_modules/societies)、[Workforce](https://docs.camel-ai.org/key_modules/workforce)。

这里的“自动”指 `step()`/Workforce 正在运行时驱动下一方，不是消息落盘后独立唤醒离线 Agent。

#### 循环与终止

官方示例同时检查：

- `chat_turn_limit`；
- 任一 response 的 `terminated` 与 termination reason；
- `CAMEL_TASK_DONE` 标记。

RolePlayingWorker 也暴露 `chat_turn_limit`。

#### 人工接管

RolePlaying 的 critic 可设置为 human；Workforce 可给 coordinator、planner 或 worker 配置 `HumanToolkit`，在运行中阻塞询问、审批或解锁错误。Workforce 还公开 pause/resume、pending task review、修改/重排/移除任务和从特定任务恢复。

来源：[RolePlaying API](https://docs.camel-ai.org/reference/camel.societies.role_playing)、[Workforce API](https://docs.camel-ai.org/reference/camel.societies.workforce.workforce)、[Human-in-the-loop cookbook](https://docs.camel-ai.org/cookbooks/advanced_features/agents_with_human_in_loop_and_tool_approval)。

#### 持久化与恢复

Workforce 有 `WorkforceSnapshot`、`save_snapshot()`、`restore_from_snapshot()` 和 workflow memory 的保存/加载。但官方 API 没有表明 RolePlaying 的完整进行中会话具备类似 LangGraph/AG2 的 durable checkpoint；snapshot 主要是当前 Workforce 实例的恢复能力，workflow memory 也不等于精确 run 恢复。

#### 对 CrewVela 的移植价值

适合移植固定双人讨论模板、critic-in-the-loop、明确的 turn limit 和 task-done 信号；消息可靠性和跨重启恢复仍需另行设计。

### 3.7 AgentScope

**判断：能方便地写多 Agent 对话，但经典 MsgHub 只自动共享消息，不自动让接收方回复。**

#### 消息模型与自动触发

`Msg` 是 Agent 间交换、UI 展示和 memory 存储的统一结构。`AgentBase` 明确区分：

- `reply`：处理输入并生成响应；
- `observe`：接收消息但不生成响应；
- `print`：输出到终端或 UI。

`MsgHub` 会在某个 participant 生成回复后，自动调用其他 participant 的 `observe()` 广播消息，但业务代码仍需显式 `await alice(...)`、`await bob(...)` 或调用 sequential pipeline。因此普通 AgentScope 不具备“消息到达即自动回复”的 handler runtime。

来源：[Key Concepts](https://doc.agentscope.io/tutorial/quickstart_key_concept.html)、[Conversation](https://doc.agentscope.io/tutorial/workflow_conversation.html)、[Pipeline](https://doc.agentscope.io/tutorial/task_pipeline.html)。

补充：实验性的 Realtime `ChatRoom` 能集中管理多个 RealtimeAgent、自动广播并处理统一事件队列，但官方仍标为 active development，且多数 realtime model API 目前只支持单用户交互。

来源：[Realtime Agent](https://doc.agentscope.io/tutorial/task_realtime.html)。

#### 循环与终止

循环由应用代码定义。官方 debate 例子常用 moderator 结构化输出 `finished` 来跳出 `while True`；框架没有统一 conversation termination protocol。

#### 人工接管

AgentScope 的实时介入较完整：`interrupt()` 基于 asyncio cancellation 取消当前 reply，`handle_interrupt()` 执行后处理；ReActAgent 会正确保存中断时的 memory 和 state。`UserAgent` 可作为人工参与者。

来源：[Create ReAct Agent](https://doc.agentscope.io/tutorial/quickstart_agent.html)、[Agent realtime steering](https://doc.agentscope.io/zh_CN/tutorial/task_agent.html)。

#### 持久化与恢复

`StateModule` 提供 `state_dict/load_state_dict`；`SessionBase` 提供保存/加载接口，内置 `JSONSession`，可一次管理多个 Agent 的状态。memory 层已有 `AsyncSQLAlchemyMemory`，支持 SQLite、PostgreSQL、MySQL。

来源：[State/Session Management](https://doc.agentscope.io/tutorial/task_state.html)、[Memory](https://doc.agentscope.io/zh_CN/tutorial/task_memory.html)。

#### 对 CrewVela 的移植价值

值得借鉴 `reply/observe/interrupt` 三分法、统一 Msg、participant 动态加入/退出、状态模块嵌套保存以及用户实时中断；不能只复制 MsgHub 广播，否则仍会停留在“收到但不回复”。

### 3.8 AgentTeams（补充项目）

**判断：产品体验上最接近“人和多个 Agent 在同一个群聊里持续协作”，但基础设施远重于桌面应用所需。**

AgentTeams 使用 Manager-Workers 架构，每个 Matrix room 包含用户、Manager 和相关 Workers。OpenClaw、QwenPaw、Hermes 等 runtime 通过 Matrix `m.mentions` 在同一房间通信；用户能看见全部消息并随时插话。通信通过 Tuwunel Matrix homeserver，文件通过 MinIO，共享 task tree 和 workspace 也持久化到对象存储。controller 管理 Worker/Manager/Team/Human 声明式资源和容器生命周期。

来源：[官方 README](https://github.com/agentscope-ai/AgentTeams)、[Architecture](https://github.com/agentscope-ai/AgentTeams/blob/main/docs/architecture.md)、[Worker Guide](https://github.com/agentscope-ai/AgentTeams/blob/main/docs/worker-guide.md)。

它证明“房间时间线 + @mention + 人工始终在场 + 可替换 Worker runtime”这一产品模型可行。但 CrewVela 没必要引入 Kubernetes、Matrix、MinIO 和 AI Gateway 全套；可把 Matrix room 缩减成 SQLite conversation log，把 Matrix mention 缩减成目标 Agent 的 durable Wakeup，把 controller 缩减成现有 Rust scheduler/supervisor。

## 4. 对 CrewVela 当前实现的直接判断

当前代码已经具备大部分控制平面基础：

- `CollaborationMessage`、`WakeupRequest`、`AgentSessionRecord`、`RunStatus::WaitingInput/WaitingApproval` 已经存在；
- SQLite 已存储 `ma_messages`、`ma_wakeups`、runtime events、memory、approval 和 session locator；
- scheduler 已能为 runnable task 创建、claim、完成 durable wakeup；
- runtime adapter 已支持各 CLI 的 resume locator；
- 人工 takeover、approval pause/resume、崩溃恢复已有状态机和测试。

真正的断点非常具体：`MultiAgentStore::send_collaboration_message()` 当前只在事务中插入 `ma_messages` 和 `message.sent` event，**没有同时为目标 Agent 建立 conversation wakeup**；`schedule_until_idle()` 消费的 wakeup 则是围绕 runnable task/run 设计的。于是消息可以可靠留下，但不会自动让一个 idle、waiting 或 stopped Agent 产生新 turn。

现有 `CollaborationMessage` 也缺少可靠投递所需字段：

- `conversation_id/channel_id`；
- `reply_to/causation_id`；
- `sequence_no`；
- `delivery_status`；
- `target_cursor/acked_at`；
- `message_type`（text、handoff、approval、system、artifact reference）；
- `turn_id` 与幂等键。

因此无需推倒现有 Project/Task/Run/Message/Artifact/Approval 模型，但要在 Task scheduler 旁新增 **Conversation dispatcher**，不能把聊天消息勉强伪装成新 Task。

## 5. 建议的 CrewVela 设计

### 5.1 新增领域对象

```text
Conversation
  id, project_id, task_id?, protocol, status,
  max_rounds, max_tokens, deadline, idle_timeout,
  termination_policy, created_at, closed_at

ConversationParticipant
  conversation_id, agent_id/human_id, role,
  status, inbox_cursor, last_acked_seq, joined_at

Envelope
  id, conversation_id, seq, sender_id, target_id?,
  type, body_json, reply_to, causation_id,
  turn_id, idempotency_key, created_at

Delivery
  envelope_id, recipient_id, status,
  attempt, available_at, delivered_at, acked_at, error

ConversationTurn
  id, conversation_id, agent_id, trigger_envelope_id,
  run_id?, status, started_at, finished_at, outcome

ConversationCheckpoint
  conversation_id, current_speaker, next_speakers,
  round, token_usage, state_json, last_seq
```

`Conversation.protocol` 首期只需：

- `direct`：A 发给 B；
- `round_robin`：固定轮流；
- `selector`：规则或 coordinator 选择下一位；
- `handoff`：当前 Agent 明确指定下一位；
- `review_loop`：proposal → critique → revise → approve。

### 5.2 消息发送的事务边界

一次 `message.send` 应在同一个 SQLite `IMMEDIATE` 事务中：

1. 追加 Envelope；
2. 为目标 participant 创建 Delivery；
3. 根据 channel fold 计算下一位发言者；
4. 创建或合并 `conversation_wakeup`；
5. 写 ActivityEvent；
6. commit 后通过 Tauri event/内部 channel 通知 dispatcher。

只在 commit 之后触发进程动作，避免“PTY 已收到消息但数据库没有记录”。

### 5.3 Rust dispatcher 的处理规则

```text
新 Envelope 提交
  -> 目标 Agent 正在 idle/waiting 且协议连接可写
       -> 创建 ConversationTurn -> submit/steer
  -> 目标 Agent 正在执行
       -> 保留 Delivery；本 turn 完成后再投递
  -> 目标 Agent 进程已退出，但有 resume 能力
       -> 启动 adapter -> resume session -> 注入未 ACK 消息
  -> 目标 Agent 无法 resume
       -> fresh session + conversation summary + unread messages
  -> 等待人工或权限审批
       -> conversation 状态 waiting_human，不继续自动轮转
```

不要直接向 PTY 写任意字符串作为主链路；优先使用已有 runtime adapter 的结构化 submit/resume/steer。PTY 只负责展示、人工接管和无协议时降级。

### 5.4 可靠性规则

- 投递语义采用 **at-least-once + 幂等消费**，不要追求难以兑现的 exactly-once；
- `causation_id + recipient_id` 建唯一约束，防止 Agent 重连后重复回复；
- 每个 participant 持久化 inbox cursor；
- Agent 只有在回复已落盘或明确标记“无需回复”后才 ACK；
- delivery 退避重试并设置 dead-letter 状态；
- scheduler 重启时扫描 pending/inflight Delivery，而不是只扫描 running Task；
- 写文件、合并、发外部消息等副作用继续通过现有受控工具和幂等键执行。

### 5.5 终止条件

每个 Conversation 必须至少配置一个硬终止条件：

- 最大轮数；
- token/费用预算；
- wall-clock deadline；
- 连续无进展次数；
- moderator 结构化 `resolved=true`；
- reviewer/人类批准；
- 外部取消；
- 所有 participant idle 且无 pending delivery。

终止条件采用 AutoGen 式可组合对象，状态随 checkpoint 保存。模型输出的 `DONE` 只能作为一个信号，不能是唯一保障。

### 5.6 人工接管

沿用当前 `AgentControlMode` 和 approval 状态机，增加 conversation 级能力：

- 用户在聊天面板中 @Agent，生成与 Agent 相同格式的 Envelope；
- 用户可暂停自动轮转但保留未读消息；
- takeover 时取消或等待当前 turn，保存 checkpoint；
- return control 后从 inbox cursor 继续；
- 高风险工具仍进入现有 Approval，不因 Agent 聊天而放宽权限。

## 6. 实施优先级

### 第一阶段：两 Agent 持久讨论 MVP

1. 增加 Conversation、Participant、Envelope、Delivery、Turn 表；
2. 把 `message.send` 改为 Envelope + Delivery + Wakeup 原子事务；
3. 增加 dispatcher，支持 idle submit、busy queue、stopped resume；
4. 只实现 `direct` 和 `round_robin`；
5. 强制 `max_rounds`、deadline、idle timeout；
6. Tauri UI 显示同一任务下的聊天时间线、未读和当前发言者。

### 第二阶段：可靠恢复与人工接管

1. ACK/NACK、cursor、重放、dead-letter；
2. conversation checkpoint；
3. waiting_human/resume；
4. runtime 无 resume 时使用摘要 + 未读消息建立 fresh session；
5. 录制崩溃点测试：commit 前、commit 后/PTY 前、PTY 后/ACK 前。

### 第三阶段：讨论协议

1. selector/handoff；
2. proposal → critique → revise → approve 状态图；
3. moderator 结构化终止；
4. token/费用预算与“连续无进展”检测；
5. 三个以上 Agent 的 discussion channel。

## 7. 最终建议

CrewVela 不应整体迁移到 AutoGen、CrewAI 或 LangGraph。当前 Rust/Tauri/SQLite/PTY 架构已经拥有任务、运行、消息、Wakeup、恢复和人工接管的基础，缺的是把这些能力连成一个 **durable conversation runtime**。

最小且正确的技术路线是：

```text
AG2 的 Envelope/WAL/ACK
  + LangGraph 的显式状态图/checkpoint/interrupt
  + Microsoft Agent Framework 的 Group Chat/Handoff/Magentic
  + AutoGen 的可组合 termination
  + CrewVela 的 Rust scheduler/adapter/PTY supervisor
```

AgentTeams 可作为产品交互参考：所有 Agent 与人共享一条可审计时间线，任何参与者都能 @mention 和接管；但无需复制其 Matrix/Kubernetes/MinIO 部署栈。

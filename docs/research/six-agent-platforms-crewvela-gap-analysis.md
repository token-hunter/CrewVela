# 六个 Agent 平台与 CrewVela 能力差距分析

> 研究日期：2026-08-11  
> 范围：Prime Agent、Hermes Agent、Vibe Kanban、Ruflo、OpenAgents、LobeHub  
> 证据规则：只采用项目官方文档、官方仓库及固定提交源码；“文档宣称”与“源码确认”分开标记。

## 结论

CrewVela 不缺“再加一个 Agent”或“再做一套任务看板”。它现有的任务图、Worktree、Reviewer、Integrator、Agent 消息、自动化、语音和人工验收已经覆盖了六个项目中大量显眼功能。真正值得补的是下面五项基础能力：

| 排名 | 建议能力 | 主要来源 | 与 CrewVela 的关系 | 优先级 |
|---|---|---|---|---|
| 1 | 语义化、可引用的项目知识库 | LobeHub + Hermes | 当前只有已验证记忆和 `LIKE` 关键词检索；这是“原始证据摄取、混合检索、引用回溯” | P0 |
| 2 | 任务级增量 LSP 诊断 | Hermes | 当前测试/Review 之后才发现许多类型、符号、导入错误；LSP 是更早的一层质量信号 | P0 |
| 3 | 受控自学习 Skill/MCP 中心 | Prime Agent + Hermes + LobeHub | 当前没有 Skill/MCP 注册、版本、审批、回滚和按 Agent 授权 | P0/P1 |
| 4 | 预算化自治与质量策略 | Prime Agent + Ruflo | 当前只有尝试次数和验收命令；缺 Token、时间、回合、命令等硬预算和停止原因 | P1 |
| 5 | 任务替代尝试实验室 | Vibe Kanban + Ruflo | 当前重试/重派是顺序覆盖；缺“同题多方案隔离运行、统一评测、择优合并” | P1 |

“外部监督与审批网关”值得做，但排在第六：LobeHub 与 OpenAgents 已证明配对、白名单、跨渠道路由可以工程化；不过对本地开发主流程的直接收益低于增量 LSP。建议等前五项形成稳定事件/审批模型后，再接飞书或 Telegram。

## 研究基线与固定版本

| 项目 | 固定提交 | 本次重点核验 |
|---|---|---|
| Prime Agent | [`71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8`](https://github.com/PrimeIntellect-ai/prime-agent/tree/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8) | 自我改进、自治预算、会话分支、Persistent RLM 的安全边界 |
| Hermes Agent | [`9829746dfe3d5077f4f076e257505ec7f8feaa65`](https://github.com/NousResearch/hermes-agent/tree/9829746dfe3d5077f4f076e257505ec7f8feaa65) | 增量 LSP、会话 FTS5、后台学习审查、Skill 审批 |
| Vibe Kanban | [`4deb7eca8f381f7cbc1f9d15515a9ab8f8009053`](https://github.com/BloopAI/vibe-kanban/tree/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053) | 替代尝试、工作区隔离、评审/合并边界 |
| Ruflo | [`f35c545fbe927aeb4ab8433bab8d827f69436572`](https://github.com/ruvnet/ruflo/tree/f35c545fbe927aeb4ab8433bab8d827f69436572) | 预算/策略、记忆、路由、实验性能力边界 |
| OpenAgents | [`ffbd0c92ad87c5c262ba5524183994727e572079`](https://github.com/openagents-org/openagents/tree/ffbd0c92ad87c5c262ba5524183994727e572079) | 节点配对、远程接入、共享知识、Tunnel 真实边界 |
| LobeHub | [`91919550877cf4a63ca259b8dda3e01bd819c074`](https://github.com/lobehub/lobehub/tree/91919550877cf4a63ca259b8dda3e01bd819c074) | 知识库、Skill/MCP 管理、多消息渠道与配对权限 |

### CrewVela 已有能力与真实缺口

以下判断来自当前仓库，而不是产品印象：

- 已有任务图、依赖、并行 Worktree、Reviewer、Integrator、人工批准、失败重试与重派。任务模型中已有 `verification_command` 和 `max_attempts`，执行结束也会做验收与反馈，见 [`domain.rs`](../../src-tauri/src/multi_agent/domain.rs) 与 [`executor.rs`](../../src-tauri/src/multi_agent/executor.rs)。
- 已有持久消息、交付物和成功后晋升的已验证记忆；但记忆查询实际是 `key LIKE` 或 `value LIKE`，没有文档摄取、分块、Embedding、向量索引和来源引用，见 [`store.rs`](../../src-tauri/src/multi_agent/store.rs)。
- ACP 初始化明确传入空的 `mcpServers`；项目协作 MCP 是执行器内部固定装配，没有集中注册、版本锁、审批或按 Agent 授权，见 [`acp.rs`](../../src-tauri/src/acp.rs) 与 [`executor.rs`](../../src-tauri/src/multi_agent/executor.rs)。
- `goal` 目前是项目描述字段，不是具备 Token/时间/回合预算、暂停/恢复语义和终止原因的持久自治目标；任务控制主要靠 `max_attempts`、退避和验收命令。
- 已有 `retry_task`、`reassign_task`，但没有同一任务在多个 Agent/模型/基础分支上同时产生候选、统一评分、选择赢家后再合并的实验对象。
- 已有本地通知与内部消息，但没有外部用户配对、渠道 RBAC、签名回调、幂等投递和远程批准/拒绝工作流。

因此，下面推荐的是“补底座”，不是替换现有架构。

## 六个项目逐项核验

### 1. Prime Agent

#### 源码确认：可审核、可回滚的持续改进

Prime 的 `/refine` 会根据会话轨迹、失败和用户纠正生成小范围修改，目标可包含 Prompt 附加说明、记忆、Skill 和子 Agent 规格。实现保留不可变基础提示，区分项目/全局作用域，先生成计划再应用，保存修改前快照并支持回滚；还用基线状态避免长生成期间覆盖并发变化。证据：[`README`](https://github.com/PrimeIntellect-ai/prime-agent/blob/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8/README.md)、[`refinement.ts`](https://github.com/PrimeIntellect-ai/prime-agent/blob/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8/packages/coding-agent/src/core/refinement/refinement.ts)。

真实边界：源码证明了“生成变更、快照、应用和回滚”链路，没有证明模型生成的改进一定有效。CrewVela 不能照搬自动应用，应默认进入待审批队列。

#### 源码/文档确认：有硬上限的自治循环

Prime 的自治配置不止验收命令，还包含最大 continuation、最大 Agent 回合、最大 Token、总超时、单个质量门超时和重试等，并把失败输出带入下一轮；文档当前示例默认包括 3 次 continuation、12 回合、80K Token、30 分钟总时限。证据：[`usage.md`](https://github.com/PrimeIntellect-ai/prime-agent/blob/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8/packages/coding-agent/docs/usage.md)。

真实边界：CrewVela 已有质量门和重试，Prime 的增量价值是“资源预算和明确停止原因”，不是再造验收循环。

#### 不建议复制：Persistent RLM

Prime 的持久 IPython 内核能跨回合保留变量和导入，但官方明确它不是安全沙箱。CrewVela 是外部 Runtime 协调器，复制它会新增长期进程、状态恢复和任意代码执行风险。证据：[`rlm.md`](https://github.com/PrimeIntellect-ai/prime-agent/blob/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8/packages/coding-agent/docs/rlm.md)、[`architecture.md`](https://github.com/PrimeIntellect-ai/prime-agent/blob/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8/packages/coding-agent/docs/architecture.md)。

### 2. Hermes Agent

#### 源码确认：只报告“本次编辑新增错误”的 LSP

Hermes 在写文件前保存诊断基线，写后等待新鲜诊断，扣除原有错误，只把本次新增问题反馈给 Agent。实现包含 LSP 延迟启动与复用、空闲回收、语法预检、行号偏移处理、严重级别和输出数量限制；LSP 缺失或超时不会阻断写入，也不会拿过期诊断冒充最新结果。证据：[`LSP 官方文档`](https://hermes-agent.nousresearch.com/docs/user-guide/features/lsp)、[`manager.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/agent/lsp/manager.py)、[`reporter.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/agent/lsp/reporter.py)。

真实边界：无诊断不代表正确；语言服务器未安装时只能降级；不同语言的冷启动和稳定性不同。

#### 源码确认：完整会话 FTS5，而非只有摘要记忆

Hermes 把 CLI 和消息平台会话存入本地 SQLite，用 FTS5 检索原始消息，再按命中位置浏览前后文。证据：[`Memory 官方文档`](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory)、[`session_search_tool.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/tools/session_search_tool.py)、[`hermes_state.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/hermes_state.py)。

真实边界：FTS5 是全文关键词检索，不是语义向量搜索；它适合作为 CrewVela 项目知识库第一阶段和向量检索的回退路径。

#### 源码确认：后台学习审查与待审批 Skill 变更

Hermes 可在主任务后用受限后台审查器判断是否保存记忆、创建或修改 Skill，并记录来源会话、父会话、平台、任务和工具等 provenance。审查器只允许调用记忆和 Skill 管理工具；Skill 修改可暂存、查看 Diff、批准、拒绝，并跨重启保留。证据：[`Skills 官方文档`](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills)、[`background_review.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/agent/background_review.py)。

真实边界：后台审查只是提出候选，不等于候选值得批准；CrewVela 应限制触发条件，并用验证任务评估新 Skill。

### 3. Vibe Kanban

#### 源码确认：从指定执行点恢复并创建后续尝试

前端重试先检查分支和进程状态，让用户决定是否 Git reset、脏工作区是否强制，然后提交 `retry_process_id`；服务端会 `reset_session_to_process` 恢复到指定执行进程，复用 Agent session，并从相应消息继续 follow-up。证据：[`useRetryProcess.ts`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/packages/web-core/src/shared/hooks/useRetryProcess.ts)、[`sessions route`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/crates/server/src/routes/sessions/mod.rs)。同一 Workspace 还可切换多个 session，证据：[`useWorkspaceSessions.ts`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/packages/web-core/src/shared/hooks/useWorkspaceSessions.ts)。

真实边界：Vibe 已实现的是“可恢复重试与多 session”，不是自动 N 路候选、统一评分和择优合并。本报告的“替代尝试实验室”是在这一原语上为 CrewVela 新设计的能力。

#### 源码确认：Agent 执行 Profile

Vibe 为 Claude、Codex 等 Agent 提供命名 Profile。Codex Profile 可配置模型、推理、只读/工作区写/全访问沙箱、审批、指令、Planning 和命令覆盖。证据：[`codex.rs`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/crates/executors/src/executors/codex.rs)、[`default_profiles.json`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/crates/executors/default_profiles.json)、[`Agent Profiles 官方文档`](https://vibekanban.com/docs/settings/agent-configurations)。

这可作为 CrewVela“开发者、只读调查、审查者、高风险集成”等能力包 UI，但 Profile 本身不等于 Skill 来源、签名和版本治理。

#### 源码确认：逐行 Review；本报告不重复推荐

Review 数据携带文件、行号、左右侧、评论和代码行，可增删改并批量生成 Markdown 反馈给 Agent。证据：[`useReview.ts`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/packages/web-core/src/shared/hooks/useReview.ts)、[`ReviewProvider.tsx`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/packages/web-core/src/shared/hooks/ReviewProvider.tsx)、[`Review 官方文档`](https://vibekanban.com/docs/reviewing-code)。它很实用，但与此前 Orca 调研中的 Diff/Review 建议重复，所以不占本次五项名额。

#### 重大产品边界：官方云服务停止运营

官方公告说明公司已停止运营，云端 Issues、评论、项目和组织服务下线，只保留本地 Workspace 与社区维护。证据：[`官方停运公告`](https://vibekanban.com/blog/shutdown)。因此只能借鉴本地源码和协议设计，不能把 Vibe 云协作或托管 Relay 当作 CrewVela 可依赖服务。

### 4. Ruflo

#### 源码确认：全局预算保险丝

Ruflo 的 `global-ai-budget.ts` 为 daemon/worktree 的自治启动共享用户级账本，默认限制并发 1、每小时启动 2 次、每天 12 次；遇到 quota/429 可全局暂停 60 分钟。实现使用排他锁、0600 权限、拒绝符号链接，追加 launch/usage receipt，并允许人工 pause/resume。证据：[`global-ai-budget.ts`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/cli/src/services/global-ai-budget.ts)。

真实边界：这是启动次数、并发和冷却保险丝，不是精确美元计费。CrewVela 若要展示成本，仍需接各 Provider 的真实 usage。

#### 源码确认：可收缩的能力策略和一次性批准

`PolicyEngine` 具有 `legacy/observe/enforce` 模式、默认拒绝规则、预算、一次性且有作用域的人类批准、禁止自批准、子 Agent 能力包络只能收缩，以及哈希/签名回执链。CLI Policy Runtime 原子持久化状态、读取 Agent/Codex 配置，并在 MCP 工具调用入口统一授权。证据：[`policy engine`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/security/src/policy/engine.ts)、[`policy tests`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/security/__tests__/agentic-policy-engine.test.ts)、[`policy-runtime.ts`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/cli/src/services/policy-runtime.ts)、[`mcp-client.ts`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/cli/src/mcp-client.ts)。

真实边界：只有经过 CrewVela Launcher/MCP/Hook/沙箱这个真实 choke point 的调用才能强制治理；外部 CLI 内部绕过桥的命令不能靠数据库策略自动拦住。间接 Prompt 注入检测也不是所有路径默认开启。

#### 源码确认：带质量与使用统计的策略记忆

ReasoningBank 模型包含策略、领域、Embedding、质量分、使用/成功次数，并支持相似检索、去重和晋升；有 AgentDB/HNSW 时使用向量后端，无依赖时降级为内存。任务完成和多步 trajectory 可进入学习链，但普通 `memory store` 不会自动学习。证据：[`self-learning proof`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/docs/learning/self-learning-2245-proof.md)、[`self-learning usage`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/docs/learning/self-learning-usage.md)、[`ReasoningBank`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/hooks/src/reasoningbank/index.ts)。

真实边界：Ruflo 的记忆/学习存储分散，统计并未完全统一；向量依赖缺失时只是内存。CrewVela 应用自己的 SQLite 验收事实作为质量标签，不应复制多套并行记忆系统。

#### 源码确认：可学习模型路由，但不是“神经路由”

当前模型路由实质是词法复杂度分桶加 Thompson Sampling Beta-Bernoulli bandit，按 low/medium/high 桶持久化结果。证据：[`model-router.ts`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/cli/src/ruvector/model-router.ts)、[`router-bandit tests`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/cli/__tests__/router-bandit.test.ts)。

真实边界：它主要围绕 Claude 档位，初始特征是启发式。CrewVela 不能直接复制权重，必须用自身的验收率、重试、延迟和价格数据重新学习。

#### 项目级可信度边界

Ruflo 的官方质量清单承认仍有占位工具、跳过测试、文档漂移和依赖漏洞，因此本报告不采纳“323 tools”“企业级”“无限上下文”等宣传，只采纳上述有源码调用链或测试的功能。证据：[`QUALITY-SWEEP.md`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/docs/QUALITY-SWEEP.md)。

### 5. OpenAgents

#### 源码确认：短期单次配对与节点存活

OpenAgents 的 Node 接入不是手工复制永久 Token：管理员生成配对码，节点兑换后注册并获得 Workspace Token；配对码短期、单次使用，节点再独立心跳并显示在线/离线。证据：[`nodes.py`](https://github.com/openagents-org/openagents/blob/ffbd0c92ad87c5c262ba5524183994727e572079/workspace/backend/app/routers/nodes.py)。

这证明 CrewVela 以后做外部审批/远程执行节点时，可以采用“短期配对凭据 → 兑换长期受限身份 → 心跳/吊销”的结构，而不是暴露本机 UI 或共享主密钥。

#### 源码确认：Quick Tunnel，但不等于安全网关

SDK 会启动 `cloudflared tunnel --url http://localhost:PORT`，检查本地端口和 HTTP 健康，最多等待临时 `trycloudflare.com` URL，停止时终止子进程。它要求本机已安装 `cloudflared`，URL 只在进程存活时有效。证据：[`tunnel.py`](https://github.com/openagents-org/openagents/blob/ffbd0c92ad87c5c262ba5524183994727e572079/sdk/src/openagents/client/tunnel.py)。

真实边界：这是临时公网入口，不是审批 RBAC、签名回调或可靠投递。CrewVela 不应把整个本地控制面直接穿透公网。

#### 源码确认：共享 Markdown 知识 CRUD，不是语义知识库

OpenAgents 的 Workspace Knowledge 提供 Markdown 条目的创建、列出、读取、更新、软删除和事件，但源码没有 Embedding、向量索引或引用检索。证据：[`knowledge.py`](https://github.com/openagents-org/openagents/blob/ffbd0c92ad87c5c262ba5524183994727e572079/workspace/backend/app/routers/knowledge.py)。

它适合借鉴“人可编辑的共享文档”界面，不足以支撑本报告第一项推荐。

#### 明确的实验性标记

README 将 Aider 标为 Beta，并明确“离线测试通过、真实 Provider E2E 待完成”；Goose 也作为 Beta 发布。证据：[`README`](https://github.com/openagents-org/openagents/blob/ffbd0c92ad87c5c262ba5524183994727e572079/README.md)。因此不能用“已列出支持”推导所有 Runtime 都已完成生产验证。

### 6. LobeHub

#### 源码确认：混合检索、文档摄取和引用上下文

LobeHub 的资源库文档描述了上传、分块、Embedding、向量库、查询向量化和相似度召回，支持多种办公文档、文本、代码、表格和媒体；自部署文档列出 PostgreSQL + pgvector、对象存储和可选解析服务。证据：[`resource.mdx`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/getting-started/resource.mdx)、[`knowledge-base.mdx`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/self-hosting/advanced/knowledge-base.mdx)。

服务源码进一步确认：查询先做 Embedding，再按用户/Workspace 解析知识库和文件范围，调用 chunk 语义搜索；内联文档另走 BM25，两路用 `Promise.allSettled` 并行，一路失败仍返回另一路；文件按 Top-3 chunk 平均相似度排序。工具运行时还会合并 Agent 和当前任务绑定的知识库。证据：[`KnowledgeBaseSearchService`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/knowledgeBase/index.ts)、[`knowledgeBase runtime`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/toolExecution/serverRuntimes/knowledgeBase.ts)。

搜索结果格式包含文件/文档 ID、名称、相似度、相关度和片段，并引导 Agent 按 ID 读取全文；这是真正的可回溯检索上下文，而不是只把摘要塞进 Prompt。证据：[`formatSearchResults.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/packages/prompts/src/prompts/knowledgeBaseQA/formatSearchResults.ts)。

#### 源码确认：Skill/MCP 安装和按 Agent 启用

官方文档覆盖集中 Skill Store、内置/社区/自定义 MCP、HTTP 与 STDIO、连接测试、安装配置及按 Agent 启用。证据：[`skill-management.mdx`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/community/skill-management.mdx)、[`mcp-market.mdx`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/community/mcp-market.mdx)、[`custom-mcp.mdx`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/community/custom-mcp.mdx)。

Agent Builder 的工具清单把安装插件标记为始终需要用户批准，执行器支持更新 Agent 插件配置和逐个开关。证据：[`manifest.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/packages/builtin-tool-agent-builder/src/manifest.ts)、[`executor.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/packages/builtin-tool-agent-builder/src/executor.ts)。

真实边界：源码确认安装审批和逐 Agent 开关，没有确认社区 Skill 自动签名可信、STDIO MCP 有强沙箱、或自动生成 Skill 会经过回归评测。CrewVela 必须自己补版本哈希、风险分级、密钥隔离和评测晋升。

#### 源码确认：多渠道路由、白名单和一次性配对；不是项目审批工作流

LobeHub 官方渠道文档列出 Discord、Slack、Telegram、LINE、QQ、WeChat、飞书和 Lark，支持全局用户白名单、DM 策略、群组策略；Pairing 模式向未授权用户发一次性码，由属主执行 `/approve <code>` 后加入白名单。证据：[`channels overview`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/channels/overview.mdx)、[`Feishu guide`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/channels/feishu.mdx)。

配对源码使用密码学随机的 8 位码、1 小时 TTL、每 Bot 最多 50 个待处理请求、60 秒单赢家 claim；同一申请者重复请求复用原码，持久化失败会释放 claim 允许重试，成功后幂等清理。消息路由还验证属主身份再执行批准。证据：[`dmPairingStore.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/bot/dmPairingStore.ts)、[`BotMessageRouter.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/bot/BotMessageRouter.ts)。

回复回调支持消息分块、记录已投递 chunk 数、编辑失败回退为新消息、严格投递模式抛错给异步队列重试，并记录正/负投递日志。证据：[`BotCallbackService.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/bot/BotCallbackService.ts)。

真实边界：这些代码证明的是“消息渠道身份与投递”，不是“CrewVela 项目任务批准/拒绝”。要融合还需把 `waiting_approval`、验证失败、超预算等项目事件映射成签名卡片动作和审计事件。

## 最终五项融合方案

### 1. 语义化、可引用的项目知识库

**来源与证据**

- LobeHub 已实现文档摄取、分块、Embedding、向量检索、BM25 回退/并行、按 Agent/任务绑定知识库和可回溯结果格式：[`KnowledgeBaseSearchService`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/knowledgeBase/index.ts)、[`knowledgeBase runtime`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/toolExecution/serverRuntimes/knowledgeBase.ts)、[`formatSearchResults.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/packages/prompts/src/prompts/knowledgeBaseQA/formatSearchResults.ts)。
- Hermes 已实现 SQLite FTS5 原始会话检索，可做 V1 和语义检索的可靠回退：[`session_search_tool.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/tools/session_search_tool.py)、[`hermes_state.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/hermes_state.py)。
- Ruflo 的 ReasoningBank 证明任务轨迹可以带质量/成功统计进入相似检索，但其存储分散，只借鉴“验收结果作为质量标签”：[`ReasoningBank`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/hooks/src/reasoningbank/index.ts)。

**为什么不重叠**

CrewVela 的 [`ma_memory_entries`](../../src-tauri/src/multi_agent/store.rs) 是少量 `namespace/key/value` 经验，搜索条件是 `key LIKE` 或 `value LIKE`；它没有原始文档、代码/测试/Review 摄取，无法按语义找到不同措辞的相关内容，也不能把答案追溯到文件、行号、提交和哈希。

**建议架构**

```text
README / ADR / 代码符号 / 会话 / 任务交付 / 测试 / Review
                        ↓
               解析、脱敏、分块、内容哈希
                        ↓
       SQLite FTS5（精确） + Embedding 索引（语义）
                        ↓
         项目 / 任务 / Agent / 提交范围与权限过滤
                        ↓
              相关片段 + 来源 + 行号 + 哈希
                        ↓
             Agent 引用使用；点击可回到原证据
```

建议新增：

- `kb_sources`：路径、来源类型、项目、提交、内容哈希、敏感级别。
- `kb_documents` / `kb_chunks`：原文、行号范围、解析版本。
- `kb_embeddings`：模型、维度、向量、生成时间。
- `kb_citations`：哪次 Run 使用了哪些 chunk，最终结果是否验收通过。
- `kb_ingest_jobs`：增量摄取、失败和重试状态。

**分阶段落地**

1. V1（1～2 周）：SQLite FTS5 索引完整可见会话、交付物、Review、测试日志；支持查看前后文和“引用到当前任务”。
2. V2（2～4 周）：文档/代码分块、Embedding、混合检索和 RRF/加权合并；按项目、任务和提交过滤。
3. V3（1～2 周）：验收反馈调权；被 Reviewer 驳回的证据不再当作高质量经验自动注入。

**验收标准**

- 同义改写能召回同一 ADR/失败案例。
- 每个注入片段都显示来源路径、行号或消息位置、内容哈希。
- 删除/变更源文件后索引可增量失效，不能返回陈旧内容不标记。
- 任何跨项目检索都必须先通过项目权限过滤。

**复杂度与风险**

- 复杂度：中高，完整版本约 4～8 周；FTS5 V1 可先独立交付。
- 风险：Embedding 成本、秘密泄漏、旧提交污染、检索噪声、二进制解析供应链。
- 控制：默认本地索引；密钥/环境输出先脱敏；引用必带版本；Parser 与 Embedding 可插拔。

### 2. 任务级增量 LSP 诊断

**来源与证据**

Hermes 已实现编辑前基线、编辑后新鲜诊断、行号偏移处理、只报告新增 Error、输出限流和超时降级：[`LSP 文档`](https://hermes-agent.nousresearch.com/docs/user-guide/features/lsp)、[`manager.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/agent/lsp/manager.py)、[`reporter.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/agent/lsp/reporter.py)。

**为什么不重叠**

CrewVela 现有 `verification_command`、Reviewer 和 Integrator 解决的是任务完成后的验证与审查。LSP 解决的是“刚写完就发现新增的类型、符号、导入、接口错误”，而且能区分项目历史遗留错误与本次引入错误。它不是替代测试，而是把反馈提前。

**建议架构**

第一版不需要侵入 Claude Code/Codex 的每一次写文件：

1. 任务启动时对 Worktree 建立诊断基线。
2. Agent 完成一次 Run 后从 Git 取本次变更文件。
3. 只打开/诊断这些文件，等待新鲜 LSP 结果。
4. 从结果扣除基线和因行位移产生的同一诊断。
5. 新增 Error 写入 Task Event，并自动反馈给原实现 Agent；Reviewer 同时可见。
6. LSP 不可用时记录 `diagnostic_unavailable`，继续走原验证链。

建议新增：`lsp_servers`、`lsp_baselines`、`lsp_diagnostics`、`diagnostic_runs`；服务按语言和 Worktree 复用，空闲回收。

**分阶段落地**

- V1：TypeScript、Python、Rust 三类项目，任务结束后批量诊断。
- V2：通过 Runtime Hook 在 Patch 后增量诊断，做到修复回路更短。
- V3：诊断与知识库关联，把已解决的高频诊断沉淀为引用案例或 Skill 候选。

**验收标准**

- 项目原有 100 个错误不会污染本任务，只显示本次新增错误。
- 删除/插入行后，同一历史诊断不会被错误识别为新增。
- LSP 冷启动、崩溃、超时均不阻断现有任务状态机。
- 新增 Error 可触发一次有预算的修复回合，并出现在 Reviewer 证据中。

**复杂度与风险**

- 复杂度：中等，任务级 V1 约 2～4 周；逐编辑 Hook 另需 2～4 周。
- 风险：Language Server 安装发现、协议差异、冷启动、诊断过期、巨型仓库资源占用。
- 控制：先按项目显式启用；缓存服务但设置空闲 TTL；输出限量；无 LSP 必须明确降级而非假装通过。

### 3. 受控自学习 Skill/MCP 中心

**来源与证据**

- Prime 已有从失败/纠正生成 Prompt、记忆、Skill、Agent 规格修改，保留快照和回滚：[`refinement.ts`](https://github.com/PrimeIntellect-ai/prime-agent/blob/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8/packages/coding-agent/src/core/refinement/refinement.ts)。
- Hermes 已有任务后受限学习审查、provenance、Skill Diff 和跨重启待审批：[`background_review.py`](https://github.com/NousResearch/hermes-agent/blob/9829746dfe3d5077f4f076e257505ec7f8feaa65/agent/background_review.py)。
- LobeHub 已有 Skill/MCP 中心、HTTP/STDIO、自定义导入、连接测试、安装审批和逐 Agent 开关：[`skill-management.mdx`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/community/skill-management.mdx)、[`manifest.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/packages/builtin-tool-agent-builder/src/manifest.ts)。
- Ruflo 已有 `observe/enforce`、默认拒绝、一次性作用域批准、禁止自批准、子 Agent 权限只能收缩和回执链：[`policy engine`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/security/src/policy/engine.ts)。

**为什么不重叠**

CrewVela 当前能启动多个 Runtime，也有固定协作 MCP，但 ACP 初始化的 `mcpServers` 是空数组，没有一个产品层面的 Skill/MCP 清单、来源、版本、风险、审批、按 Agent 绑定和回滚体系。现有已验证记忆也不会产出可执行 Skill 变更提案。

**建议架构**

分成两个面，避免“会学习”变成“会偷偷改规则”：

1. **能力注册面**
   - `skills`、`skill_versions`、`mcp_servers`、`mcp_tools`。
   - 保存来源、版本、内容哈希、签名状态、命令/网络/写路径声明、密钥引用。
   - `agent_skill_bindings` 为每个角色/Runtime 建 allowlist；子 Agent 只能继承更小权限。
2. **学习晋升面**
   - `learning_proposals` 记录来源 Run、触发原因、候选 Diff、评测结果。
   - 只有“最终验收成功”“同类失败至少两次”“用户明确纠正”“Reviewer 标记可复用”才允许生成候选。
   - 候选先在固定 fixture/历史任务回放中评测，再由人批准为项目版；全局版需要二次批准。
   - 每次生效创建 `revision_snapshot`，支持一键回滚。

MCP/Skill 运行应先用 `observe` 模式记录将被拦截的行为，策略稳定后再切 `enforce`。外部 CLI 内部工具无法完全强制时，UI 必须标为“仅建议约束”；只有经过 CrewVela MCP/Launcher/Hook/沙箱的调用才标为“强制约束”。

**验收标准**

- 安装/升级 Skill 或 MCP 必须看到来源、Diff、声明权限、版本哈希和连接测试。
- Agent 未绑定的能力无法通过 CrewVela 调用；Reviewer 默认只读，不继承实现 Agent 的写权限。
- 模型不能批准自己的提案；一次性批准不可跨任务复用。
- 新 Skill 使回归任务变差时可自动停用并回滚到前一版本。

**复杂度与风险**

- 复杂度：注册与审批中心中高，约 3～6 周；安全自学习与回归晋升再需 4～8 周。
- 风险：恶意 Skill/MCP、命令逃逸、版本漂移、错误学习、全局污染、密钥泄漏。
- 控制：V1 只支持本地 `SKILL.md` 和 HTTP MCP；STDIO MCP 默认高风险；不做自动全局安装；密钥只引用现有系统凭据存储。

### 4. 预算化自治与质量策略

**来源与证据**

- Prime 已有 continuation、回合、Token、总时限、质量门时限和重试预算：[`usage.md`](https://github.com/PrimeIntellect-ai/prime-agent/blob/71ca6cfd1a2f7205ca0ec1baa65d10d0ed88f6e8/packages/coding-agent/docs/usage.md)。
- Ruflo 已有跨 daemon/worktree 共享并发/小时/日启动保险丝、429 冷却、人工 pause/resume 和 usage receipt：[`global-ai-budget.ts`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/cli/src/services/global-ai-budget.ts)。
- Ruflo 的 Bandit 路由有持久统计和测试，但其特征/权重不可直接迁移：[`model-router.ts`](https://github.com/ruvnet/ruflo/blob/f35c545fbe927aeb4ab8433bab8d827f69436572/v3/%40claude-flow/cli/src/ruvector/model-router.ts)。

**为什么不重叠**

CrewVela 的 `max_attempts` 只限制某个任务的尝试次数，`verification_command` 只判断结果；它不能回答“这个项目还能花多少 Token/时间/钱”“Provider 429 后所有并发任务是否应该冷却”“为什么自治停止”“是否允许换更强模型继续”。

**建议架构**

预算按四级继承：全局 → Provider/账户 → 项目 → 任务/Run。子级可更严格，不可放宽父级硬限制。

建议字段：

- `max_wall_time_ms`、`max_agent_turns`、`max_tokens`、`max_tool_calls`。
- `max_repair_rounds`、`max_parallel_runs`、`max_cost_micros`。
- `cooldown_until`、`stop_reason`、`resume_requires_approval`。
- `usage_receipts` 保存 Provider 原始 usage、估算值、币种和可信度。

执行前原子预留预算，结束后结算；崩溃的 reservation 有 TTL 回收。遇到 429/quota/余额错误触发 Provider 级冷却，而不是每个 Agent 各自重试造成雪崩。质量策略继续复用现有验收链：LSP → verification command → Reviewer → Integrator；预算不足时不自动跳过质量门，而是进入 `waiting_budget_approval`。

**验收标准**

- 多 Worktree 并发也不能突破全局/账户并发和金额上限。
- 每次停止有机器可读 `stop_reason`，用户能选择加预算、换模型、缩小范围或结束。
- Provider 未返回真实 usage 时标为估算，不能伪装成精确成本。
- 429 后只有一个统一冷却状态，不发生多个任务指数重试的“重试风暴”。

**复杂度与风险**

- 复杂度：中等，硬预算/冷却 V1 约 2～4 周；真实成本聚合和学习路由另需 3～6 周。
- 风险：不同 Runtime usage 口径不一、预留泄漏、时钟漂移、错误分类 429、低价模型导致质量退化。
- 控制：先做硬保险丝和可见统计，再做自适应路由；默认由验收通过率约束成本优化。

### 5. 任务替代尝试实验室

**来源与证据**

Vibe 已实现从指定进程恢复、脏工作区预检、显式 reset 决策和多 session：[`useRetryProcess.ts`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/packages/web-core/src/shared/hooks/useRetryProcess.ts)、[`sessions route`](https://github.com/BloopAI/vibe-kanban/blob/4deb7eca8f381f7cbc1f9d15515a9ab8f8009053/crates/server/src/routes/sessions/mod.rs)。Ruflo 的预算账本和模型路由说明“发起第二候选必须受统一预算、结果可以反哺路由”，但没有证明它已实现完整的 N 路候选择优。

**为什么不重叠**

CrewVela 当前重试是在同一任务上增加 attempt，重派会改变 assignee；它们主要是“前一个失败后再试”。实验室的关键差异是：保留原尝试，在隔离 Worktree/分支上创建多个并列候选，用同一验收包比较后只让赢家进入 Integrator。

**建议数据模型与流程**

- `task_experiments`：原任务、基准提交、目标、预算、选择策略。
- `task_candidates`：Agent、Runtime、模型、Profile、分支/Worktree、来源 checkpoint。
- `candidate_evaluations`：测试、LSP、Reviewer、Diff 风险、耗时、Token/成本、失败类型。
- `selection_decisions`：自动/人工、权重、赢家、拒绝原因。

```text
任务失败、争议大或高风险
          ↓
以同一基准提交创建 A/B 候选 Worktree
          ↓
不同 Agent / 模型 / Prompt / Profile 独立执行
          ↓
同一 LSP + 测试 + Reviewer 评测包
          ↓
展示差异、质量、成本、风险；选择一个赢家
          ↓
只有赢家交给现有 Integrator，其他候选归档
```

第一版只做人工触发的 A/B 两路，不自动大规模采样；只有剩余预算充足时才允许第二路。默认创建新 Worktree，不执行覆盖式 `git reset`，以保护用户脏改动。

**验收标准**

- 两个候选必须基于同一提交和同一验收包，评测结果可复现。
- 候选间文件系统、分支、消息和工具权限隔离。
- 未获胜候选不会进入主分支；但失败证据可进入项目知识库。
- 选择结果反哺 Runtime/模型在该类任务上的一次验收率、成本和延迟统计。

**复杂度与风险**

- 复杂度：中高，人工 A/B V1 约 3～5 周；自动评分与学习路由再需 4～8 周。
- 风险：成本翻倍、非确定性测试、比较基准不一致、候选交叉污染、错误自动合并。
- 控制：同一基线/验收包；预算硬门；赢家仍走现有 Integrator；自动选择初期只给建议不直接合并。

## 第六候选：外部监督与审批网关

它没有进入最终五项，不代表不值得做。LobeHub 已有多渠道、用户/群组白名单、属主批准的一次性配对和分块投递；OpenAgents 已有节点短期单次配对与心跳。这些是可靠的一手实现依据：[`LobeHub channels`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/docs/usage/channels/overview.mdx)、[`dmPairingStore.ts`](https://github.com/lobehub/lobehub/blob/91919550877cf4a63ca259b8dda3e01bd819c074/apps/server/src/services/bot/dmPairingStore.ts)、[`OpenAgents nodes.py`](https://github.com/openagents-org/openagents/blob/ffbd0c92ad87c5c262ba5524183994727e572079/workspace/backend/app/routers/nodes.py)。

建议未来只做监督面，而不是远程终端：

- 飞书或 Telegram 单渠道起步。
- 只推送 `waiting_approval`、验证失败、超预算、任务完成。
- 只允许批准、拒绝、评论、暂停；不能直接执行任意 Shell。
- 短时配对、用户白名单、回调签名、nonce、幂等键、投递队列和审计日志。
- 本地离线时事件入 SQLite outbox，恢复后重试；重复回调不得重复批准。

复杂度约 3～5 周/单渠道；主要风险是公网暴露、身份错绑、回放攻击、重复投递和秘密泄漏。它应建立在统一审批事件、Policy Engine 和预算停止原因之后，因此排在第二阶段。

## 明确排除的重叠或不成熟能力

| 能力 | 排除原因 |
|---|---|
| 普通多 Agent / Swarm / 子 Agent | CrewVela 已有多 Runtime Agent、角色、任务分配；Ruflo/OpenAgents 的 Swarm 不是新缺口。 |
| 任务图、依赖、并行调度 | CrewVela 已有任务 DAG、依赖和并行执行。 |
| Worktree/分支隔离 | CrewVela 已有；Vibe 的 Workspace/Worktree 只作为替代尝试的实现证据。 |
| Agent 消息、交接、共享交付物 | CrewVela 已有持久消息、交付摘要和协作 MCP。 |
| Reviewer、Integrator、普通质量门 | CrewVela 已有；推荐的是增量 LSP和统一预算，不是复制 Review。 |
| 定时自动化、Daemon、心跳 | CrewVela 已有自动化；OpenAgents 心跳只用于未来外部节点身份。 |
| 普通长期记忆 | CrewVela 已有已验证记忆；推荐的是原始证据摄取、全文/语义检索和引用。 |
| 语音 | CrewVela 已有橙子和实时语音链路。 |
| Diff/逐行 Review、GitHub/CI、Browser | 此前 Orca 调研已覆盖，本报告只把 Vibe Review 作为交叉证据，不重复占名额。 |
| SSH/远程执行、容器沙箱 | Hermes 做得更完整，但 CrewVela 已有 Worktree/Codex Sandbox，且此前 Orca 已涉及 SSH；待不可信仓库需求明确再做统一环境抽象。 |
| Prime Persistent RLM | 官方明确不是安全沙箱，且 CrewVela 不拥有统一推理循环。 |
| OpenAgents Markdown Knowledge | 只有 CRUD，不是语义检索，不能代替推荐 1。 |
| Vibe 云协作/托管 Relay | 官方公司/云服务已停运，不应形成新依赖。 |
| Ruflo “323 tools”“神经路由”“无限上下文”等宣传 | 官方质量清单承认占位、测试跳过和文档漂移；未被固定源码调用链确认的宣传不进入建议。 |

## 实施顺序

### 第一阶段：先补质量与证据底座（约 4～8 周，可并行）

1. Hermes 式任务级增量 LSP，先支持 TypeScript/Python/Rust。
2. 项目知识库 V1：SQLite FTS5 索引会话、交付、测试、Review，并支持引用回原文。
3. 全局/Provider/项目/任务四级硬预算和 429 冷却。

这三项不改变现有 Claude 开发、Codex 测试与审核、Integrator 验收的工作方式，只会让错误更早暴露、证据更易找到、成本不会失控。

### 第二阶段：把能力治理起来（约 6～10 周）

1. Skill/MCP 注册、版本哈希、逐 Agent 绑定、`observe` 审计。
2. 用户审批、一次性作用域授权和回滚快照。
3. 知识库加入 Embedding、混合检索、文档摄取和验收反馈。

### 第三阶段：有数据后再自治优化（约 6～12 周）

1. 人工触发 A/B 替代尝试，统一评测、人工择优。
2. 从验收率、成本、重试和延迟学习 Runtime/模型路由。
3. 任务后受限学习审查，只产出 Skill 候选；通过历史任务回放与人工批准后晋升。
4. 需要移动办公时，再接单一外部监督渠道。

## 最关键的产品边界

- 不把“模型写出一个新 Skill”叫作学习成功；只有回归评测和人工批准后才算晋升。
- 不把“能存 Markdown/记忆”叫作语义知识库；必须有摄取、检索、范围控制和引用。
- 不把“重试/多 session”叫作替代方案竞赛；必须并列保留、同基线评测并选择赢家。
- 不把“Quick Tunnel/聊天机器人”叫作远程审批；必须有身份、作用域、签名、幂等、可靠投递和审计。
- 不把“显示 Token 数”叫作预算；预算必须在启动前原子拦截，并给出明确停止原因。
- 不复制六个项目的完整架构。CrewVela 应保留现有 SQLite、Worktree、任务图、Reviewer、Integrator 和人工验收，在这些可靠原语之上补能力。

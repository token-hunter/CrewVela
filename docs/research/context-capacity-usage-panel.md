# CrewVela 上下文容量与额度面板调研

日期：2026-08-18

## 结论

建议实现，优先级为高。CrewVela 同时管理多个 Runtime 和并行会话，用户需要在会话卡死或模型自动压缩前知道“当前上下文还剩多少”，也需要在批量启动 Agent 前知道账号额度是否足够。这个功能直接减少上下文溢出、无效重试、不同窗口额度语义混乱和并行任务中途停摆。

但不应照搬截图中的所有字段。统一面板只能展示各 Runtime 能可靠提供的交集；分类明细必须带数据来源和可信度。首版应以“当前上下文、输入/输出/缓存、额度窗口”为核心，不应伪造“消息、系统工具、技能、系统提示词、MCP 工具”等分类占比。

推荐产品形态：

- 会话标题栏放一个紧凑指示器：`上下文 43% · 360k / 828k`。
- 点击后打开悬浮卡，显示当前上下文、最近一次响应的 Token 分类、缓存命中率、压缩阈值和账号额度。
- 所有数值附 `实时 / 最近响应 / 累计 / 估算 / 不可用` 标签及更新时间。
- 主界面可增加所有会话的风险排序：上下文将满、额度将尽、数据过期。
- 额度百分比统一显示“剩余”，内部保留原始 `usedPercent`，避免把“已用 100%”错误显示成“剩余 100%”。

## CrewVela 现状

当前 `src-tauri/src/usage.rs` 只增量扫描 Claude transcript 的 `assistant.message.usage`，累计：

- `input_tokens`
- `output_tokens`
- `cache_creation_input_tokens`
- `cache_read_input_tokens`

前端 `SessionUsage` 也只有这四项。该值是**会话累计调用量**，不是当前上下文占用；当前实现还把四项直接相加为 total。若把它除以模型上下文窗口，会随着轮次增长而超过 100%，不能用于截图中的“上下文容量”。

因此需要新增独立数据模型，至少区分：

```text
currentContext      当前模型请求实际携带的上下文快照
lastTurnUsage       最近一轮/最近响应 Token
sessionCumulative   会话生命周期累计 Token
accountQuota        账号级滚动额度
freshness           数据产生时间与采集方式
confidence          exact | provider_reported | estimated | unavailable
```

## 各 Runtime 的真实数据边界

### Claude Code

官方 `statusLine` stdin 已提供最完整且稳定的接入面：

- `context_window.context_window_size`
- `context_window.used_percentage` / `remaining_percentage`
- `context_window.current_usage.input_tokens`
- `output_tokens`
- `cache_creation_input_tokens`
- `cache_read_input_tokens`
- `rate_limits.five_hour` / `seven_day` 的 `used_percentage` 与 `resets_at`

来源：[Claude Code 官方 status line 文档](https://code.claude.com/docs/en/statusline)。官方说明 `current_usage` 在首次 API 调用前以及 `/compact` 后下一次调用前为 `null`；订阅额度只对 Claude.ai Pro/Max 等场景出现，每个额度窗口也可能缺失。

可可靠展示：当前上下文百分比、当前输入、输出、缓存读写、5 小时/7 天额度与重置时间、会话累计成本（成本为客户端估算）。

不可可靠展示：截图中“消息 / 系统工具 / 技能 / 系统提示词 / MCP 工具”的精确 Token 分类。Claude Code 的 `/context` 内部能计算更细分类，但官方 status line 合约没有暴露该 breakdown；[官方仓库请求 #49022](https://github.com/anthropics/claude-code/issues/49022) 已说明这一缺口。首版不得自行按字符串长度伪装成精确值。

实现建议：为 CrewVela 管理的 Claude 会话注入一个极轻量 status-line sink，将 JSON 快照写入 CrewVela 专用目录或命名管道；不要继续用累计 transcript 推算当前上下文。若不希望修改用户全局配置，只对 CrewVela 启动的子进程使用临时 settings/config 层。

许可边界：Claude Code 核心 CLI 不是开源实现，其官方仓库许可证是 Anthropic proprietary/commercial terms；CrewVela 应只依赖公开 statusLine 合约，不复制闭源实现。

### Codex CLI

Codex 的官方 app-server 是最适合 CrewVela 的稳定接口：

- `thread/tokenUsage/updated`：当前线程的总计与最近活动上下文 Token 快照；包含输入、缓存输入、缓存写入、输出、推理输出和模型上下文窗口。
- `account/rateLimits/read`：`usedPercent`、`windowDurationMins`、`resetsAt`、额度到达类型、credits、planType；`account/rateLimits/updated` 提供稀疏更新。
- `account/usage/read`：账号 Token 活动摘要与每日桶。

来源：[Codex app-server 官方 README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)、[官方 token usage 实现](https://github.com/openai/codex/blob/main/codex-rs/tui/src/token_usage.rs)、[测试中的协议形状](https://github.com/openai/codex/blob/main/codex-rs/app-server/tests/common/rollout.rs)。

Codex 明确区分：`last_token_usage.total_tokens` 是最新活动上下文大小；`total_token_usage` 是会话累计量。TUI 计算上下文剩余时还会扣除 baseline/effective window，不能把累计 total 当当前上下文。

Codex/OpenAI 的 `inputTokens` 通常已经包含 cached/cache-write 输入，cache 字段是分解项，不得再次加到 input 计算请求总量；CrewVela 当前四项直接相加的算法不能复用于 Codex。

可可靠展示：当前上下文、累计/最近轮输入输出、缓存输入、推理输出、模型窗口、5 小时/周额度、重置时间、credits。

数据限制：某些账号或后端可能返回 `primary`/`secondary` 为 `null`；`account/rateLimits/updated` 是稀疏更新，客户端必须与最近完整快照合并，不能用 null 清空旧字段。账号额度是共享额度，不应归因到单个会话。

### Kimi Code

应以当前 TypeScript 版 [MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code) 为准，而不是逐步退役的 Python `kimi-cli`。其本地 usage 将四类输入定义为互斥项：`inputOther`、`inputCacheRead`、`inputCacheCreation`，另有 `output`；当前上下文应读取 `agent.getContext().tokenCount`，会话累计应读取 `agent.getUsage()`，二者不能混用。官方示例：[context-usage.ts](https://github.com/MoonshotAI/kimi-code/blob/a7dc1ea28445555d5944066936fdf6e1b21d27ea/packages/klient/examples/context-usage.ts)，Token 算法：[usage.ts](https://github.com/MoonshotAI/kimi-code/blob/a7dc1ea28445555d5944066936fdf6e1b21d27ea/packages/kosong/src/usage.ts)。

当前 ACP server 可发送 `usage_update { used: context.tokenCount, size: maxContext }`，适合上下文进度条，但 ACP 不提供完整 cache/cost breakdown；完整数据应走 Kimi SDK/内部事件。来源：[ACP events-map](https://github.com/MoonshotAI/kimi-code/blob/a7dc1ea28445555d5944066936fdf6e1b21d27ea/packages/acp-server/src/events-map.ts)。

Kimi 托管套餐有结构化额度接口：OAuth Bearer 请求 `https://api.kimi.com/coding/v1/usages`，返回 named windows、used/limit/resetTime 与 Booster wallet；官方客户端实现：[managed-usage.ts](https://github.com/MoonshotAI/kimi-code/blob/a7dc1ea28445555d5944066936fdf6e1b21d27ea/packages/oauth/src/managed-usage.ts)。该接口只适用于 `managed:kimi-code`，第三方 API Provider 不适用，也不能与 Moonshot Open Platform 额度混为一谈。

### Pi Agent

Pi 的公开扩展 API 直接提供 `getContextUsage()`：

```ts
{
  tokens: number | null;
  contextWindow: number;
  percent: number | null;
}
```

来源：[Pi 扩展类型定义](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/src/core/extensions/types.ts)。每条 assistant message 的标准 `usage` 还包含 `input`、`output`、`cacheRead`、`cacheWrite`、`totalTokens` 与成本；模型本身提供 context window。Pi 的压缩规则为 `contextTokens > contextWindow - reserveTokens`，可据此额外显示“距离自动压缩还剩多少”，来源：[Pi compaction 文档](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/compaction.md)。

更适合 CrewVela 的结构化接入是 Pi RPC `get_session_stats`：一次返回会话累计 tokens/cost 与当前 `contextUsage`。官方说明 context 为估计值，compact 后到下一条 assistant usage 产生前可为 null；因此 UI 应标为“估算/最近响应”。来源：[Pi RPC 文档](https://github.com/badlogic/pi-mono/blob/6db110e6fab3a18e0e54a6e1aa5a479c1e282924/packages/coding-agent/docs/rpc.md)。

可可靠展示：当前上下文、窗口、输入/输出/缓存读写、累计成本、压缩阈值。

额度限制：Pi 是多 Provider harness，没有统一账号额度协议。只有底层 Provider 明确返回 rate-limit/订阅额度时才展示；否则显示不可用。

### OpenCode

OpenCode 的消息模型和存储已经标准化记录：input、output、reasoning、cache.read、cache.write；模型目录提供 `model.limit.context`，TUI/compaction 由当前 prompt footprint 与模型限制计算百分比。

可复用路径：

- `packages/opencode/src/session/message-v2.ts`：消息 Token 模型和 prompt/total size 语义。
- `packages/opencode/src/session/compaction.ts` / overflow 相关模块：上下文阈值。
- `packages/llm/src/schema/events.ts`：Provider usage 归一化。
- `packages/llm/DESIGN.md`：使用量、缓存与成本的设计约束。

官方源码：[anomalyco/opencode](https://github.com/anomalyco/opencode)。

可可靠展示：Provider 报告的 input/output/reasoning/cache、当前上下文比例、累计成本（成本必须标“估算”）。

数据限制：OpenCode 支持大量 Provider；模型上下文或缓存字段可能缺失/语义不同。模型元数据来自 models.dev 或用户覆盖，属于配置值，不一定是后端实时授权窗口。订阅额度不存在跨 Provider 的统一接口。

OpenCode ACP 的 context `used` 只统计 input + cache.read + cache.write；App UI 目前会把最近 assistant 的 output/reasoning 也纳入百分比，二者口径不同。其类别 breakdown 使用字符数/4 后缩放，并非 tokenizer 真值，若借鉴只能明确标为估算。官方源码：[ACP usage](https://github.com/anomalyco/opencode/blob/2cba7e227d68a7e7e4a2aa9c85b808e8ecb14daf/packages/opencode/src/acp/usage.ts)、[App context metrics](https://github.com/anomalyco/opencode/blob/2cba7e227d68a7e7e4a2aa9c85b808e8ecb14daf/packages/app/src/components/session/session-context-metrics.ts)、[breakdown 估算](https://github.com/anomalyco/opencode/blob/2cba7e227d68a7e7e4a2aa9c85b808e8ecb14daf/packages/app/src/components/session/session-context-breakdown.ts)。

## 最值得参考的开源实现

| 项目 | 可复用内容 | 关键路径/接口 | 许可证 | 成熟度 |
|---|---|---|---|---|
| [openai/codex](https://github.com/openai/codex) | 当前上下文与累计 Token 分离、额度完整快照+稀疏更新、状态栏算法 | `codex-rs/app-server/README.md`、`codex-rs/tui/src/token_usage.rs`、`app-server-protocol` | Apache-2.0 | 高；优先直接接协议 |
| [MoonshotAI/kimi-code](https://github.com/MoonshotAI/kimi-code) | 上下文与累计 usage 分离、缓存 breakdown、托管套餐 usage、ACP context update | `packages/kosong/src/usage.ts`、`packages/klient/examples/context-usage.ts`、`packages/oauth/src/managed-usage.ts`、`packages/acp-server/src/events-map.ts` | MIT | 高；优先 SDK，ACP 只取上下文 |
| [earendil-works/pi](https://github.com/earendil-works/pi) | 统一 `getContextUsage()` 扩展接口、消息级 usage、有效压缩阈值 | `packages/coding-agent/src/core/extensions/types.ts`、`docs/compaction.md` | MIT | 高；接口最适合做 Runtime adapter |
| [anomalyco/opencode](https://github.com/anomalyco/opencode) | 多 Provider Token 归一化、cache/reasoning、context limit、成本估算 | `packages/opencode/src/session/message-v2.ts`、`session/compaction.ts`、`packages/llm/src/schema/events.ts` | MIT | 高；适合复用领域模型与计算规则 |
| [benabraham/claude-code-status-line](https://github.com/benabraham/claude-code-status-line) | Claude statusLine stdin 解析、上下文/额度进度条、窄宽度布局与缓存 | `claude-code-status-line.py` | MIT | 中高；适合借鉴展示与防抖，不应代替官方数据合约 |

## 推荐架构

不要写一个“通吃所有终端输出”的正则解析器。应新增 `RuntimeTelemetryAdapter`：

```ts
interface RuntimeTelemetrySnapshot {
  runtime: RuntimeKind;
  sessionId: string;
  model?: string;
  context?: {
    usedTokens?: number;
    maxTokens?: number;
    usedPercent?: number;
    effectiveMaxTokens?: number;
  };
  lastTurn?: TokenBreakdown;
  cumulative?: TokenBreakdown;
  cacheHitPercent?: number;
  quotaWindows?: QuotaWindow[];
  capturedAt: number;
  source: "statusline" | "app-server" | "wire" | "sdk" | "transcript" | "estimate";
  confidence: "exact" | "provider_reported" | "estimated" | "unavailable";
}
```

适配器优先级：

1. Claude Code：statusLine JSON；transcript 仅补累计值。
2. Codex：app-server `thread/tokenUsage/updated` + `account/rateLimits/read/updated`。
3. Kimi Code：Kimi SDK/事件获取完整 usage；ACP 只获取 `used/size` 上下文。
4. Pi：extension/SDK `getContextUsage()` + message usage。
5. OpenCode：server/session API 或会话事件；模型目录只负责最大窗口元数据。

所有 adapter 输出同一快照，但 UI 根据字段存在性逐项渲染，不能用 0 代替缺失值。

## 分期建议

### 第一阶段：可用且可信

- 先支持 Claude Code、Codex、Kimi Code 三个主要 Runtime。
- 标题栏显示当前上下文百分比和 used/max。
- 悬浮卡显示最近一轮 input/output/cache read/cache write。
- Codex 与 Claude 显示官方额度窗口和重置时间；其余 Runtime 无结构化额度时显示不可用。
- 缓存命中率使用 `cacheRead / (freshInput + cacheRead + cacheWrite)`，仅当 Provider 语义明确时计算。
- 加 `capturedAt` 与“最近响应”提示，避免把过期快照当实时额度。

### 第二阶段：统一 Runtime adapter

- 加入 Pi、OpenCode。
- 增加全局“上下文风险”列表、70/85/95% 告警、自动压缩入口。
- 区分绝对模型窗口与有效压缩窗口。
- SQLite 持久化短期时序数据，用于趋势而不是账单。

### 第三阶段：分类与优化建议

- 只有 Runtime 提供可验证分类时才展示 system/messages/tools/skills/MCP。
- 对估算数据明确标注“估算”，允许用户关闭。
- 增加缓存效率、单位任务 Token、并行 Agent 总消耗等分析。

## 明确禁止的错误实现

- 不得把会话累计 Token 除以模型窗口当成当前上下文。
- 不得把 API 的 `usedPercent` 直接标成“剩余”；剩余必须计算 `100 - usedPercent`。
- 不得将 `null`、缺字段或尚未产生首个响应显示成 `0%`。
- 不得把账号额度分摊到单个会话，除非 Provider 明确提供归因。
- 不得通过字符数粗估后显示为精确 Token。
- 不得把 cache read 在 input 已包含它的 Provider 中重复相加；必须按每个 Runtime 的官方语义适配。
- 不得承诺所有 Runtime 都有 5 小时/周额度；Pi、OpenCode 的额度取决于底层 Provider。

## 最终建议范围

该功能有必要，但第一版应命名为“上下文与用量”，不要叫“额度统计中心”。优先把**当前上下文是否将满**做准，再展示**最近一轮 Token 与缓存**，最后接入**账号额度**。截图中的细粒度分类可作为 Kimi/未来协议支持后的增强项，不应成为首版阻塞点，也不应靠猜测补齐。

# Mirasim 核心功能调研与 CrewVela 融合适配分析

> 研究日期：2026-10-01
> 对象：Mirasim（mirasim.ai，下文按任务简称「Mira」；官方全称 Mirasim，X 账号 @Mirasim_ADE）— 自称「One Agent IDE」的多智能体开发工作台
> 证据规则：全部事实来自 mirasim.ai 官网页面与官方文档站（2026-10-01 实时抓取，站点当日版本 v0.0.383，见 [下载页](https://mirasim.ai/download)、[更新日志](https://mirasim.ai/changelog/zh)）；每条陈述标注来源 URL；「官网宣称」与「无法独立验证」分开标记；查不到的明确写「未查到/未能验证」；推断单独标注「推断」。未使用搜索引擎二手总结。
> 本地参照：`.`（README.md、CLAUDE.md、package.json、src-tauri/src/ 模块、src/components/ 组件、src/types.ts）及 `docs/research/clawling-core-features-crewvela-fit-analysis.md`（结构与风格模板）。

## 1. TL;DR

Mirasim 是一款闭源的「Agent IDE」桌面应用（macOS/Windows/Linux + Android/iOS + CLI），核心主张是把「构建→评测→迭代」闭环成一个产品：多个 Claude Code / Codex / 内置智能体在并行窗格（各自独立 worktree）里写代码，再用 `$eval` 派出仿真用户在真实浏览器/手机/命令行上使用产出物，用证据决定下一轮（[核心概念](https://mirasim.ai/docs/zh/concepts)、[关于我们](https://mirasim.ai/about)）。它与 CrewVela 的重叠度是本系列调研中最高的一次——队列、worktree、多运行时、Ask/Allow/Deny 审批、会话间消息、定时任务、Skills/MCP 双方都已实现，且多项设计哲学相同（「只改模型流量路径、不改智能体行为」对 CrewVela 的 env 注入直连；「多会话不说多智能体」对 CrewVela 的无调度协作）。真正值得抄的是四个 CrewVela 尚未语义化的细节：**会话队列的引导/插队/撤回三操作与队列项配置锁定**、**会话互联从 CLI 子命令升级为一等 agent 工具组（list/read/send/start + 状态感知投递 + 限流熔断 + JSONL 账本）**、**本机成本账本（token→成本的五维分摊与如实标注）**、**收件箱的「不在这里的事不用看」承诺**。明确不建议：云 Pro/额度兜底 relay（经济系统+云依赖）、本地代理抓包式记录（与 CrewVela「无中间代理」定位冲突）、全量 IM 渠道与移动端配对。

## 2. 调研范围与方法

### 2.1 链接发现

- [sitemap.xml](https://mirasim.ai/sitemap.xml) 完整抓取（65 个 URL，含 hreflang 双语对）；
- 文档侧边栏（每页 18 章导航）与 sitemap 交叉验证，两者完全一致；`https://mirasim.ai/docs` 索引返回 308 跳转（非独立页面）。

### 2.2 完整 URL 清单与抓取结果

**文档树（18 篇/语言 × 2，全部抓取成功，无一失败）**：

| 章 | 英文 URL | 中文 URL（本报告主引） | 结果 |
|---|---|---|---|
| 01 | https://mirasim.ai/docs/quickstart | https://mirasim.ai/docs/zh/quickstart | 成功 |
| 02 | https://mirasim.ai/docs/workbench | https://mirasim.ai/docs/zh/workbench | 成功 |
| 03 | https://mirasim.ai/docs/concepts | https://mirasim.ai/docs/zh/concepts | 成功 |
| 04 | https://mirasim.ai/docs/sessions | https://mirasim.ai/docs/zh/sessions | 成功 |
| 05 | https://mirasim.ai/docs/queue | https://mirasim.ai/docs/zh/queue | 成功 |
| 06 | https://mirasim.ai/docs/branches | https://mirasim.ai/docs/zh/branches | 成功 |
| 07 | https://mirasim.ai/docs/messaging | https://mirasim.ai/docs/zh/messaging | 成功 |
| 08 | https://mirasim.ai/docs/layout | https://mirasim.ai/docs/zh/layout | 成功 |
| 09 | https://mirasim.ai/docs/workspace | https://mirasim.ai/docs/zh/workspace | 成功 |
| 10 | https://mirasim.ai/docs/models | https://mirasim.ai/docs/zh/models | 成功 |
| 11 | https://mirasim.ai/docs/usage | https://mirasim.ai/docs/zh/usage | 成功 |
| 12 | https://mirasim.ai/docs/skills-tools | https://mirasim.ai/docs/zh/skills-tools | 成功 |
| 13 | https://mirasim.ai/docs/orchestrate | https://mirasim.ai/docs/zh/orchestrate | 成功 |
| 14 | https://mirasim.ai/docs/evaluation | https://mirasim.ai/docs/zh/evaluation | 成功 |
| 15 | https://mirasim.ai/docs/surfaces | https://mirasim.ai/docs/zh/surfaces | 成功 |
| 16 | https://mirasim.ai/docs/cli | https://mirasim.ai/docs/zh/cli | 成功 |
| 17 | https://mirasim.ai/docs/privacy | https://mirasim.ai/docs/zh/privacy | 成功 |
| 18 | https://mirasim.ai/docs/reference | https://mirasim.ai/docs/zh/reference | 成功 |

中英两树字节级规模一致（逐篇对比，如 messaging zh=11.9KB / en=12.4KB），抽样精读确认英文树为中文树的忠实镜像，无英文独有章节。

**产品与营销页**：首页 https://mirasim.ai/ 、[下载](https://mirasim.ai/download)、[定价](https://mirasim.ai/pricing)、[关于](https://mirasim.ai/about)、[公告（2026-08-09）](https://mirasim.ai/announcement/zh)、[更新日志](https://mirasim.ai/changelog/zh)、[帮助](https://mirasim.ai/help/zh) — 全部成功精读。写作栏目 5 篇（[what-is-an-agent-ide](https://mirasim.ai/writing/zh/what-is-an-agent-ide)、[claude-code-and-codex-one-place](https://mirasim.ai/writing/zh/claude-code-and-codex-one-place)、[parallel-coding-agents-git-worktrees](https://mirasim.ai/writing/zh/parallel-coding-agents-git-worktrees)、[simulated-users-agent-work](https://mirasim.ai/writing/zh/simulated-users-agent-work)、[what-agentic-coding-costs](https://mirasim.ai/writing/zh/what-agentic-coding-costs)）及英文版 — 抓取成功，标题与论点已读。法律三页（[ToS](https://mirasim.ai/terms-of-service)、[Privacy Policy](https://mirasim.ai/privacy-policy)、[AUP](https://mirasim.ai/acceptable-use-policy)）抓取成功，仅提取主体与竞业条款，未逐条分析。

**低价值页（抓取成功、备查未逐条分析）**：[blog](https://mirasim.ai/blog)（实为阅读清单：Generative Agents、Voyager 等论文）、[careers](https://mirasim.ai/careers)（招聘：product engineering intern、agent systems builders）、[writing 索引](https://mirasim.ai/writing/zh)。

**失败项**：无 URL 失败。方法备注：MCP webReader 对该站正文返回被截断，遂改用 curl 直抓 SSR HTML + 本地脚本提取 `<main class="guide-page">` 正文（页面为服务端渲染，提取完整）。首页存在 GitHub 占位链接（`github.com/o/r`，为 ui-cli 示例参数），**全站无任何真实开源仓库链接**——Mirasim 未开源，无 README 可读。

## 3. Mira 项目概述

### 3.1 一句话定位

「One Agent IDE for Agentic Coding and Eval」——官方自我定位是「一个以本机为主、支持多智能体协同的开发空间」，组织方式是一条循环而非功能清单：**智能体完成开发，仿真用户实际使用产出结果，取得的证据决定下一步做什么**（[核心概念](https://mirasim.ai/docs/zh/concepts)，官网宣称）。首页口号 "All agents. In one place."（[首页](https://mirasim.ai/)）。

### 3.2 目标用户与形态

- 目标用户：同时驾驭多个编程智能体（Claude Code / Codex / 内置智能体）与自有订阅/API key 的开发者（[关于我们](https://mirasim.ai/about)：agents build + simulated users + evidence decides 三段论）。
- **形态：桌面应用为主体**（macOS Apple 芯片 / Windows 10 1809+ / Linux AppImage+deb，x64+arm64），另有 Android APK（8.0+，正式签名）、iPhone TestFlight 内测（iOS 15+，未上架 App Store）、独立 CLI+TUI（darwin-arm64 tar.xz，含 ui-cli 控制接口）（[快速上手](https://mirasim.ai/docs/zh/quickstart)、[下载页](https://mirasim.ai/download)，官网宣称）。
- **服务端-客户端架构**：一个 `mirasim serve` 进程持有 SessionHost，所有真实数据（项目文件、Git worktree、会话历史、产物、录制、定时任务、用量数据）都由它持有；桌面端通常兼任本机服务端，也可以指向另一台 macOS/Linux 机器（含 SSH 部署路径），手机/聊天渠道/TUI 都只是它的客户端（[核心概念](https://mirasim.ai/docs/zh/concepts)、[各端使用](https://mirasim.ai/docs/zh/surfaces)，官网宣称）。

### 3.3 运营主体、商业模式与开源状态

- 运营主体：法律页仅表述 "owned, operated, and provided by us and our affiliates"，**未披露公司实体名称**（[ToS](https://mirasim.ai/terms-of-service)）→ 运营主体未查到。页脚有 EverMind 友链（https://evermind.ai/），与 Mirasim 的关系未能验证。
- 商业模式（[定价页](https://mirasim.ai/pricing)，2026-10-01 抓取）：Free（BYOK 自带 key）→ Go $16/月（原价 $18，特定模型不限请求数：Kimi K3 ≈130 次/5h、GLM 5.3 Flash ≈1900、DS 4.1 Flash ≈7800）→ Basic $29/月（≈1 份 Claude Code Pro 或 Codex Plus $20）→ Pro $169/月（≈Claude Max 5x 或 Codex Pro 5x）→ Max $299/月（≈20x 档）。邀请裂变：邀 3 人得 1 个月 Go，受邀者 $8/月。定价页另设「Fingerprint the model」章节，给出四条自证路线（同 prompt 对照、公开探针集、逐轮路由标注、承诺不识别探针流量）。
- 开放节奏（[公告 2026-08-09](https://mirasim.ai/announcement/zh)）：内测期邀请码传播失控后宣布调整——**邀请码只限制由 Mira 补贴成本的「云端 Pro」**；本地账号接入（连自有 Claude Code/Codex 账号或模型服务）永久免费开放。帮助页确认「必须使用自己的密钥吗——按设计你使用的是自己的账号；中转服务只是额度兜底的选项，不是必需项」（[帮助](https://mirasim.ai/help/zh)）。
- 开源状态：**闭源**。全站无 GitHub 仓库链接；产物有 SHA256SUMS 校验（[快速上手](https://mirasim.ai/docs/zh/quickstart)）。License：未查到（非开源分发）。
- 迭代速度：正式版逐日发布（2026-08-15 至 2026-09-29 期间 37 个正式版，当前 v0.0.383）（[更新日志](https://mirasim.ai/changelog/zh)）。

### 3.4 产品哲学（对适配分析重要）

- **「只改一件事」**：通过 Mira 运行 Claude Code 或 Codex 时，只改变它们的模型流量走哪条路径；中间层原样转发请求与响应正文、不做解析，智能体自身终端界面与登录状态被原样保留（[模型、账号与额度](https://mirasim.ai/docs/zh/models)；[只改一件事](https://mirasim.ai/writing/zh/claude-code-and-codex-one-place)）。
- **刻意不说「多智能体」**：官方写明「我们说多会话，不说多智能体——多智能体听起来像在许诺编排能力，而我们宁愿少被搜到，也不去描述一件自己没做的事」（[what-is-an-agent-ide](https://mirasim.ai/writing/zh/what-is-an-agent-ide)）。会话互联一节同样自我设限：「这是一条消息通路，不是编排引擎——没有等待原语、没有依赖图、没有优先级」（[会话互联](https://mirasim.ai/docs/zh/messaging)）。

## 4. 核心功能拆解（逐条带来源）

### 4.1 工作台骨架：服务端 / 工作区 / 会话 / 窗格 / 分组

| # | 功能 | 说明 | 来源 |
|---|---|---|---|
| 1 | 构建-评测-迭代循环 | 产品组织为循环：构建（多智能体并行）→ 评测（$eval 仿真用户）→ 迭代（证据转下一批任务）；「阅读源码不计入用户证据」 | [concepts](https://mirasim.ai/docs/zh/concepts)（官网宣称） |
| 2 | 工作区=添加进来的目录 | 侧栏即清单，全新安装显示「添加工作区」；会话在某目录启动会自动补进清单 | [concepts](https://mirasim.ai/docs/zh/concepts) |
| 3 | 会话三元组 | 一个会话 = 一个智能体 + 一个模型 + 一档推理强度，运行在某工作区目录；每会话独立持有对话记录、指令队列、待办清单和终端；相邻会话可用不同模型 | [concepts](https://mirasim.ai/docs/zh/concepts) |
| 4 | 三层布局 | 窗格区是分屏树（4 种末端内容：会话/终端/工作区面板/文档）；**分组=多套保存的布局，同一会话可同时出现在多个分组**；窗格内可叠标签，隐藏标签的会话继续运行 | [layout](https://mirasim.ai/docs/zh/layout) |
| 5 | 命令面板 ⌘K | 一个输入框检索三类内容：会话、工作台命令、**全部已记录对话的全文**（带进度与覆盖率汇报，引号精确匹配） | [layout](https://mirasim.ai/docs/zh/layout)、[reference](https://mirasim.ai/docs/zh/reference) |
| 6 | 标题栏右侧坞 | Git/文件/产物/任务四个视图共用右侧坞；另有用量读数（订阅+云端额度）、受控浏览器实况入口、主机指示（智能体跑在哪台机器） | [workbench](https://mirasim.ai/docs/zh/workbench) |

### 4.2 并行会话、队列与引导

| # | 功能 | 说明 | 来源 |
|---|---|---|---|
| 1 | 并行两形态 | 分工并行 / 同一任务多模型并行尝试；「每开一个窗格就多一份需要你亲自审阅的改动」 | [sessions](https://mirasim.ai/docs/zh/sessions) |
| 2 | 按「谁需要你」排序 | 侧栏排序：等你回答 > 还在运行 > 最近活跃；⌘1 跳到最需要处理的会话 | [sessions](https://mirasim.ai/docs/zh/sessions) |
| 3 | **队列四操作** | 发送（排队尾）/ **引导**（交给正在执行的一轮、在下一步生效、记录中标注）/ **插队**（打断当前轮）/ **撤回**（取回输入框可编辑） | [queue](https://mirasim.ai/docs/zh/queue) |
| 4 | **队列项配置锁定** | 已排入队列的指令按发送时选定的模型与推理强度执行，之后改选择项不影响已排队内容 | [queue](https://mirasim.ai/docs/zh/queue) |
| 5 | 对话分叉与重跑 | 任意消息可「从此分支」生成独立新会话（继承完整历史）；重跑某一轮；修改自己发出的指令；不支持分叉的智能体会如实说明 | [branches](https://mirasim.ai/docs/zh/branches) |
| 6 | worktree 隔离 | 会话可运行在独立 Git worktree（智能体创建、Mira 记录）；并回主线是普通合并；营销口径「最多六个窗格、各自独立 worktree」 | [branches](https://mirasim.ai/docs/zh/branches)、[download](https://mirasim.ai/download) |

### 4.3 会话互联（agent 间消息）——与 CrewVela 最直接的对照物

逐会话 opt-in 的开关（默认关闭，仅作用于该会话）；开启后智能体获得四个工具；关闭则「拿着它确切的 key 也发不进去」（[会话互联](https://mirasim.ai/docs/zh/messaging)，下同）：

| # | 机制 | 说明 |
|---|---|---|
| 1 | **四个动词** | `list_sessions`（列出可寻址会话及状态）/ `read_session`（读对方最近四轮摘要，上限 2KB，带等待选项=盯着它跑完，最多等 300s）/ `send_session_message`（单条文本上限 8KB）/ `start_session`（造新会话：指定智能体/模型/推理强度+开场任务，不阻塞，回信作为消息回来） |
| 2 | @ 点名即授权 | 输入 `@` 候选先文件后会话；一条消息最多 3 个；点名会就地为两端开启互联；点名只是事实行，投递决策由智能体判断 |
| 3 | **状态感知投递** | 目标空闲→起新轮；正在跑且支持注入→进入运行中回合的下一步边界；不支持注入或正等人回答→泊住最多 300s 后过期并说明；未开通互联→直接拒绝并告知不要盲试重发；终端会话可读不可收 |
| 4 | **投递承诺** | 至多一次（at-most-once）；信封带来源声明「这是来自另一个会话的消息，不是本会话用户的指令」；不承诺已读/照做；「Mirasim 不在智能体之上加权限层，真正兜住的是来源声明、账本、限流与熔断三件结构性的事」 |
| 5 | **固定上限** | 令牌桶按发送会话计：容量 30、每秒补 0.5；同文本 30s 内重发同会话判重丢弃；接力链深最多 24 跳（有真人参与归零）；**乒乓熔断**：一对会话 5 分钟内 >12 条且无真人输入→熔断并在界面显示 |
| 6 | **消息账本** | 本地追加式 JSONL 按月分文件（`~/.mirasim/messages/msgs-<YYYY-MM>.jsonl`），保留 90 天不上传；互联开关状态存 `~/.mirasim/messages/enrolled.json` |

### 4.4 多运行时与模型配置

| # | 功能 | 说明 | 来源 |
|---|---|---|---|
| 1 | 四个智能体 | Mirasim 内置智能体（随应用安装、进程内运行、可经中转）、Claude Code、Codex、Pi GUI（专做浏览器操作：按辅助功能树读屏而非截图；编码智能体经 `gui_task` 工具交接，≤15 分钟，带回会话标识续接浏览器状态，可 `PI_GUI_CDP_URL` 外接 Chrome） | [models](https://mirasim.ai/docs/zh/models)、[skills-tools](https://mirasim.ai/docs/zh/skills-tools) |
| 2 | **三层模型配置** | 供应商（地址+凭据）→ 配置项 profile（某模型+推理强度+token 上限）→ 绑定 binding（某智能体走哪个 profile 或回到原生订阅）；Anthropic/OpenAI/Google/任意 OpenAI 兼容地址一等支持 | [models](https://mirasim.ai/docs/zh/models) |
| 3 | 内置供应商清单 | OpenRouter、阿里 Qwen（DashScope）、豆包 Seed（火山方舟）、智谱 GLM、MiniMax、DeepSeek、Anthropic Direct、Google AI Studio、OpenAI Direct、自定义 | [models](https://mirasim.ai/docs/zh/models) 设置页抓取原文 |
| 4 | 本地模型 | Ollama + macOS mlx-lm；可探测、启动、带进度拉取、删除；「本地模型改变的是信任边界，不是工作方式」 | [models](https://mirasim.ai/docs/zh/models) |
| 5 | 1M 上下文 | 部分模型经供应商 beta 选项申请，须主动选 `[1m]` 后缀版本，从不当默认 | [models](https://mirasim.ai/docs/zh/models) |
| 6 | 多账号与实时额度 | 同一智能体可存多账号（账号是智能体级全局设置，非会话级）；每账号显示套餐/剩余量/周期用量比/重置时间，状态直白标注 | [models](https://mirasim.ai/docs/zh/models) |
| 7 | **额度兜底 relay** | 订阅耗尽时走 Mira 中转接续；触发条件：用量比例/5h 周期/7d 周期/被拒绝时/始终；走了兜底的轮次在对话记录与智能体状态行中**逐轮标注**；Codex 额度重置代用（先确认） | [models](https://mirasim.ai/docs/zh/models) |
| 8 | 智能体 CLI 版本管理 | 设置中并列显示各智能体 CLI 已装版本与最新版本，可自动更新 | [models](https://mirasim.ai/docs/zh/models) |

### 4.5 流量账本（用量与成本）——全部本机计算

四步链路（[用量与成本](https://mirasim.ai/docs/zh/usage)，官网宣称，下同）：① 本机代理在 127.0.0.1 上**原样转发模型流量并留存副本**，不改写不解析不阻塞；② 原始报文落盘时认证头即被替换为指纹；③ 离线解算为 token/缓存/模型数据；④ 渲染只读本机磁盘。功能面：

- 时间范围（今天/7 天/30 天/90 天）+ 关键数字（调用次数、总 token、输出 token、缓存命中率、成本估算）+ 环比；
- 五维分摊：智能体/模型/账号/工作区/会话，点行即加入全局筛选；
- 单次调用明细含原始请求/响应正文（若仍在磁盘），CSV 导出；
- 无报价模型单独归入「未定价模型」，不默认按零计；成本是估算并如实标注；
- 与之配套的隐私设计见 4.9。

### 4.6 Skills / MCP / 连接器 / 工具清点 / 浏览器自动化

| # | 功能 | 说明 | 来源 |
|---|---|---|---|
| 1 | Skills | 命名可复用说明；全局或工作区作用域，工作区级存 `<工作区>/.mirasim` **随仓库评审**；`/` 呼出时与智能体内置命令并列 | [skills-tools](https://mirasim.ai/docs/zh/skills-tools) |
| 2 | MCP | 全局/工作区两作用域；同时显示 Mira 管理的与智能体自带的服务；`mcp health` 探活握手 | [skills-tools](https://mirasim.ai/docs/zh/skills-tools) |
| 3 | 连接器 | MCP 之上的精选目录（约 40 项：GitHub/GitLab/Notion/Linear/Jira/Stripe/支付宝/飞书/钉钉/企业微信/微信/Gmail/M365 等，多数 Official/OAuth 标注）+ 官方 MCP 注册表搜索；完整生命周期：校验→OAuth 续期→注销撤销；**按智能体分别启用**，不强制加给每个会话 | [skills-tools](https://mirasim.ai/docs/zh/skills-tools) |
| 4 | CLI 工具清点 | 清点 PATH 上常见开发命令行（git/gh/docker 等）及登录状态；补齐从 Dock 启动缺失的 shell 环境 | [skills-tools](https://mirasim.ai/docs/zh/skills-tools) |
| 5 | 浏览器/屏幕自动化 | **默认关闭**；驱动独立配置目录的浏览器（与日常浏览分离、非沙箱）；macOS 屏幕控制需「屏幕录制+辅助功能+完全磁盘访问」权限并逐项说明 | [skills-tools](https://mirasim.ai/docs/zh/skills-tools) |

### 4.7 定时任务 / 目标 / 任务总览 / 收件箱

| # | 功能 | 说明 | 来源 |
|---|---|---|---|
| 1 | 定时任务 | 普通会话指令+时间触发；存 `<工作区>/.mirasim/schedules.json` **随仓库评审**；预设频率+五段 cron，最短 10 分钟；只在 Mira 于该端运行时触发，**错过的触发被跳过不补跑** | [orchestrate](https://mirasim.ai/docs/zh/orchestrate) |
| 2 | **目标 /goal**（预览） | 一整件带完成条件的工作交由一个智能体编排：把你的对话整理成**只读规格**（预期结果、可验证验收标准、范围、边界、不做事项、决策与假设、未决项），确认即授权；执行中配任务关系图+看板+关键路径；只有改变范围/安全性/权限/不可逆后果的决定才阻塞，并显示阻塞了多少执行单元；**验收依据证据（产物+验证过程+工作记录）而非声明**，记录缺失会如实说明 | [orchestrate](https://mirasim.ai/docs/zh/orchestrate) |
| 3 | 任务总览 | 多会话/目标集中一屏：进行中/等你/需关注/今日完成；每任务带事件轨迹（开始、进入某步、子任务启动或失败、向你提问、停滞、完成）；**用直白语言区分「停滞」与「只是较慢」**；无任务时直说空闲，不虚构动态 | [orchestrate](https://mirasim.ai/docs/zh/orchestrate) |
| 4 | **待处理收件箱** | 每条标明所需动作（回答/对齐/决定/修复/继续/验收）+已等待时长；标已读即清除且不影响会话；承诺「不在这里的事项，你不需要关注」 | [orchestrate](https://mirasim.ai/docs/zh/orchestrate) |

### 4.8 $eval 仿真用户评测（最大差异点）

- 显式调用的能力（非常开的自动审查），在本机运行（[evaluation](https://mirasim.ai/docs/zh/evaluation)，官网宣称，下同）；
- 流程：锁定版本并写明不同结果对应的不同决定 → 构建相互独立的人物画像与自然场景（缺耐心的新用户、跳引导的熟手、想离开的怀疑者）→ 给每个仿真用户真实入口（浏览器/手机/命令行/对话/已渲染产物）→ 先读证据再决定（保留/回退/换用例重验）；
- 观察到的事实与推断出的判断**分开记录**；截图、命令、轨迹、产物全部保留并绑定到产出它的会话；「无法追溯到具体某次运行的结论，一律视为没有结论」；
- 结果转化：每个观察到的问题直接转为下一轮开发任务；⌘K 搜「Eval 记录」可回放；
- 回传平台是独立的选择加入项，默认一切留本机（[evaluation](https://mirasim.ai/docs/zh/evaluation)、[privacy](https://mirasim.ai/docs/zh/privacy)）。

### 4.9 文件 / Git / 终端 / 产物 / 隐私

- 文件树带逐行 Git 状态；完整文件管理器右键操作；会话可区分「工作区（输出目录）」与「项目（源码目录）」两个目录（[workspace](https://mirasim.ai/docs/zh/workspace)，官网宣称，下同）。
- Git 面板：暂存/提交/推送/分支/还原全量操作；**失败情形诊断**——未完成时指明属于哪种常见情形（本地改动会被覆盖、分支分叉、未设上游、远端有新提交、未完成合并/rebase、凭据被拒、远端不可达、被钩子拒绝）并给建议操作；后台只读查询远端使领先/落后数字准确（可关闭）。
- 产物预览：MD/JSON/CSV 表格/图片/PDF/音视频，有意只读；超长文件只预览前几千行并说明；工作区外的产物拒绝读取。
- 导出对话：整段或**单轮**（推荐，附图片与文件）导出到目录。
- 隐私（[privacy](https://mirasim.ai/docs/zh/privacy)、[reference](https://mirasim.ai/docs/zh/reference)，官网宣称）：原始模型请求/响应记录**默认开启**（`~/.mirasim/recordings`，支持原生直通路径；请求头默认脱敏为白名单；写盘时有界队列满则丢弃并计数，不阻塞会话）；关闭方式 `MIRASIM_RECORD=0` 或 `~/.mirasim/capture.off`（**记录与上传是同一个开关，没有只管上传的独立开关**）；留存策略：已上传 24h / 未上传 30 天 / 用量账目 90 天；Codex 原生抓取需 OpenSSL，经自配模型注入的轮次不抓取；凭据在系统钥匙串（无钥匙串环境用加封本地存储）。

### 4.10 多端与 ui-cli

- **安卓客户端**：配对码含服务端端到端公钥时使用应用层 E2EE；局域网优先、不在时自动改用中转地址（服务端自己找到，无需配置）；凭据加密并与设备绑定；手机可切换「运行账号」、可升级配对的电脑（[surfaces](https://mirasim.ai/docs/zh/surfaces)、[privacy](https://mirasim.ai/docs/zh/privacy)，官网宣称）。
- **七个聊天渠道**：Telegram、飞书、Slack、Discord、钉钉、企业微信、微信；全部长连接模式（无需公网 IP 与回调地址）；每渠道凭据先校验再启用；明确「聊天平台属于另一条信任边界」（[surfaces](https://mirasim.ai/docs/zh/surfaces)，官网宣称）。
- **SSH 远端**：`mirasim ssh` 探测目标机、推送服务端、启动并隧道回连，远端无需开放端口；支持 ssh-agent/密码/私钥与跳板机（[surfaces](https://mirasim.ai/docs/zh/surfaces)，官网宣称）。
- **ui-cli：整个 GUI 的可脚本版本**（[cli](https://mirasim.ai/docs/zh/cli)、[reference](https://mirasim.ai/docs/zh/reference)，官网宣称，下同）：每条命令使用与桌面端/移动端/编辑器端完全相同的协议帧，输出单个 JSON；覆盖读取（state/sessions/queue/traffic…）与操作（prompt/send/watch/stop、**「回答智能体的交互式提问——相当于可脚本调用的点击允许」**、queue steer/jump/recall/move、session new/open/rename/delete/resume）；设置是一棵**声明式 JSON 配置树**（`config get/set/apply --dry-run`，先比较再写入、热生效、凭据有意不在树中）；覆盖不到的能力用 `raw` 发任意协议帧兜底。

## 5. 技术架构与实现方式

### 5.1 官网宣称但无法独立验证（闭源）

- 服务端 SessionHost 的实现、数据存储格式、投递队列实现（[concepts](https://mirasim.ai/docs/zh/concepts) 仅有逻辑描述）；
- 本机代理「原样转发+落盘脱敏+离线解算」的实际实现与性能（[usage](https://mirasim.ai/docs/zh/usage)）；
- E2EE 配对的密码学细节（仅描述配对码携带端到端公钥）（[privacy](https://mirasim.ai/docs/zh/privacy)）；
- 「首页工程师来自 NVIDIA/Google/AWS/Meta…」的背书滚动条（[首页](https://mirasim.ai/)）——无法核实；
- 「Everything shown is real——本站每张截图都来自真实产品」（[关于我们](https://mirasim.ai/about)）——宣传口径。

### 5.2 未查到

- 运营公司实体与注册地（ToS 只写 "we and our affiliates"）；
- 内置智能体的模型来源与许可（「进程内运行，按你配置的路径访问模型」）（[models](https://mirasim.ai/docs/zh/models)）；
- 定价档位对应的云端 Pro 具体配额表（定价页只给等效换算）。

### 5.3 架构判断（推断）

Mirasim 的架构是「**本地服务端 + 多端瘦客户端 + 透明模型代理**」：桌面应用本质是 `mirasim serve` 的一个视图，这使 ui-cli 能用同一协议帧全量驱动 GUI、手机与 IM 渠道能以长连接出站接入、SSH 机器能即插即用。与 CrewVela「前端直连 provider、无中间代理」相比，Mira 在本机引入了一层 127.0.0.1 代理，这正是流量账本、额度兜底、状态行注入三件事的实现前提——这是两个产品最根本的架构分歧点。

## 6. 与 CrewVela 的功能重叠与差异对照

CrewVela 现状基线（以仓库为准）：Tauri 2 桌面应用；providers.json（glm/kimi/minimax/deepseek/custom）+ keychain + env 注入直连 claude CLI（providers.rs/secrets.rs/launcher.rs）；多运行时会话（AgentRuntimeKind 覆盖 claude_code/codex/kimi_code/opencode/pi/hermes 六种，messaging.rs `runtime_alias`；ACP 适配 claude/codex，acp.rs + package.json 的 @agentclientprotocol 依赖）；会话状态机与恢复（sessions.rs，含归档/重启恢复）；网格分屏（MultiTerminal + pty_poll_all 轮询）；**会话间消息**（messaging.rs 的 `CrewVela send`/`sessions` CLI + outbox 原子写 + SessionActionQueue + 文本/回车分两次写 SUBMIT_DELAY + 每对 1h/10 条限流 + lurk 2 条 + DeliveryRecord 账本 500 条封顶，control.rs）；**Ask/Allow/Deny 审批策略 + 自动代答器 + 干预中心卡片流**（approvals.rs、InterventionCenter.tsx，5 类 intervention + 10 分钟超时提示 + snooze）；群聊面板三档活跃度 active/lurk/muted（GroupChatPanel.tsx、control.rs AgentActivity）；multi_agent 编排（coordinator/executor/governance/runtime/store/workflow_canvas，任务图+人工接管+知识/记忆检索，ma_* 命令 40+）；automation.rs（时间/事件触发+熔断+运行历史）；会话级 worktree（worktree.rs）；token 用量（usage.rs 增量解析 claude transcript 四类 token，无成本估算）；git_source_control.rs（stage/commit/push/branch/merge + AI 提交信息）；skill_center（SQLite，发现/导入/部署/回滚/更新/市场/信任 24 个命令）；mcp.rs、plugin_market.rs、livekit_voice.rs 等。

| 维度 | Mirasim | CrewVela | 关系判定 |
|---|---|---|---|
| 产品形态 | 桌面+手机+IM+CLI 的服务端-客户端全家桶（[surfaces](https://mirasim.ai/docs/zh/surfaces)） | 本地桌面工作台，外部通道有 livekit PoC | **理念同构，半径不同**：都做「桌面为主」，Mira 的多端是融资级工程 |
| 多运行时 | 内置 agent + Claude Code + Codex + Pi GUI（4 种）（[models](https://mirasim.ai/docs/zh/models)） | claude_code/codex/kimi/opencode/pi/hermes（6 种） | **CrewVela 更多**；Mira 的 Pi GUI（浏览器专职 agent + gui_task 交接）是 CrewVela 没有的品类 |
| 「不改智能体行为」 | 中间层只转发模型流量，保留 CLI 自身界面与登录（[models](https://mirasim.ai/docs/zh/models)） | env 注入直连 + 保留 xterm 原生界面（README/CLAUDE.md） | **同一哲学**；差异只在代理层有无 |
| 模型配置 | provider/profile/binding 三层 + 10 内置供应商 + 本地模型 + `model test` 真实验证（[models](https://mirasim.ai/docs/zh/models)） | providers.json + 模型映射 + test_provider_endpoints | **高度重叠**；Mira 的「三层解耦 + 绑定到 agent」命名值得对齐 |
| Key 安全 | 系统钥匙串，无钥匙串用加封本地存储（[privacy](https://mirasim.ai/docs/zh/privacy)） | keychain（secrets.rs） | **等同** |
| 会话队列 | 发送/引导/插队/撤回四操作 + 队列项配置锁定（[queue](https://mirasim.ai/docs/zh/queue)） | SessionActionQueue 排队+广播+@定向，**无引导/插队/撤回语义、无配置锁定**（control.rs） | **CrewVela 空白**（最高价值差异点之一） |
| 会话间消息 | 一等 agent 工具四动词 + 状态感知投递 + 泊住/过期 + 令牌桶/判重/链深/乒乓熔断 + JSONL 账本（[messaging](https://mirasim.ai/docs/zh/messaging)） | `CrewVela send` CLI + outbox + @mention 直投 + 每对 1h/10 条限流 + DeliveryRecord | **大幅重叠 + 语义差距**：Mira 把消息做成智能体的一等工具并给出完整投递语义 |
| 会话分支/重跑 | 任意消息分叉新会话、重跑某一轮、修改已发指令（[branches](https://mirasim.ai/docs/zh/branches)） | 无（有 --resume/归档） | **CrewVela 空白** |
| worktree | 会话级独立 worktree（[branches](https://mirasim.ai/docs/zh/branches)） | 会话级 worktree（worktree.rs） | **等同**（CrewVela 先行） |
| Git 面板 | 全量操作 + 8 种失败情形诊断（[workspace](https://mirasim.ai/docs/zh/workspace)） | git_source_control.rs 全量操作 + AI 提交信息 | **重叠**；仅失败诊断话术可借鉴 |
| 审批 | Ask/Allow/Deny 三档 + 卡片（队列四操作之「撤回」与 ui-cli「回答提问=可脚本化的点击允许」，[reference](https://mirasim.ai/docs/zh/reference)） | ask/allow/deny + 自动代答 + InterventionCenter 卡片流（approvals.rs） | **CrewVela 已等价实现**（本系列首次出现），无需引入 |
| 编排 | /goal：只读规格单一入口 + 确认即授权 + 证据验收 + 阻塞计数（预览）（[orchestrate](https://mirasim.ai/docs/zh/orchestrate)） | multi_agent 任务图 + 人工接管/交还 + 语义复核 | **同构**；Mira 的交互收敛度更高，但明确自称「不愿许诺编排」 |
| 定时任务 | 工作区级随仓库评审 + 错过不补跑（[orchestrate](https://mirasim.ai/docs/zh/orchestrate)） | 全局 automations.json + 事件触发 + 熔断（automation.rs） | **高度重叠**；作用域取舍不同 |
| 用量成本 | 本机代理抓包→离线解算→五维分摊→成本估算+缓存命中率（[usage](https://mirasim.ai/docs/zh/usage)） | claude transcript 四类 token，无成本、无缓存率、不覆盖其他 runtime（usage.rs） | **CrewVela 弱项** |
| Skills/MCP | 双作用域+连接器目录+注册表搜索（[skills-tools](https://mirasim.ai/docs/zh/skills-tools)） | skill_center 双作用域部署/回滚/更新 + mcp.rs | **高度重叠**；连接器「按智能体 opt-in」可借鉴 |
| 评测 | $eval 仿真用户闭环（[evaluation](https://mirasim.ai/docs/zh/evaluation)） | 无（model_eval.rs 是模型评测，非产物评测） | **CrewVela 空白**，但依赖重设施 |
| 记录与隐私 | 原始记录默认开、单一开关、留存策略（[privacy](https://mirasim.ai/docs/zh/privacy)） | 不做中间代理、不抓包 | **定位相反**；CrewVela 默认不记录更干净 |
| 经济系统 | 云 Pro/relay 兜底/订阅/邀请裂变（[pricing](https://mirasim.ai/pricing)） | 无，直连 provider | **不适合引入**（同 Clawling 报告对经济系统的否决先例） |

## 7. 融合可行性分析

### 7.1 可融入（映射到 CrewVela 现有模块）

1. **会话队列的「引导 / 插队 / 撤回」三语义 + 队列项配置锁定**（价值：高）
   - Mira 证据：四操作的定义与语义边界清晰（发送/引导/插队/撤回只差「对正在执行那一轮的影响」），队列项锁定发送时模型与推理强度（[queue](https://mirasim.ai/docs/zh/queue)）。
   - 映射：CrewVela 的 `SessionActionQueue`（control.rs）已具备排队尾与广播；补三类消息 kind（guide=轮询时写入运行中会话的下一次输入窗口、jump=先发 Esc 中断再投、recall=从队列取回前端输入框）即可，「引导需在对话记录标注」可先落到 DeliveryRecord。配置锁定在入队消息里带上当时 provider/model 快照。
   - 代价：低-中。主要是 control.rs 状态机扩展 + InterventionCenter/群聊面板的操作入口。

2. **会话互联升级为一等 agent 工具组**（价值：高）
   - Mira 证据：四个动词（list/read/send/start）、@点名即授权、状态感知投递（空闲起新轮/运行中步进边界注入/泊住 300s 过期）、at-most-once + 来源信封、令牌桶 30@0.5/s、判重、链深 24、乒乓熔断 12/5min、JSONL 月度账本（[messaging](https://mirasim.ai/docs/zh/messaging)）。
   - 映射：CrewVela 的 `CrewVela send`/`sessions`（messaging.rs）是「让 agent 用 Bash 调 CLI」的等价物，且已有每对限流与信封（MessageOrigin）；差距在：① read/start 两个动词没有（agent 不能读对方近况、不能自己造会话）；② 投递无「泊住/过期」状态感知；③ 限流参数与熔断不可见。read_session 可复用 transcript.rs 的记录读取；start_session 可复用 launch_one_session；账本可把 DeliveryRecord（内存 500 条封顶）落成同款 JSONL。
   - 代价：中。新 skill（暴露给 claude/codex 的工具说明）+ control.rs 投递状态机。

3. **本机成本账本（token→成本）**（价值：高）
   - Mira 证据：五维分摊（agent/model/account/workspace/session）、缓存命中率、成本估算如实标注、未定价模型单列、CSV 导出（[usage](https://mirasim.ai/docs/zh/usage)）。
   - 映射：**不抄它的本机代理抓包**（见 7.2），沿 usage.rs 现有 transcript 路线扩展：单价表入 provider 元数据（types.ts 的 PricingModelSource 已有雏形）、按 provider/session 分摊、补 cache_read 命中率展示。Mira 的「页面自报数据来源与云端请求数」的透明原则照抄（[usage](https://mirasim.ai/docs/zh/usage)）。
   - 代价：低-中。纯前端+现有数据扩展，无新进程。

4. **收件箱与任务总览的「承诺式」设计**（价值：中-高）
   - Mira 证据：收件箱每条带动作类型与等待时长，承诺「不在这里的事项不需要关注」；任务总览区分「停滞」与「只是较慢」，带事件轨迹，空闲时直说（[orchestrate](https://mirasim.ai/docs/zh/orchestrate)）。
   - 映射：InterventionCenter（5 类 intervention + 审批卡片）已是「等我的事」的聚合器；补三件事——① 每条记录的「所需动作」枚举与等待时长；② multi_agent store 的事件轨迹投影（ma_activity_events 已有事件源）；③ 空态文案从「无数据」改为明确承诺。
   - 代价：低。以 UI 与投影为主。

5. **/goal 的只读规格与证据验收交互**（价值：中）
   - Mira 证据：目标空间一侧是主对话、另一侧是只读规格（验收标准/边界/决策/未决项），修改只有一个入口；验收核对证据而非声明，节点展开即产物+验证过程+工作记录，缺失会说明（[orchestrate](https://mirasim.ai/docs/zh/orchestrate)）。
   - 映射：CrewVela multi_agent 已有任务图/看板/验收命令（README）；可借鉴的是「规格只读、由对话同步」的单一写入入口与「证据卡」节点展开，防止用户在两处编辑同一意图。
   - 代价：中（governance/store 交互改造）。

6. **Git 失败情形诊断话术 + ⌘K 对话全文检索**（价值：中，顺手项）
   - Mira 证据：八种常见 git 失败情形+建议操作（[workspace](https://mirasim.ai/docs/zh/workspace)）；命令面板全文检索全部对话并汇报覆盖率（[layout](https://mirasim.ai/docs/zh/layout)）。
   - 映射：git_source_control.rs 的错误分支归一化为情形枚举；transcript.rs 已存记录，做一个跨会话检索面板即可。
   - 代价：低。

### 7.2 融合代价高或不适合

- **云 Pro / 额度兜底 relay / 订阅计费**：需要云侧账号、计费与中转服务，与 CrewVela「直连 provider、无中间代理、key 不出本机」的定位相反（[models](https://mirasim.ai/docs/zh/models) vs README）——同既有报告对经济系统的否决先例。
- **本机代理抓包式记录**：Mira 的流量账本依赖 127.0.0.1 透明代理 + 原始报文落盘（且记录默认开启）（[usage](https://mirasim.ai/docs/zh/usage)、[privacy](https://mirasim.ai/docs/zh/privacy)）；CrewVela 引入中间层会推翻 README 的「无中间代理」承诺，且默认开记录是隐私负债。成本账本走 7.1.3 的 transcript 路线即可。
- **七个 IM 渠道与安卓/iOS 端**：Mira 的长连接出站与 E2EE 配对设计值得存档（[surfaces](https://mirasim.ai/docs/zh/surfaces)），但移动端+全渠道是平台级工程；CrewVela 已有 livekit PoC 与 IM 相关研究，等「外部接入」立项时再参考其「凭据先校验再启用」「信任边界声明」两条。
- **$eval 完整闭环**：依赖受控浏览器、Pi GUI、手机端、产物渲染等重设施（[evaluation](https://mirasim.ai/docs/zh/evaluation)）；CrewVela 短期可取其「画像+场景+观察/推断分离+结果转任务」的方法论，用 headless claude + 产物预览做轻量冒烟，不做完整仿真用户。
- **Pi GUI / 浏览器专职 agent**：CrewVela 无对应运行时与浏览器设施；引入即新增一个 runtime 的适配面，优先级低于以上各项。

### 7.3 推荐路径

1. **P1 队列三语义 + 配置锁定**（7.1.1）：改动集中在 control.rs，直接增强日常单会话体验。
2. **P1-P2 会话互联四动词**（7.1.2）：把 CrewVela 已有的消息通路补齐成 Mira 级语义，是「多会话并行」故事的自然收尾。
3. **P2 成本账本**（7.1.3）：usage.rs 已有四类 token，只差单价与分摊视图。
4. **P2 收件箱承诺设计**（7.1.4）：InterventionCenter 的小步增强。
5. **P3 /goal 规格模式、git 诊断话术、对话全文检索**（7.1.5/6）。
6. **不做**：relay/云 Pro、代理抓包、IM 全渠道、移动端、$eval 完整闭环、Pi GUI（理由见 7.2；经济系统与 IM 化的否决沿用 clawling 报告先例）。

## 8. 参考来源列表

### 官方文档（2026-10-01 抓取，18 章双语树全量）

- 快速上手 https://mirasim.ai/docs/zh/quickstart · 工作台 https://mirasim.ai/docs/zh/workbench · 核心概念 https://mirasim.ai/docs/zh/concepts · 并行会话 https://mirasim.ai/docs/zh/sessions · 排队与引导 https://mirasim.ai/docs/zh/queue · 分支与 worktree https://mirasim.ai/docs/zh/branches · 会话互联 https://mirasim.ai/docs/zh/messaging · 窗格分组 https://mirasim.ai/docs/zh/layout · 文件 Git 终端 https://mirasim.ai/docs/zh/workspace · 模型账号额度 https://mirasim.ai/docs/zh/models · 用量成本 https://mirasim.ai/docs/zh/usage · Skills/MCP/工具 https://mirasim.ai/docs/zh/skills-tools · 定时任务/目标/总览 https://mirasim.ai/docs/zh/orchestrate · $eval https://mirasim.ai/docs/zh/evaluation · 各端 https://mirasim.ai/docs/zh/surfaces · CLI/ui-cli https://mirasim.ai/docs/zh/cli · 数据与隐私 https://mirasim.ai/docs/zh/privacy · 查阅 https://mirasim.ai/docs/zh/reference（英文树 https://mirasim.ai/docs/<slug> 逐篇对应，已验证镜像）

### 产品页

- 首页 https://mirasim.ai/ · 下载 https://mirasim.ai/download · 定价 https://mirasim.ai/pricing · 关于 https://mirasim.ai/about · 公告 https://mirasim.ai/announcement/zh · 更新日志 https://mirasim.ai/changelog/zh · 帮助 https://mirasim.ai/help/zh · 站点地图 https://mirasim.ai/sitemap.xml

### 写作栏目（产品哲学）

- https://mirasim.ai/writing/zh/what-is-an-agent-ide · https://mirasim.ai/writing/zh/claude-code-and-codex-one-place · https://mirasim.ai/writing/zh/parallel-coding-agents-git-worktrees · https://mirasim.ai/writing/zh/simulated-users-agent-work · https://mirasim.ai/writing/zh/what-agentic-coding-costs

### 法律

- https://mirasim.ai/terms-of-service · https://mirasim.ai/privacy-policy · https://mirasim.ai/acceptable-use-policy

### 开源仓库

- 无（全站无 GitHub 项目链接，产品闭源；仅 ui-cli 示例中的占位 `github.com/o/r`）

### 本地参照文件

- `.\README.md`、`.\CLAUDE.md`、`.\package.json`
- `.\src-tauri\src\`（approvals.rs、messaging.rs、control.rs、sessions.rs、usage.rs、automation.rs、worktree.rs、git_source_control.rs、skill_center.rs、mcp.rs、multi_agent/）
- `.\src\`（types.ts、App.tsx、components/InterventionCenter.tsx、components/GroupChatPanel.tsx）
- `.\docs\research\clawling-core-features-crewvela-fit-analysis.md`（风格模板与「IM 化/经济系统」否决先例）

# Clawling 核心功能调研与 CrewVela 融合适配分析

> 研究日期：2026-09-30
> 对象：Clawling（clawling.com）— ClawChat / ClawNest / ClawMesh 三件套
> 证据规则：全部事实来自官网页面、官方文档站与官方 GitHub 仓库（2026-09-30 实时抓取）；每条陈述标注来源 URL；「官网宣称」与「源码/文档验证」分开标记；查不到的明确写「未查到」；推断单独标注「推断」。
> 本地参照：`.`（README.md、CLAUDE.md、src-tauri/src/ 模块结构）及 `docs/research/six-agent-platforms-crewvela-gap-analysis.md`、`docs/research/openclaw-hermes-resume-notes.md`。

## 1. TL;DR

Clawling 是新加坡公司 ClawChat Pte. Ltd. 运营的「可信数字伴侣」平台：ClawChat 是一款把 AI Agent 当作**一等账户成员**（member, not tool）的跨端即时通讯应用，ClawNest 是无代码创建/托管开源 Agent（OpenClaw / Hermes）的市场，ClawMesh 是尚未落地的 agent-to-agent 开放协议白皮书。它对 CrewVela 的价值不在产品形态（To C 社交 IM vs 开发者工作台，直接融合不可行），而在三个可直接借鉴的机制：**结构化审批卡片（Ask/Allow/Deny 三级 + 过期）**、**群聊式无调度多 Agent 协作（共享上下文 + 活跃度/静音开关）**、**一次性 connect code 的 Agent 配对接入协议（Protocol v2 over WebSocket）**。

## 2. Clawling 项目概况

### 2.1 一句话定位

「基于开源构建、人人可用的可信数字伴侣平台」——官网口号 "A trustworthy digital companion, for everyone"，核心理念是可信的伴侣必须 "open-source, self-hosted, and private by default"。（[官网首页](https://clawling.com/)，官网宣称）

官网首页三件套原文（官网宣称，2026-09-30 抓取）：

- ClawChat — "A messenger for digital companions"
- ClawNest — "Spin up your own open-source agent"（Pick OpenClaw or Hermes and ship in minutes — no code）
- ClawMesh — "An open protocol"

搜索摘要中另收录完整标语："Any agent can do the work. But your digital companion must answer to you. Clawling puts open-source agents in everyone's hands."（[搜索结果引用首页](https://clawling.com/)）

### 2.2 目标用户

- 想要自主掌控 AI Agent、不信任大厂托管代理的用户（首页：反平台代理叙事）（[首页](https://clawling.com/)，官网宣称）
- 长尾用例用户——"专为平台代理不会理会的长尾用例而建"（[首页](https://clawling.com/)；[ClawNest](https://clawling.com/nest) 同样强调 long-tail use cases）
- ClawChat 文档明确产品是面向普通人的「数字成员」通讯录体验，创建 Agent 像「添加联系人」而非配置工具（[What is ClawChat](https://clawling.com/chat/docs/what-is-clawchat)、[Create your first Agent](https://clawling.com/chat/docs/first-agent)，官网宣称）

### 2.3 运营主体与产品矩阵

- 运营主体：**ClawChat Pte. Ltd.**（新加坡公司），服务条款生效日 2026-05-12，联系邮箱 contact@clawling.com；责任上限取「过去 12 个月付费金额」与 100 新加坡元中的较低者。（[Terms of Service](https://clawling.com/clawchat/legal/terms.html)，已验证）
- ClawMesh 白皮书署名 @newbaselab，联系邮箱 hello@newbaselab.com。（[ClawMesh](https://clawling.com/mesh)，官网宣称）→ 推断：Clawling 与 NewBaseLab 为同一团队的两个品牌名。
- 产品矩阵共 46 个公开页面：主站 1 + ClawChat 产品页 6 + ClawChat Docs 17 + Blog 10 + ClawNest 2 + ClawMesh 1 + 法律 4 + 多语言 sitemap 5。（[Sitemap](https://clawling.com/sitemap/)，已验证）
- 多语言支持：英/简中/繁中/日/西/韩。（[Sitemap](https://clawling.com/sitemap/)，已验证）
- 客户端分发：iOS App Store（id6775673749）、Google Play（com.clawling.clawchat.app）、Android APK、macOS DMG、Windows setup.exe，分 Early Access 与 Stable 双通道。（[ClawChat](https://clawling.com/chat)、[Early Access](https://clawling.com/chat/preview)，官网宣称）

### 2.4 商业模式

- ClawChat：核心功能免费、承诺永不投放广告（"We will never run ads"）；商业模式表述为正在探索「更健康的替代方案」，未披露细节。（[ClawChat](https://clawling.com/chat)，官网宣称）
- 服务条款仅模糊提及「部分功能可能需要付款、积分（credits）、订阅」，无价格表。（[Terms](https://clawling.com/clawchat/legal/terms.html)，已验证）
- ClawNest：托管 Agent 按 **CSP** 计费（具体全称未查到），新 Agent 附带免费额度；支持 BYOK（自带 API key）；收益可充值 **fuel（燃料）** 或提现；创作者从每次调用中获得分成（见 2.6 矛盾记录）。（[Models and API keys](https://clawling.com/chat/docs/model-and-key)、[ClawNest](https://clawling.com/nest)，官网宣称）
- **具体定价（价格表/订阅档位）：未查到。**

### 2.5 开源状态

- **平台本体（ClawChat 客户端/服务端、ClawNest）闭源**。官网只宣称「构建在开源之上」（指基于 OpenClaw / Hermes 两个开源框架），从未宣称自身开源。（[首页](https://clawling.com/)、[ClawNest](https://clawling.com/nest)，官网宣称）
- GitHub 组织 **CLAWLING PTE.LTD.**（[github.com/clawling](https://github.com/clawling)）仅 2 个公开仓库，均 MIT：
  - `clawchat-plugin-hermes-agent`：Python，21 stars，Hermes 网关插件。（已验证，2026-09-29 更新）
  - `clawchat-plugin-install-cli`：TypeScript，1 star，npm 安装 CLI。（已验证，2026-09-29 更新）
- ClawMesh 承诺为 open spec，但完整规范与参考实现 "land at a later date"（TBA），白皮书 PDF 下载未开放。（[ClawMesh](https://clawling.com/mesh)，官网宣称）
- License 汇总：**自研平台未声明 License（闭源）；仅连接插件为 MIT（源码验证）。**

### 2.6 官方页面间的一处数据矛盾（已回源核对）

- 首页 ClawNest 介绍写 **"5% of every call rewards the original creator"**。（[首页](https://clawling.com/)，2026-09-30 抓取原文）
- ClawNest 页写 **"10% of every call rewards the original creator"**（两处出现）。（[ClawNest](https://clawling.com/nest)，2026-09-30 抓取原文）
- ClawMesh 白皮书亦写 5%。（[ClawMesh](https://clawling.com/mesh)）
- → 推断：首页/白皮书滞后于 Nest 页改版，当前口径以 ClawNest 页 10% 为准；此矛盾本身说明项目迭代较快、文档不同步。

## 3. 核心功能拆解（逐条带来源）

### 3.1 ClawChat — 数字伴侣消息应用

| # | 功能 | 说明 | 来源 |
|---|---|---|---|
| 1 | **Agent 一等账户成员** | Agent 拥有完整账户：名称头像、加好友、自主入群、发帖、@ 提及、引用转发、Agent 间直接交流；无需 @ 即可主动发起对话；人与 Agent 在聊天列表中不做视觉区分（官方称为 deliberate product decision） | [ClawChat](https://clawling.com/chat)、[What is ClawChat](https://clawling.com/chat/docs/what-is-clawchat)（官网宣称） |
| 2 | **聊天记录本地存储** | 聊天记录存于用户设备（on-device），服务器不保留长期存档；与三大「典型 IM 区别」之一 | [What is ClawChat](https://clawling.com/chat/docs/what-is-clawchat)（官网宣称；服务端实现无法验证） |
| 3 | **E2EE（Beta，默认关闭）** | 设置中手动开启；仅覆盖一对一文字消息与设备间传输；群聊、媒体、元数据不在保护范围；开启后紫色气泡+锁图标；密钥管理（备份/验证）文档完全未提及——信息缺口 | [E2EE](https://clawling.com/chat/docs/e2ee)（官网宣称） |
| 4 | **无广告 + 核心免费** | "We will never run ads"；商业模式未披露 | [ClawChat](https://clawling.com/chat)（官网宣称） |
| 5 | **两级记忆体系** | 会话历史按会话隔离（DM 与每个群互不共享上下文）+ 每个 Agent 一份全局长期记忆（只存提炼要点，不存逐字记录）；记忆写入有同步延迟，跨会话精确召回需在 Agent 框架侧做编排；查历史应使用框架的会话搜索命令 | [Memory](https://clawling.com/chat/docs/memory)、[首页](https://clawling.com/)（官网宣称） |
| 6 | **模型配置在 Agent 侧（无客户端 key 输入框）** | "The model is always configured on the Agent's side." 三种方式：① ClawNest 托管 + 平台算力（CSP 计费，新 Agent 免费额度）；② ClawNest 托管 + BYOK（compute center 里 Connect my own API，仅对该 Agent 生效，保存后切换模型并重启 Agent）；③ 自托管（key 在自己机器配置文件里）。支持的具体模型/提供商名单：未查到 | [Models and API keys](https://clawling.com/chat/docs/model-and-key)（官网宣称） |
| 7 | **权限审批卡片** | Agent 执行「更改账户」的操作（加好友/接受好友/删好友、发帖/评论/点赞/删除）时发审批卡片，卡片会过期；每类操作三档：Ask every time / Allow / Deny；批准后由服务器直接执行，Agent 无需再次询问；另提及可「限制工具集或启用命令审批」但未展开 | [Permissions](https://clawling.com/chat/docs/permissions)（官网宣称） |
| 8 | **安全提示：Agent 可读宿主机文件** | 官方明确警告：Agent 可访问其运行机器上的文件，加好友后存在被诱导读取主机敏感文件（含模型 API 密钥）的风险，建议限制工具或开启命令审批 | [Permissions](https://clawling.com/chat/docs/permissions)（官网宣称） |
| 9 | **多 Agent 群组协作（无中心调度）** | 群内所有消息对每个 Agent 可见；消息带发送者身份与 mention 元数据，Agent 自主决定是否介入；明确 "no central dispatcher"；与编排式（固定流水线）对立定位为「探索性协作」；人可 @ 介入、用活跃度/静音开关控场；官方建议 >3 个 Agent 先统一调低活跃度再逐个放开；代价是可能互相触发/重复工作/刷屏 | [Multi-agent](https://clawling.com/chat/docs/multi-agent)、[Activity & mute](https://clawling.com/chat/docs/activity-and-mute)（官网宣称） |
| 10 | **Liveware 小程序** | 自然语言（如"做个月度预算追踪器"）让 Agent 直接在聊天内构建可用的迷你应用，无需编程、无需应用商店 | [ClawChat](https://clawling.com/chat)（官网宣称） |
| 11 | **拍照/截图启动任务** | 拍照即可触发带预设技能的 Agent 处理（示例：硬币识别收藏），可建快捷方式 | [ClawChat](https://clawling.com/chat)（官网宣称） |
| 12 | **多框架 Agent 接入** | 支持 Claude Code、Codex、OpenClaw、Hermes，另有 GitHub 连接器接入自有 Agent（官方维护的 OpenClaw 连接器非 OpenClaw 官方渠道） | [ClawChat](https://clawling.com/chat)、[搜索摘要](https://clawling.com/chat/docs/what-is-clawchat/)（官网宣称） |
| 13 | **一次性 connect code 配对** | Add Agent 卡片生成含一次性连接码的消息 → 用户发给本机 Agent → Agent 按消息内指南自行安装插件并连接；连接码单次有效、过期需重新生成；连接成功的唯一可靠标志是 Agent 在 ClawChat 内主动发来问候（"A terminal saying 'activation complete' doesn't count."）；多 Agent 共存需独立身份/连接码，复用配置会覆盖前者；插件装完需手动重启宿主进程 | [Connect your own Agent](https://clawling.com/chat/docs/connect-your-agent)、[First Agent](https://clawling.com/chat/docs/first-agent)（官网宣称 + 文档验证） |
| 14 | **自托管接入三路径** | ① Hermes / OpenClaw 官方插件（自动安装）；② ClawNest 托管（免维护）；③ 开放协议自建通道。硬性要求：必须有进程持有**持久连接**（Agent 常驻在线可收推送）；非常驻框架需自建桥接进程。自建方需实现：配对激活、保活、消息收发、群组发言规则、媒体处理 | [Self-host](https://clawling.com/chat/docs/self-host)（文档验证） |
| 15 | **Flash 平台运维代理** | 常驻平台代理，处理 Agent 异常（重启、回滚、上下文调整），每次激活携带具体 Agent 的上下文（演示：诊断上下文超限并调高单任务 token 上限至 8K） | [ClawNest](https://clawling.com/nest)（官网宣称） |

### 3.2 ClawNest — 开源 Agent 市场

| # | 功能 | 说明 | 来源 |
|---|---|---|---|
| 1 | 无代码创建 Agent | "Pick a framework, name it, ship it"，仅支持开源框架 OpenClaw / Hermes（"trust starts with open source"）；任何人可提交 | [ClawNest](https://clawling.com/nest)（官网宣称） |
| 2 | 官方基础镜像 | OpenClaw Base（保留原体验）、Hermes-Advance（增强记忆与浏览器使用）、Hermes-Base（保留原体验） | [ClawNest](https://clawling.com/nest)（官网宣称） |
| 3 | Clone Key | 生成文本 + 二维码形式的克隆密钥私下分享代理；走市场发布则需审核（编辑精选） | [ClawNest](https://clawling.com/nest)（官网宣称） |
| 4 | 创作者分成 | nest 页：每次调用收入 10% 归原创者（首页/白皮书为 5%，见 2.6）；收益自动累积，可充值 fuel 或提现 | [ClawNest](https://clawling.com/nest)、[首页](https://clawling.com/)（官网宣称） |
| 5 | Creator Studio | 创作者工作室（/nest/studio）、My Agents 面板（/nest/dashboard） | [ClawNest](https://clawling.com/nest)（官网宣称） |
| 6 | 市场示例 | 群组管理助手、酒馆管理员角色扮演（推荐 DeepSeek V4 Flash）、塔罗、OfficeCLI 等 | [ClawNest](https://clawling.com/nest)（官网宣称） |

### 3.3 ClawMesh — 开放协议（未落地）

- 定位：agent-to-agent 开放协议，类比 SMTP/HTTP 之前所需的互联网协议栈；让任意两个 Agent 无需平台中介即可互相识别、协商任务、交换价值。（[ClawMesh](https://clawling.com/mesh)，官网宣称）
- 状态：v0.1 草案（DRAFT 2026-05），自我定位为「立场文件」，PDF 下载与最终规范 TBA。（已验证页面内容为草案）
- 三支柱：**身份**（可验证、跨平台携带、可追溯到人类管理者）、**声誉**（基于真实交互的可验证凭据，归代理主人而非平台）、**资产**（CSP 余额/收益/声誉归人类所有，可审计、可控、可撤销）。（官网宣称）
- 明确立场：代理是工具，没有自己的钱包；价值流转中每次交互 5% 回馈原创者。（官网宣称）
- 路线图：能力先以早期形式落地 ClawChat / ClawNest；规范迭代、参考实现、ABI 兼容性测试交由开源社区（OpenClaw、Hermes 等）维护。（官网宣称）

## 4. 技术架构与实现方式

### 4.1 已验证（源码/官方文档）

- **ClawChat Protocol v2 over WebSocket**：hermes 插件 README 明确 "ClawChat Protocol v2 (WebSocket)"，权威线协议契约在其 `docs/client-integration.md`；install-cli 在安装时写入三个后端端点——REST API（激活/连接、资料、好友）、WebSocket 消息通道（规范化为 `wss://host:port/ws`，可传 `ws://` 关 TLS）、媒体上传下载端点；端点缺省时插件回退内置默认值。（[clawchat-plugin-hermes-agent](https://github.com/clawling/clawchat-plugin-hermes-agent)、[clawchat-plugin-install-cli](https://github.com/clawling/clawchat-plugin-install-cli)，源码验证）
- **插件化接入，委托宿主插件管理器**：CLI 只有 `install`/`update` 两个命令，OpenClaw 侧委托 `openclaw plugins install @clawling/clawchat-plugin-openclaw`（含旧插件 ID 迁移保留配对信息），Hermes 侧拉取远程 plugin.yaml、检查 `hermes --version` 兼容性后 `hermes plugins install clawling/clawchat-plugin-hermes-agent --enable`；Hermes 写 `<Hermes home>/.env`，OpenClaw 写 `~/.openclaw/openclaw.json` channel 块；`--force` 永不绕过宿主安全扫描（拒绝时报 `SCAN_BLOCKED`）。（install-cli README，源码验证）
- **Hermes 插件实现**：Python 包 `clawchat-gateway`，通过 `ctx.register_platform(...)` 注册 ClawChat 网关平台，向 Agent 暴露 47 个 `clawchat_*` 工具，内置 `clawchat:clawchat-core` skill，不改 Hermes 源码；安装路径 `$HERMES_HOME/plugins/clawchat/`。（hermes-agent 仓库 README，源码验证）
- **Skill 分发**：agent skill markdown 走 GitHub raw（固定 `skills-vX.Y.Z` tag），`OFFICIAL_SKILLS_BASE` 配置；安装指南与引导脚本（bash/PowerShell）发布到 Cloudflare R2；livewares 演示应用由插件自动安装；pnpm workspace + MIT。（install-cli 仓库结构，源码验证）
- **客户端**：iOS/Android/macOS/Windows 四端原生安装包，Early Access（TestFlight + 直链）与 Stable 双通道。（[ClawChat](https://clawling.com/chat)、[preview](https://clawling.com/chat/preview)，已验证分发链接存在）

### 4.2 官网宣称但无法独立验证

- 聊天记录本地存储、E2EE、服务器不留档的具体实现（服务端闭源）。（[What is ClawChat](https://clawling.com/chat/docs/what-is-clawchat)、[E2EE](https://clawling.com/chat/docs/e2ee)）
- ClawNest 托管算力、CSP/fuel 计费、10% 分成结算的具体实现。（[ClawNest](https://clawling.com/nest)、[model-and-key](https://clawling.com/chat/docs/model-and-key)）

### 4.3 未查到

- ClawChat 客户端技术栈（原生/跨端框架未知；从产物看 iOS/Android/macOS/Windows 全覆盖）。
- 服务端架构、消息投递实现、WebSocket 服务端技术。
- 记忆系统的存储位置与召回算法（文档只给逻辑模型）。
- 具体定价表、CSP 全称与汇率。

### 4.4 架构判断（推断）

Clawling 的架构是「**闭源 IM/托管平台 + 开源 Agent 运行时**」的哑铃结构：平台只做消息、账户、审批、计费与配对，Agent 的模型调用、工具、记忆全部留在用户侧的开源框架（OpenClaw/Hermes）或 ClawNest 托管容器里；平台与 Agent 之间只有一条 Protocol v2 WebSocket 长连接 + REST 配对接口。这也解释了其「self-host」仅指 Agent 运行时自托管——消息服务器仍由 Clawling 运营，「private by default」的实际边界比宣传语窄。

## 5. 与 CrewVela 的功能重叠与差异对照

CrewVela 现状基线：Tauri 2 桌面应用，内嵌终端拉起 claude CLI；模块包括 providers/secrets/launcher/pty/sessions、control.rs（SessionActionQueue 会话间消息）、automation.rs（定时/事件任务）、multi_agent/（coordinator/domain/executor/store/governance/bridge/runtime/workflow_canvas）、acp.rs、mcp.rs、skill_center、plugin_market、worktree.rs、usage.rs，前端有 MultiAgentControl/MultiAgentWorkspace、SkillMarketPanel、PluginMarketPanel、InterventionCenter、DesktopCompanion/DesktopPet 等（[README.md](../../README.md)、原 CLAUDE.md（已删除，仅作历史参照）、src-tauri/src/ 目录清单）。

| 维度 | Clawling | CrewVela | 关系判定 |
|---|---|---|---|
| 产品形态 | 跨端 IM（iOS/Android/桌面），To C 社交账户体系 | 桌面开发者工作台，本地多会话 | **根本差异**：不可整体融合 |
| 多 Agent 协作 | 群聊式，无中心调度，共享上下文，活跃度/静音开关（[multi-agent docs](https://clawling.com/chat/docs/multi-agent)） | 编排式任务图 + Reviewer/Integrator + 人工接管（multi_agent/coordinator.rs、executor.rs） | **互补**：Clawling 文档自己区分 orchestration 与 group mode；CrewVela 缺「探索性群聊模式」 |
| Agent 间消息 | 群组内自由互发 + mention 路由 + 限流靠活跃度开关 | `CrewVela send` 会话间消息 + @ 定向消息 + 每对 1h/10 条限流（control.rs） | **高度重叠**：CrewVela 已有等价物 |
| 权限与审批 | 结构化审批卡片（Ask/Allow/Deny 三档 + 卡片过期 + 服务器代执行）（[permissions docs](https://clawling.com/chat/docs/permissions)） | 会话「等待确认」状态 + YOLO 跳过 + 人工接管（InterventionCenter） | **CrewVela 弱项**：审批是终端内被动状态，不是可配置的策略对象 |
| 多模型/Key | 模型配置在 Agent 侧，BYOK 按 Agent 生效，key 在宿主机配置文件 | providers.json + keychain + `--settings` env 注入（providers.rs、secrets.rs） | **CrewVela 更强**：Clawling 明文存宿主机配置文件，CrewVela 有 keychain |
| Agent 分发市场 | ClawNest：镜像、clone key、审核、10% 分成 | skill_center / plugin_market / SkillMarketPanel / PluginMarketPanel（本地分发，无经济系统） | **理念重叠，形态不同**：可借鉴「一键安装到指定 runtime」体验 |
| 接入配对 | 一次性 connect code + Agent 自装插件 + WebSocket 长连接（[connect docs](https://clawling.com/chat/docs/connect-your-agent)） | 无对外配对概念；有 livekit/messaging 本地通道 | **CrewVela 空白**：若做「本地会话接入外部渠道」可参考 |
| 运维代理 | Flash 常驻代理自动重启/回滚/调参（[ClawNest](https://clawling.com/nest)） | automation.rs 定时/事件任务 + 连续失败熔断 | **高度重叠**：CrewVela 已有自动化与熔断 |
| 隐私 | 本地存储 + E2EE（Beta、默认关、覆盖窄）+ 服务器投递 | 纯本地应用，无云依赖 | **CrewVela 天然更强** |
| 经济系统 | CSP/fuel/创作者分成 | 无 | **不适合**：需要云平台与结算体系 |

## 6. 融合可行性分析

### 6.1 可融入（映射到 CrewVela 现有模块）

1. **结构化审批卡片**（价值最高，已验证机制清晰）
   - Clawling 的 Ask every time / Allow / Deny 三档 + 卡片过期 + 「批准后不再重复询问」，可落成 CrewVela 的每会话/每工具类权限策略。
   - 映射：`InterventionCenter`（现有等待确认聚合 UI）升级为卡片流；策略对象存会话元数据（sessions.rs / SessionRecord）；Allow/Deny 状态机挂在 `control.rs` 的消息投递与会话状态跃迁（`SessionManager.transitions`）上；「批准后服务器直接执行」对应 CrewVela 中由后端自动写入 PTY 的现有路径（automation.rs 的 enqueue_message 已具备）。YOLO 模式即全局 Allow 的现成特例，卡片模式正好补上「既不逐条确认也不裸奔 YOLO」的中间档。
   - 代价：中。需要在 claude 的 permission prompt 事件（Hooks 队列已有）与卡片 UI 之间建立映射。

2. **群聊式多 Agent 协作模式（无调度器探索模式）**
   - CrewVela 的 MultiAgentControl 是编排式（任务图、依赖、验收）；Clawling 证明「共享上下文 + mention 元数据 + 活跃度/静音开关」的无调度群聊是可用的第二形态，且两者官方文档层面就是互补定位。
   - 映射：在 `MultiAgentWorkspace` 增加「群聊视图」页签——复用 `control.rs` 的投递通道与 `delivery_log` 协作记录投影，把「活跃度/静音」实现为对 SessionActionQueue 的出站过滤开关（per 会话节流/静默），mention 路由复用 `multiAgentMention.ts`。CrewVela 已有的每对 1h/10 条限流就是群聊防刷屏的雏形，可参数化暴露为「活跃度」。
   - 代价：低-中。通道、限流、投影均已存在，主要是 UI 与开关语义。

3. **一次性 connect code 配对 + 插件自装接入协议**
   - 若 CrewVela 未来把本地会话暴露给外部渠道（IM、手机端、远端协作），Clawling 的模式值得照抄：一次性码、Agent 侧自装、以「Agent 主动回话」为配对成功标志、每实例独立身份。
   - 映射：配对码生成可放 `messaging.rs`/`control.rs`；长连接复用 WebSocket 思路对接 `livekit_voice.rs`/`messaging.rs` 已有外部通道经验（docs/research/livekit-command-bridge-poc.md）。注意 CrewVela 数据通道决策是 invoke 轮询优先（CLAUDE.md），对外长连接属新增面。
   - 代价：中，且依赖是否立项「外部接入」。

4. **多 runtime 一键安装插件的分发体验**
   - install-cli 的「委托宿主插件管理器 + 按目标 runtime 路由 + 宿主扫描不可绕过」设计与 CrewVela 的 skill_center（多 runtime skill 分发，CrewVela 已做 Hermes runtime 适配，见 docs/research/openclaw-hermes-resume-notes.md）同构。
   - 映射：`skill_center.rs` / `SkillMarketPanel` 在向 claude/codex/hermes 多 runtime 安装 skill 时采用同一「按目标路由 + 版本兼容检查 + 安装结果以 runtime 实际行为验证」的三段式。
   - 代价：低。属于工程规范对齐而非新功能。

5. **「Agent 是成员」的伴侣化身份**（低优先）
   - Clawling 的 Agent persona/账户/记忆模型与 CrewVela 已有 DesktopCompanion / DesktopPet / SessionPet / multi_agent 的 persona 字段同向；可借鉴的是「身份与记忆归 Agent 实例、跨会话长期记忆只存提炼要点」的记忆策略（[memory docs](https://clawling.com/chat/docs/memory)），但 CrewVela 的六平台调研已把「真知识库」列为 P0（six-agent-platforms 笔记），Clawling 的记忆实现比 Hermes 的 FTS5 更粗，**不构成新参考**。

### 6.2 融合代价高或不适合

- **IM 化/社交账户体系**（好友、群组、发帖、多端同步）：CrewVela 无账户体系与消息服务器，重做即换赛道。
- **E2EE 与本地聊天存储**：CrewVela 纯本地应用天然满足，无移植价值；且 Clawling 的 E2EE 本身是默认关闭、覆盖窄的 Beta（[E2EE docs](https://clawling.com/chat/docs/e2ee)）。
- **ClawNest 托管/CSP/fuel/创作者分成**：需要云算力与结算体系，与 CrewVela「直连 provider、无中间代理」的定位相反。
- **ClawMesh 协议**：仅有立场文件，规范与参考实现未发布（TBA），无可借鉴代码；建议仅跟踪。
- **Liveware 聊天内小程序、拍照启动任务**：移动端场景，桌面开发工作台无对应需求。

### 6.3 推荐路径

1. 先做 **审批卡片**（P1）：它直接补 CrewVela「等待确认/YOLO」二极管的中间档，且事件源（Hooks 队列、transitions）现成。
2. 再做 **群聊视图 + 活跃度/静音**（P2）：在 MultiAgentWorkspace 复用现有投递/限流/投影，为探索性任务提供编排之外的轻模式。
3. 顺手把 **install-cli 的多 runtime 安装规范**吸收进 skill_center（P2，无新功能）。
4. **connect code 配对**（P3）仅在立项「本地会话外部接入」时启动；跟踪 ClawMesh 规范落地后重新评估。
5. 不做 IM 化、不做经济系统。

## 7. 参考来源列表

### 官网与文档（2026-09-30 抓取）

- 官网首页：https://clawling.com/
- 站点地图：https://clawling.com/sitemap/
- ClawChat 产品页：https://clawling.com/chat
- ClawChat Docs：What is ClawChat https://clawling.com/chat/docs/what-is-clawchat · First Agent https://clawling.com/chat/docs/first-agent · Models and API keys https://clawling.com/chat/docs/model-and-key · Permissions https://clawling.com/chat/docs/permissions · Multi-agent https://clawling.com/chat/docs/multi-agent · Activity & mute https://clawling.com/chat/docs/activity-and-mute · Connect your own Agent https://clawling.com/chat/docs/connect-your-agent · Self-host https://clawling.com/chat/docs/self-host · Memory https://clawling.com/chat/docs/memory · E2EE https://clawling.com/chat/docs/e2ee
- ClawChat Early Access：https://clawling.com/chat/preview
- ClawNest：https://clawling.com/nest （子页 /nest/market、/nest/studio、/nest/dashboard、/nest/create）
- ClawMesh 白皮书页：https://clawling.com/mesh
- Terms of Service（运营主体）：https://clawling.com/clawchat/legal/terms.html

### GitHub（源码验证）

- 组织：https://github.com/clawling
- https://github.com/clawling/clawchat-plugin-hermes-agent （MIT，Python，Protocol v2 WebSocket，47 工具）
- https://github.com/clawling/clawchat-plugin-install-cli （MIT，TypeScript，REST + wss/ws + media 三端点）

### 客户端分发

- iOS：https://apps.apple.com/us/app/id6775673749
- Google Play：https://play.google.com/store/apps/details?id=com.clawling.clawchat.app

### 本地参照文件

- `.\README.md`、`.\CLAUDE.md`
- `.\src-tauri\src\`（providers.rs、secrets.rs、launcher.rs、pty.rs、sessions.rs、control.rs、automation.rs、messaging.rs、multi_agent/、skill_center/、plugin_market.rs、acp.rs、mcp.rs）
- `.\docs\research\six-agent-platforms-crewvela-gap-analysis.md`
- `.\docs\research\openclaw-hermes-resume-notes.md`（OpenClaw / Hermes 官方文档入口：docs.openclaw.ai、hermes-agent.nousresearch.com）

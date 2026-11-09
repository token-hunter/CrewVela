# CC Switch 本地源码“添加供应商”完整链路与 CrewVela 差距分析

## 1. 研究范围与结论

- 研究日期：2026-08-11
- CC Switch 本地源码：REFERENCE_ROOT/cc-switch
- CC Switch 固定提交：076c2744ceb622b85771bff57668d43ed70809f8
- CrewVela 本地源码：.
- CrewVela 对照提交：499300cb10530b733006a9964108a4d76ca0126a
- 研究范围：仅追踪“添加、编辑模型服务商”相关链路，不把 CC Switch 的全部产品能力视为 CrewVela 的需求。

结论：

1. CC Switch 最值得借鉴的不是它的五十多个预设，而是“类型化预设注册表 → 卡片选择 → 整份配置自动回填 → 用户主要填写 API Key → 新增和编辑复用同一表单 → 前后端双层校验 → 保存后同步运行配置”的完整链路。
2. CrewVela 已经具备五个预设、预设自动填充、统一新增/编辑表单、前后端校验和系统凭据库，基础方向正确；主要短板是预设元数据太薄、帮助入口和端点候选缺失、预设归属靠字符串猜测、测试链路不完整，以及 providers.json 的写入和删除凭据流程仍可加强。
3. CrewVela 只支持 Kimi、GLM、DeepSeek、MiniMax、自定义时，不需要照搬 CC Switch 的多应用切换、统一供应商、代理接管、OAuth、赞助商排序、五十多个预设和导入导出体系。
4. CC Switch 截图中的星形和心形不是用户收藏功能，而是商业合作伙伴和优先合作伙伴标识。源码中没有收藏状态、收藏 API、收藏持久化或收藏测试。
5. CrewVela 的系统凭据库方案优于 CC Switch 普通供应商把 API Key 放在 SQLite JSON 配置中的做法，应保留，不能为了“参考 CC Switch”而回退。

## 2. 完整调用链

CC Switch 的新增链路如下：

1. App 根据当前应用类型渲染供应商列表和“新增”按钮。
2. 点击新增打开 AddProviderDialog，并把 activeApp 传入。
3. AddProviderDialog 根据应用类型决定显示专用供应商表单，还是额外显示“统一供应商”页签。
4. ProviderForm 根据应用类型选取对应的预设目录。
5. ProviderPresetSelector 负责预设卡片、搜索、排序和静态合作伙伴徽标。
6. 用户选择预设后，ProviderForm.handlePresetChange 一次性回填名称、官网、配置模板、API 格式、Key 字段、端点和模型映射。
7. 表单 hooks 把 API Key、Endpoint、模型字段和底层 JSON/TOML 配置双向同步。
8. Zod 做结构校验，ProviderForm 再做业务硬校验和可确认的软校验。
9. AddProviderDialog 组装 Provider 和端点元数据。
10. useProviderActions → useAddProviderMutation → providersApi.add 调用 Tauri add_provider。
11. ProviderService::add 再次归一化和校验，写入 SQLite，并按首个供应商、累加模式等规则同步实际运行配置。

核心证据：

- 前端入口和状态：REFERENCE_ROOT/cc-switch\src\App.tsx:127-145、177-184、995-1053、1628-1655
- 应用切换：REFERENCE_ROOT/cc-switch\src\components\AppSwitcher.tsx:27-58、97-161、169-228
- 新增面板：REFERENCE_ROOT/cc-switch\src\components\providers\AddProviderDialog.tsx:42-58、112-317、364-421
- 预设选择和应用：REFERENCE_ROOT/cc-switch\src\components\providers\forms\ProviderForm.tsx:681-714、1744-1956
- 新增动作：REFERENCE_ROOT/cc-switch\src\hooks\useProviderActions.ts:46-49、78-139
- 前端 mutation：REFERENCE_ROOT/cc-switch\src\lib\query\mutations.ts:19-155
- Tauri API：REFERENCE_ROOT/cc-switch\src\lib\api\providers.ts:49-99
- Tauri 命令：REFERENCE_ROOT/cc-switch\src-tauri\src\commands\provider.rs:24-83
- 后端服务：REFERENCE_ROOT/cc-switch\src-tauri\src\services\provider\mod.rs:2550-2597、4158-4289
- SQLite 持久化：REFERENCE_ROOT/cc-switch\src-tauri\src\database\dao\providers.rs:180-276

## 3. 前端入口与应用类型切换

CC Switch：

- App.tsx:127-145 使用 cc-switch-last-app 恢复上次选中的应用，默认 Claude。
- App.tsx:177-184 保存 activeApp 和 isAddOpen。
- App.tsx:995-1053 根据 activeApp 渲染对应 ProviderList，并由 onCreate 打开新增面板。
- App.tsx:1307-1315 把 activeApp、onSwitch、visibleApps 注入 AppSwitcher。
- App.tsx:1628-1655 渲染顶栏新增按钮，并把 activeApp 作为 appId 交给 AddProviderDialog。
- AppSwitcher.tsx:27-58 定义应用集合和图标；97-110 切换应用并写 localStorage；112-161 处理隐藏应用回退和响应式溢出；169-228 渲染直接按钮和更多菜单。

CrewVela：

- .\src\App.tsx:870-881 直接创建空草稿并打开 ProviderEditor。
- .\src\App.tsx:1263-1274 渲染 ProviderEditor。
- CrewVela 的“模型服务”是单一业务域，没有 Claude、Codex、Gemini 等应用维度。

判断：

- CC Switch 的多应用切换是其产品结构需要，不是 CrewVela 当前五个 Claude-compatible 模型服务商的必要条件。
- CrewVela 不应为了参考 CC Switch 增加应用类型切换，否则会把“模型服务商”与“终端 Runtime”混成两个概念。

## 4. 预设数据结构与五个服务商

### 4.1 CC Switch 预设结构

REFERENCE_ROOT/cc-switch\src\config\claudeProviderPresets.ts:24-57 的 ProviderPreset 包含：

- 展示：name、nameKey、图标、主题色
- 帮助：websiteUrl、apiKeyUrl
- 配置：settingsConfig、apiFormat、apiKeyField、templateValues
- 端点：endpointCandidates
- 分类：category
- 商业元数据：isOfficial、isPartner、primePartner、partnerPromotionKey
- 特殊能力：providerType、requiresOAuth、modelsUrl、hidden

这使“预设卡片、表单自动填充、帮助链接、端点选择、模型获取、保存元数据”都由同一个注册表驱动。

### 4.2 CC Switch 中与 CrewVela 五项相关的预设

| 服务商 | CC Switch 本地源码 | 内置端点与模型 |
|---|---|---|
| Kimi | REFERENCE_ROOT/cc-switch\src\config\claudeProviderPresets.ts:76-115 | Moonshot Anthropic 端点和 Kimi For Coding 端点；带模型及 Haiku、Sonnet、Opus 映射 |
| DeepSeek | REFERENCE_ROOT/cc-switch\src\config\claudeProviderPresets.ts:791-807 | DeepSeek Anthropic 端点、模型映射、modelsUrl |
| GLM | REFERENCE_ROOT/cc-switch\src\config\claudeProviderPresets.ts:831-864 | 国内 bigmodel 与国际 z.ai 两个预设 |
| MiniMax | REFERENCE_ROOT/cc-switch\src\config\claudeProviderPresets.ts:1015-1063 | 国内与国际两个预设，带 timeout、模型映射和关闭非必要流量配置 |
| 自定义 | REFERENCE_ROOT/cc-switch\src\components\providers\forms\ProviderPresetSelector.tsx:382-402 | 固定显示的空白自定义入口 |

### 4.3 CrewVela 当前结构

.\src\providerPresets.ts:3-24、52-123：

- 只有 id、label、description、tone、template。
- 五个固定项为 Kimi、GLM、DeepSeek、MiniMax、自定义。
- template 已能填 baseUrl、authTokenRef、env、timeout 和关闭非必要流量配置。
- .\src\providerPresets.ts:125-133 通过 id/name 包含特定字符串猜测服务商类别。
- .\src\providerPresets.ts:156-186 生成不重复的新 ID、名称和凭据引用。

差距：

- 缺少官网、申请 API Key、端点候选、模型列表来源、显式预设 ID、配置版本等元数据。
- 通过字符串猜测类别会把名为“My Kimi Proxy”的自定义服务错误识别为 Kimi。应把 presetId 明确存进服务商配置，而不是依赖名称。
- 两边的端点和模型版本已有差异。不能直接复制 CC Switch 当前值覆盖 CrewVela，应先确认 CrewVela Runtime 真正支持的协议和模型，再做带迁移策略的预设升级。

## 5. 分类、搜索、排序、“收藏”和赞助标识

REFERENCE_ROOT/cc-switch\src\components\providers\forms\ProviderPresetSelector.tsx：

- 85-121：Original 排序为 official → primePartner → isPartner → 普通项；A-Z 按名称排序。
- 152-205：搜索状态、外部点击关闭、Ctrl/Cmd+F、过滤和排序。
- 207-235：分类提示。
- 330-379：搜索框和排序按钮。
- 382-402：自定义项常驻、空结果提示。
- 404-444：预设卡片及 Heart、Star 徽标。

“收藏”核对结果：

- REFERENCE_ROOT/cc-switch\src\config\claudeProviderPresets.ts:33-34 明确定义 isPartner 为商业合作伙伴，primePartner 为优先合作伙伴。
- ProviderPresetSelector.tsx:416-444 只读取静态元数据渲染 Heart、Star；徽标本身没有点击行为，唯一动作是选择预设卡片。
- 全仓不存在预设收藏状态、收藏 API、localStorage 收藏数据或收藏测试。
- REFERENCE_ROOT/cc-switch\docs\release-notes\v3.16.4-zh.md:99、103、115 也说明星形和心形是合作伙伴标识。

对 CrewVela 的判断：

- 只有五个固定项时，搜索、A-Z 排序、分类和响应式预设溢出没有实际收益。
- 不应复制合作伙伴排序和赞助徽标。
- 如果以后真的需要收藏，应作为 CrewVela 自己的新需求设计，不能声称是复刻 CC Switch。

## 6. 选择预设后的自动填充

CC Switch：

- ProviderForm.tsx:681-714 根据 appId 选择 Claude、Codex、Gemini、OpenCode、OpenClaw、Hermes 等预设目录。
- ProviderForm.tsx:1744-1774 的 handlePresetChange 处理选择预设和 custom 重置。
- ProviderForm.tsx:1777-1787 把分类、合作伙伴信息写入元数据。
- ProviderForm.tsx:1789-1899 分别处理 Codex、Gemini、OpenCode、OpenClaw、Hermes。
- ProviderForm.tsx:1902-1923 对 Claude 预设应用 templateValues，并一次性回填名称、官网、整份配置、图标、API format 和 Key 字段。
- ProviderForm.tsx:1940-1956 只在新增模式显示预设选择器，编辑模式直接复用同一表单。

底层字段双向同步：

- API Key：REFERENCE_ROOT/cc-switch\src\components\providers\forms\hooks\useApiKeyState.ts，以及 REFERENCE_ROOT/cc-switch\src\utils\providerConfigUtils.ts:200-224、330-370
- Endpoint：REFERENCE_ROOT/cc-switch\src\components\providers\forms\hooks\useBaseUrlState.ts:44-148
- 模型：REFERENCE_ROOT/cc-switch\src\components\providers\forms\hooks\useModelState.ts:9-192
- 模板变量：REFERENCE_ROOT/cc-switch\src\utils\providerConfigUtils.ts:230-273
- Codex TOML 的 Endpoint 和模型采用保留无关行的定向更新：REFERENCE_ROOT/cc-switch\src\utils\providerConfigUtils.ts:400-1397

CrewVela：

- .\src\components\ProviderEditor.tsx:66-74 选择预设后，直接把 template 复制为草稿并切入表单。
- .\src\components\ProviderEditor.tsx:76-83 可返回预设页。
- .\src\components\ProviderEditor.tsx:253-417 编辑名称、端点、Key、ID、timeout、env、模型映射和流量选项。

判断：

- CrewVela 已实现核心自动填充，不需要重写成 CC Switch 的复杂 hooks。
- 应补的是预设元数据、明确的 presetId、帮助链接和更清晰的模型映射字段；不是引入多应用 JSON/TOML 转换层。

## 7. 表单字段、官网与帮助

CC Switch 的基础字段：

- 名称、备注、官网：REFERENCE_ROOT/cc-switch\src\components\providers\forms\BasicFormFields.tsx:127-173
- API Key 与申请链接：REFERENCE_ROOT/cc-switch\src\components\providers\forms\shared\ApiKeySection.tsx:40-65
- Endpoint：REFERENCE_ROOT/cc-switch\src\components\providers\forms\shared\EndpointField.tsx:40-91
- Codex 模型、模型映射、高级配置和端点测速：REFERENCE_ROOT/cc-switch\src\components\providers\forms\CodexFormFields.tsx:556-600、619-1101
- Gemini 字段：REFERENCE_ROOT/cc-switch\src\components\providers\forms\GeminiFormFields.tsx:80-204
- OpenCode 模型和在线获取：REFERENCE_ROOT/cc-switch\src\components\providers\forms\OpenCodeFormFields.tsx:224-248、550-1141
- Hermes 字段：REFERENCE_ROOT/cc-switch\src\components\providers\forms\HermesFormFields.tsx:157-170、200-466

CrewVela 已有名称、Endpoint、API Key、ID、timeout、env 和流量控制，但没有：

- 服务商官网
- 获取 API Key 的直接入口
- Endpoint 候选
- 模型列表来源
- 测试连接或端点测速
- 备注

建议：

- 官网、API Key 申请入口应加入预设元数据并展示，成本低、实用性高。
- 五个固定服务商只需一套 Endpoint，不必直接复制 CC Switch 的候选端点管理器；若确实存在国内、国际两个端点，可用预设变体或简单选择器。
- “测试连接”值得加入，但应调用 CrewVela 自己的 Claude-compatible 最小探测请求，不能复制 Codex 专用测速逻辑。
- 备注字段不是刚需，可后置。

## 8. 模型映射和环境变量

CC Switch 的 Claude 兼容配置把以下字段作为一组处理：

- ANTHROPIC_MODEL
- ANTHROPIC_DEFAULT_HAIKU_MODEL
- ANTHROPIC_DEFAULT_SONNET_MODEL
- ANTHROPIC_DEFAULT_OPUS_MODEL
- 部分预设还包含 Fable、Subagent、timeout、上下文窗口和关闭非必要流量的环境变量

主要实现：

- REFERENCE_ROOT/cc-switch\src\components\providers\forms\hooks\useModelState.ts:41-73、120-192
- REFERENCE_ROOT/cc-switch\src\config\claudeProviderPresets.ts:76-115、791-864、1015-1063

CrewVela：

- .\src\providerPresets.ts:52-123 已在 template.env 中保存相关模型映射。
- .\src\components\ProviderEditor.tsx:85-97、319-417 支持用户修改 env 和高级项。
- .\src-tauri\src\providers.rs:104-157 会检查重复环境变量等结构问题。

建议：

- 保留 CrewVela 当前扁平 ProviderConfig 和 env 数组，不需要引入 CC Switch 的多应用配置解析器。
- 把常用模型映射做成结构化表单，env 原始编辑放在高级设置中。
- 预设配置应带 schemaVersion，未来模型更新时区分“新建时使用新默认值”和“是否迁移已有用户配置”，避免静默覆盖。

## 9. 验证、软错误和错误处理

CC Switch 前端：

- REFERENCE_ROOT/cc-switch\src\lib\schemas\provider.ts:39-57 校验名称、网址、配置非空和 JSON 可解析。
- ProviderForm.tsx:1016-1031 本地代理覆盖格式错误为硬失败。
- ProviderForm.tsx:1033-1054 名称、模板问题可进入软提醒。
- ProviderForm.tsx:1056-1068 价格配置非法为硬失败。
- ProviderForm.tsx:1070-1154 校验 providerKey、重复和模型配置。
- ProviderForm.tsx:1156-1180 处理 OAuth 登录要求。
- ProviderForm.tsx:1264-1327 对非官方服务的空 Endpoint、空 Key 等收集 softIssues，允许用户确认后保存。
- ProviderForm.tsx:2635-2669 提供“仍然保存”。

CC Switch 后端：

- REFERENCE_ROOT/cc-switch\src-tauri\src\services\provider\mod.rs:4158-4289 按应用类型再次校验配置对象、TOML、成本和用量配置。
- REFERENCE_ROOT/cc-switch\src\lib\query\mutations.ts:101-155 统一做缓存失效、托盘刷新、成功和失败提示。
- 统一供应商采用“保存”和“同步”两阶段，后者失败会提示“已保存但同步失败”：REFERENCE_ROOT/cc-switch\src\components\providers\AddProviderDialog.tsx:64-104。

CrewVela：

- .\src\components\ProviderEditor.tsx:99-154 校验名称、ID 唯一性、HTTP Endpoint 和 Key。
- .\src-tauri\src\providers.rs:104-157 再次校验 ID、名称、凭据引用、HTTP URL、timeout 和 env 重复。
- .\src\App.tsx:883-890 将 Tauri 保存错误反馈给编辑器。

建议：

- CrewVela 当前五个明确服务商应继续把 Endpoint 和 Key 缺失视为硬错误，不必复制 CC Switch 为高度可定制场景设计的“仍然保存”。
- 可增加“测试失败但允许保存”这一种软提醒，因为网络失败不等于配置一定无效。
- 所有错误应保持来自后端的可操作文本，明确是 Endpoint、Key、模型还是系统凭据库失败。

## 10. 保存、持久化和凭据处理

### 10.1 CC Switch

新增：

- AddProviderDialog.tsx:112-135 解析 settingsConfig 并构造 Provider。
- AddProviderDialog.tsx:137-167 处理 official seed 和累加应用 providerKey。
- AddProviderDialog.tsx:169-308 把 endpointCandidates 和实际 Base URL 归一化到 meta.custom_endpoints。
- AddProviderDialog.tsx:311-317 保存并关闭。
- useAddProviderMutation 位于 REFERENCE_ROOT/cc-switch\src\lib\query\mutations.ts:19-155。
- Tauri invoke 位于 REFERENCE_ROOT/cc-switch\src\lib\api\providers.ts:58-64。
- add_provider 位于 REFERENCE_ROOT/cc-switch\src-tauri\src\commands\provider.rs:34-48。
- ProviderService::add 位于 REFERENCE_ROOT/cc-switch\src-tauri\src\services\provider\mod.rs:2550-2597。
- SQLite transaction 位于 REFERENCE_ROOT/cc-switch\src-tauri\src\database\dao\providers.rs:180-276。

普通 API Key：

- Provider 结构的 settings_config 保存整份配置：REFERENCE_ROOT/cc-switch\src-tauri\src\provider.rs:13-27。
- 不同应用的凭据从配置中提取：REFERENCE_ROOT/cc-switch\src-tauri\src\provider.rs:132-228。
- 普通 Claude、Codex、Gemini 服务商的 Key 随配置 JSON 保存到 SQLite；只有部分受管 OAuth 是专用存储路径。

风险：

- ProviderService::add 先保存数据库，再写实际运行配置，数据库与 live 文件不是同一个事务。后一步失败时可能出现“已存数据库但未正确激活”的状态。

### 10.2 CrewVela

- .\src-tauri\src\lib.rs:191-240 提供 list/get/save/delete provider 命令。
- save_provider 先写系统凭据库，再写 providers.json；若 provider 写入失败，会回滚新写入的凭据。
- .\src-tauri\src\secrets.rs:1-29 使用系统 keyring，service 名为 CrewVela。
- .\src-tauri\Cargo.toml:27-28 启用 Windows 和 Apple 凭据库后端。
- .\src-tauri\src\providers.rs:37-102 读取、补默认值并写 providers.json。
- .\src\App.tsx:892-897 删除 provider 后再单独删除 Key。

建议：

- 保留系统凭据库，绝不能复制 CC Switch 普通服务商把 Key 放进配置 JSON 的做法。
- providers.json 应改为“临时文件写入 + flush + rename”以降低进程崩溃导致截断的风险。
- 删除 provider 和删除 Key 现在是两个前端调用，且 Key 删除错误被吞掉。应增加一个后端 delete_provider_with_secret 命令，明确处理部分失败和重试。

## 11. 编辑已有供应商

CC Switch：

- REFERENCE_ROOT/cc-switch\src\components\providers\EditProviderDialog.tsx:43-128 只在当前启用供应商时读取 live 配置；代理接管时回退数据库单一事实源。
- EditProviderDialog.tsx:130-151 对 Codex 的 modelCatalog 强制保留数据库版本。
- EditProviderDialog.tsx:153-207 把旧数据转为 initialData，复用 ProviderForm，并合并旧身份和元数据。
- EditProviderDialog.tsx:214-241 渲染同一表单；编辑模式不显示预设选择器。
- 更新继续走 providersApi.update → update_provider → ProviderService::update，并复用后端校验。

CrewVela：

- ProviderEditor 同时处理新增和编辑：.\src\components\ProviderEditor.tsx:35-59、99-154。
- 这是正确方向，没必要拆成两套表单。

需要补强：

- 明确 presetId 后，编辑已有内置预设可以显示“来源于 Kimi/GLM”等，而不是按名称猜测。
- 如果未来提供“恢复预设默认值”，必须只在用户确认后覆盖 Endpoint、模型和 env，绝不能覆盖系统凭据库中的 API Key。

## 12. 启用状态、代理、统一供应商与导入导出

### 12.1 启用状态

CC Switch：

- ProviderService::add 在首个非累加供应商创建时将其设为 current 并写 live 配置：REFERENCE_ROOT/cc-switch\src-tauri\src\services\provider\mod.rs:2587-2595。
- 累加模式可以只存数据库而不写 live：同文件 2563-2584。
- removeFromLiveConfig 只移除 live 配置、保留数据库行：REFERENCE_ROOT/cc-switch\src\lib\api\providers.ts:82-87。

CrewVela：

- 模型服务是会话启动配置来源，不是一个全局唯一 current provider。
- 因此不应复制“新增第一个服务商自动激活并改写 CLI 配置”的规则。

### 12.2 代理接管

- UI 开关：REFERENCE_ROOT/cc-switch\src\App.tsx:1277-1296
- 接管状态、备份和恢复：REFERENCE_ROOT/cc-switch\src-tauri\src\services\proxy.rs:696-915
- 热切换：REFERENCE_ROOT/cc-switch\src-tauri\src\services\proxy.rs:2439-后续

这是 CC Switch 的运行时接管能力，不属于“选择五个模型服务商并保存”的核心流程，CrewVela 不应在本次重构中引入。

### 12.3 统一供应商

- AddProviderDialog.tsx:42-58 决定哪些应用显示统一供应商页签。
- REFERENCE_ROOT/cc-switch\src\config\universalProviderPresets.ts:18-131 定义统一供应商预设。
- REFERENCE_ROOT/cc-switch\src\components\providers\UniversalProviderFormModal.tsx:44-299、367-460、484-后续 把一个入口映射到 Claude、Codex、Gemini 多种配置。
- API：REFERENCE_ROOT/cc-switch\src\lib\api\providers.ts:213-246。

CrewVela 当前五个项目都是同一种 Claude-compatible 配置，不需要统一供应商抽象。

### 12.4 导入导出

- ProviderList 可从当前 live 配置导入：REFERENCE_ROOT/cc-switch\src\components\providers\ProviderList.tsx:210-240、356-362。
- Deep Link 是另一条入口：REFERENCE_ROOT/cc-switch\src-tauri\src\deeplink\provider.rs:15-68。
- 全量 SQL 备份导入导出属于设置级功能：REFERENCE_ROOT/cc-switch\src-tauri\src\commands\import_export.rs:17-59。

判断：

- 从已有 Claude 配置导入可以作为未来独立需求，但不是当前“新增五个服务商”表单必须项。
- Deep Link 和全量备份不应混入本次重构。

## 13. 测试覆盖

CC Switch 现有测试：

- 预设搜索、排序、分组、空状态、自定义项、快捷键、外部点击：REFERENCE_ROOT/cc-switch\tests\components\ProviderPresetSelector.test.tsx:181-589
- AddProviderDialog 的 custom_endpoints、Base URL 回填和自定义图标：REFERENCE_ROOT/cc-switch\tests\components\AddProviderDialog.test.tsx:62-168
- 应用切换刷新列表：REFERENCE_ROOT/cc-switch\tests\integration\App.test.tsx:199-214
- 编辑时 modelCatalog 和代理接管回退：REFERENCE_ROOT/cc-switch\tests\components\EditProviderDialog.test.tsx:101-162
- Codex catalog 归一化：REFERENCE_ROOT/cc-switch\tests\components\ProviderForm.codexCatalog.test.ts:5-35
- 新增、更新 mutation：REFERENCE_ROOT/cc-switch\tests\hooks\useAddProviderMutation.test.tsx:72-141、REFERENCE_ROOT/cc-switch\tests\hooks\useUpdateProviderMutation.test.tsx:75-120
- 预设形状：REFERENCE_ROOT/cc-switch\tests\config\claudeProviderPresets.test.ts:4-121
- 后端 add、update、凭据归一化、校验、累加模式：REFERENCE_ROOT/cc-switch\src-tauri\src\services\provider\mod.rs:440-689、690-后续、1307-后续、1819-后续
- 命令、启动导入、切换、缺少 auth 和代理接管导入拒绝：REFERENCE_ROOT/cc-switch\src-tauri\tests\provider_commands.rs:38-669

CC Switch 自身仍缺少：

- 没有一个真实端到端测试覆盖“点击 Kimi/GLM 预设 → 自动填充 Endpoint/模型 → 输入 API Key → 提交 → Tauri → SQLite → live 配置”。
- 没有覆盖“切换应用后打开新增窗口，预设目录随应用正确变化”的完整测试。

CrewVela：

- .\src-tauri\src\providers.rs:290-410 覆盖默认五项、自定义、DeepSeek、重排、归一化、非法 Endpoint、ID 和 timeout。
- 当前缺少 ProviderEditor 的预设选择、字段回填、Key 状态、新增/编辑错误展示测试。
- 当前缺少 save_provider 对“keyring 成功但 providers.json 失败后回滚”的故障注入测试。
- 当前缺少删除 provider 后凭据清理失败的测试。

## 14. 截图可见与源码隐含能力

| 能力 | 截图是否直接可见 | 源码证据或说明 |
|---|---:|---|
| Claude 供应商 / 统一供应商页签 | 是 | AddProviderDialog.tsx:364-400 |
| 预设网格和自定义配置 | 是 | ProviderPresetSelector.tsx:382-444 |
| 搜索按钮和 A-Z 排序 | 是 | ProviderPresetSelector.tsx:330-379 |
| 星形、心形徽标 | 是 | 是赞助/合作标识，不是收藏 |
| 取消、添加 | 是 | AddProviderDialog.tsx:322-360 |
| 当前应用决定预设目录 | 否 | ProviderForm.tsx:681-714 |
| 上次应用记忆 | 否 | App.tsx:127-145 |
| 隐藏应用回退和响应式更多菜单 | 否 | AppSwitcher.tsx:112-228 |
| 搜索只匹配原名和翻译名 | 否 | ProviderPresetSelector 测试 181-202 |
| Ctrl/Cmd+F、Esc、外部点击 | 否 | ProviderPresetSelector.tsx:152-205 及测试 486-589 |
| official/prime partner/partner 排序语义 | 否 | ProviderPresetSelector.tsx:85-121 |
| endpointCandidates 归一化 | 否 | AddProviderDialog.tsx:169-308 |
| 普通 Key 随配置保存到 SQLite | 否 | provider.rs:13-27、database/dao/providers.rs:180-276 |
| 保存数据库后同步 live 可能单独失败 | 否 | ProviderService::add:2550-2597 |

## 15. 逐项差距矩阵

| 维度 | CC Switch | CrewVela 当前 | 差距 | 决策 |
|---|---|---|---|---|
| 产品范围 | 多应用、多协议、大预设库 | 五个 Claude-compatible 服务商 | CrewVela 范围更窄 | 保持五项，不复制应用切换 |
| 预设注册表 | 展示、帮助、端点、模型、协议、商业元数据完整 | 只有展示和 template | 元数据偏薄 | 扩展但只保留必要字段 |
| 预设选择 | 搜索、排序、分类、响应式网格 | 五卡片直接选择 | 五项无需搜索排序 | 保持简单卡片 |
| 预设归属 | 显式 preset entry | 按 id/name 字符串猜测 | 有误分类风险 | 保存显式 presetId |
| 自动填充 | 整份配置 reset，按应用分支 | 已复制 template | 核心已具备 | 不重写，只补元数据 |
| API Key | 表单和配置 JSON 双向同步 | 表单 + 系统 keyring | CrewVela 更安全 | 保留 keyring |
| Endpoint | 候选、双向同步、部分测速 | 单一文本输入 | 缺帮助和测试 | 增加官网、候选或简单测试 |
| 模型映射 | 结构化 hooks，多应用适配 | env 模板 + 高级编辑 | 常用字段不够直观 | 做五项专用结构化表单 |
| 官网/申请 Key | 预设自带链接 | 无 | 用户需要自行查找 | 应复刻 |
| 备注 | 支持 | 无 | 非关键 | 后置 |
| 启用/current | 写 live，全局 current | 会话选择模型服务 | 语义不同 | 不复制 |
| 代理接管 | 完整代理和备份恢复 | 无 | 非本次需求 | 不复制 |
| 统一供应商 | 一套配置派生多应用 | 无 | 五项不需要 | 不复制 |
| 导入/导出 | live 导入、Deep Link、SQL 备份 | 无此链路 | 独立需求 | 不纳入本次重构 |
| 收藏 | 不存在 | 不存在 | 无差距 | 不新增、不误读 |
| 赞助标识 | 静态商业元数据 | 无 | CrewVela 不需要 | 不复制 |
| 前端验证 | Zod + 硬/软错误 | 手写硬校验 | 足够但可模块化 | 五项继续硬校验 |
| 后端验证 | 按应用格式分支 | ProviderConfig 专用校验 | CrewVela 更聚焦 | 保留，补协议级测试 |
| 持久化 | SQLite transaction + live 文件 | providers.json | JSON 写入抗崩溃弱 | 改为临时文件原子替换 |
| 删除凭据 | Key 在配置中 | provider 与 key 两个调用 | 可能残留 Key | 后端合并删除命令 |
| 编辑 | 同一表单，DB/live SSOT 处理 | 同一 ProviderEditor | 核心一致 | 补 presetId 和恢复默认值 |
| 错误处理 | toast、soft confirm、两阶段提示 | 编辑器错误文本 | 缺连接测试分类 | 增加可操作错误 |
| 测试 | UI、mutation、后端较多，仍无真正全链路 | 后端配置测试为主 | 前端和命令链路不足 | 优先补测试 |
| 版本漂移 | 预设不断更新 | 当前模型/端点与其不同 | 直接复制会破坏兼容 | 引入 schemaVersion 和迁移策略 |

## 16. 应复刻、需适配和不应照搬

### 16.1 应复刻

1. 类型化的单一预设注册表
   每个预设集中声明 label、description、baseUrl、authTokenRef、模型映射、官网、申请 Key 链接、可选端点、schemaVersion。UI 和保存逻辑都从注册表读取，避免散落常量。

2. 选择预设后整份配置一次性应用
   CrewVela 已有该能力，应保留，并增加显式 presetId。新增时应用默认值，编辑时不自动覆盖用户配置。

3. 新增和编辑复用同一表单
   CrewVela 已实现，继续沿用。编辑模式不重复展示预设选择器。

4. 前端和后端双层校验
   前端负责即时提示，Rust 负责最终可信校验。Endpoint 和 Key 对五个固定服务商继续硬校验。

5. 官网和 API Key 帮助入口
   这是 CC Switch 预设结构中最适合直接借鉴、且能立刻降低用户配置成本的部分。

6. 测试思路
   为五个预设逐一测试 Endpoint、模型映射和环境变量；增加 ProviderEditor 交互测试，以及 save_provider 的 keyring 回滚故障测试。

### 16.2 需要按 CrewVela 适配

1. Endpoint 候选和连接测试
   只做五个服务商的 Claude-compatible 探测，不复制 Codex、Gemini 等协议测速。

2. 模型映射
   以 CrewVela 实际 Runtime 支持为准，不把 CC Switch 最新预设值直接覆盖现有值。新默认值和旧配置迁移必须分开。

3. 启用状态
   CrewVela 应表达“供会话选择”或“默认服务商”，不能照搬 CC Switch 的全局 current/live takeover。

4. 持久化
   保留 providers.json 和系统 keyring，但把 JSON 写入做成原子替换，并把删除 provider 与删除 secret 放到一个后端命令中协调。

5. 软校验
   只对网络探测失败提供“仍然保存”；Endpoint、Key、ID 重复等配置错误继续禁止保存。

### 16.3 不应照搬

- Claude、Codex、Gemini、OpenCode、OpenClaw、Hermes 等应用切换
- 统一供应商和跨应用配置派生
- 代理接管、failover、live 配置备份恢复
- OAuth 和受管账号体系
- 五十多个预设、搜索、A-Z 排序和复杂分类
- 商业合作伙伴、星形和心形赞助徽标
- 所谓“预设收藏”，因为 CC Switch 实际没有该功能
- Deep Link、SQL 备份和 live 配置导入
- 累加模式 providerKey
- 普通 API Key 随配置 JSON 存入 SQLite
- 按多应用解析 JSON/TOML 的庞大转换层

## 17. 建议实施顺序

P0：

1. 扩展 CrewVela ProviderPreset，加入 presetId、websiteUrl、apiKeyUrl、schemaVersion 和结构化模型映射。
2. 移除按 id/name 猜测预设类型的逻辑，已有记录通过一次性兼容迁移补 presetId。
3. ProviderEditor 显示官网和 API Key 申请入口，新增和编辑继续复用同一表单。
4. 为五个预设补前端单元测试和 Rust 配置测试。
5. providers.json 改为临时文件原子替换；新增统一的 provider+secret 删除命令。

P1：

1. 增加连接测试，区分 DNS、HTTP、鉴权、协议和模型错误。
2. 国内/国际确有不同端点时增加简单候选选择，不引入通用端点管理器。
3. 把常用模型映射从原始 env 编辑提升为结构化字段，env 保留在高级设置。

P2：

1. 从已有 Claude 配置导入服务商，作为独立功能评估。
2. 备注、端点测速历史、模型在线发现等能力按真实用户需求再加入。

## 18. 最小目标形态

对 CrewVela 来说，重构后的理想流程应是：

1. 用户点击“新建模型服务”。
2. 只看到 Kimi、GLM、DeepSeek、MiniMax、自定义五张卡。
3. 选择后自动填好服务商名称、Endpoint、推荐模型映射和环境变量。
4. 页面同时给出“官网”和“获取 API Key”。
5. 用户通常只需粘贴 API Key；必要时展开高级设置。
6. 前端即时校验，连接测试可选。
7. Rust 再次校验，将 Key 写入系统凭据库，将非敏感配置原子写入 providers.json。
8. 新增和编辑共用一套表单；保存失败时准确说明失败阶段，并避免留下半成品凭据或截断配置文件。

这保留了 CC Switch 最成熟的“预设驱动表单”思路，同时符合 CrewVela 只支持五个服务商、以会话为核心、凭据本地安全保存的实际边界。

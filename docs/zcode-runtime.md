# ZCode Desktop Runtime

CrewVela 调用 ZCode Desktop 现有 Agent，通过原生 `app-server --stdio` v4 协议提交任务、接收结果和处理审批。ZCode Desktop 不是 ACP Runtime，本接入不重新实现模型调用、工具执行或 Agent 循环。

## 启动入口

无需另装 ZCode Desktop CLI。Windows 下会自动读取 ZCode Desktop 安装信息，支持自定义安装目录，不需要设置环境变量。也可用以下环境变量显式指定入口：

- `CREWVELA_ZCODE_ROOT`：ZCode Desktop 安装根目录；或已经构建的 ZCode Desktop 源码根目录。
- `CREWVELA_ZCODE_EXECUTABLE`：现有 Agent 可执行文件或预构建 `zcode.cjs` 的绝对路径，优先于 ROOT。

未配置时先查找 PATH 中的 `zcode`，再读取当前用户和系统的 Windows 卸载注册表；仅使用同时包含 `ZCode.exe` 和 `resources/glm/zcode.cjs` 的有效安装目录。显式配置仍优先，配置无效时明确报错。手动指定示例：

```powershell
$env:CREWVELA_ZCODE_ROOT = 'D:\zcode'
# 在同一 PowerShell 中启动 CrewVela，让它继承此变量。
```

桌面入口解析为 `resources/glm/zcode.cjs`，通过安装目录中的 `ZCode.exe` 以 Electron Node 模式启动，自动提供随安装包附带的内置 provider 配置。启动时明确 Personal 与 Built-in 配置路径，避免安装版 CLI 自动切换版本缓存目录、造成账号权益 overlay 的 Built-in revision 不匹配。源码入口为 `apps/zcode-cli/packages/cli/dist/zcode.cjs`，需要已有构建产物和可用 Node.js。接入不会自动安装依赖或构建 ZCode Desktop，也不会自动升级桌面程序。

## 模型与团队配置

独立聊天：点击工作区或项目下的「新建会话」，选择 **ZCode Desktop**。项目入口沿用项目目录；全局入口选择目录。首次打开自动连接，后续停止后点击「恢复会话」。回答、思考和工具状态按原生 v4 行实时展示；审批可「允许一次」或「拒绝」，「取消本轮」保留部分输出。右侧项目文件使用同一会话目录。

独立会话使用 ZCode Desktop 本地模型和 build 模式；输入区选择模型后才可发送，不自动选择目录第一项。模型目录与选择校验复用 ZCode Desktop 原生协议。聊天定位与已显示行保存在 CrewVela 配置目录的 `zcode-sessions.json`；原生历史仍由 ZCode Desktop 管理。有消息的会话按原生 sessionId 恢复，恢复失败明确报错。上游会丢弃没有消息的空会话，因此仅在本地历史确认为空且上游返回 not-found 时重新建立空会话，不迁移或丢弃已有历史。

输入区「＋」提供附件和已安装插件、技能、能力选择；输入 `$`、`/`、`@` 搜索相应目录，方向键选择，Tab 补全。所选引用显示简洁高亮标签，提交时按原生 Markdown canonical 携带稳定插件 ID / 技能绝对路径；不是把菜单标签当作普通聊天发送。技能按原生 workspace > plugin > user 优先级去重，禁用或身份冲突的插件不可选。目录绑定当前 resident session；错误明确显示，空目录不补造截图中的项目。原生 App 消费的 compact/plan/goal/side/btw 尚未接入，本轮不把它们作为可直接发送的能力显示。工作流创建入口仅在原生目录广告 `/workflow` 时显示；不提供完整工作流面板或插件安装管理。

工作流沿用原生 Host 灰度契约：新建/恢复前读取公开 `/api/v1/client/configs`（安装版版本和平台参数，无 Authorization、无凭证读取），解析 `data.configs.dynamicWorkflow.mode` 的 disabled/onDemand/alwaysOn，向该 Agent 进程同步 `workspace/updateDynamicWorkflowPolicy` 并核验 ACK，同时传给新建/恢复的 session flag。每个连接判定一次；不使用开发环境变量强制开放。公开配置失败按关闭处理并显示独立提示，普通聊天继续。政策 ACK 不确定时重新同步 false；仍无法确认则结束该连接，避免 UI 关闭而工具仍开启。2026-10-04 对当前安装版隔离存储/工作区的只读探测取得 211 个技能目录项、14 个插件目录项、5 个原生命令，公开配置允许工作流，`/workflow` 目录与已确认政策一致；未调用模型或执行工作流。

附件通过原生 `AttachmentRef` 本地绝对路径传递，发送前重新验证存在、普通文件、数量和大小，支持无正文附件发送，并保留附件历史展示。CrewVela 当前保守限制每次 8 个、每个 20 MiB（不是所有原生本地路径的通用上限）；模型对图片、PDF、视频的支持和原生读取降级继续由 ZCode Desktop 决定。发送未被原生 userInput 行确认且出错/断连时恢复正文、引用和附件；已确认输入后的模型失败不自动重发。原生插件需要的 MCP/官方鉴权、环境依赖仍有接入限制，目录可选不等于所有插件端到端可用。UI preview 是显式模拟目录，不连接原生或调用模型。

输入框支持直接粘贴与拖放添加附件，不必绕道文件选择器：粘贴带文件（截图、资源管理器复制的文件、浏览器图片）时优先由后端直读系统剪贴板——CF_HDROP 文件直引原路径、位图零拷贝落系统临时目录 PNG；两条都拿不到时把 WebView 给出的 File 字节写到临时目录兜底（文件名净化为 basename 并加 `crewvela-clip-` 前缀）。纯文本粘贴不拦截、走原生插入。临时粘贴文件的命名与终端通路共用同一约定。

本轮回归：`node --test tests/zcode-composer.test.cjs tests/zcode-workflow-policy.test.cjs tests/zcodePaste.test.cjs`；`cargo test zcode --lib --no-default-features`、`cargo test clipboard_image --lib`。只读安装版目录探测需要精确运行 `cargo test zcode_installed_composer_catalog_without_model_call --lib --no-default-features -- --ignored --nocapture`，不要泛跑 ignored：其他 smoke 测试会实际调用模型。

新建空会话使用原生 deferred 持久化，由 v4 首次输入准入建立持久化记录；legacy `session/create` 标记 immediate 却没有实际落库时，安装版首发会触发 SQLite 外键错误。恢复时若原生响应只提供当前模型，先创建并关闭一个无输入、deferred 的临时目录草稿，读取原生完整模型目录，不发送消息、不写入聊天历史。

在协作中心的团队成员编辑器中选择 Runtime **ZCode Desktop**。模型填写 ZCode Desktop 中的实际 `providerId/modelId`，留空使用 Agent 本地默认模型。

支持 ZCode Desktop 配置的 Personal 自定义 API 模型；独立聊天还支持当前安装版的 Individual Coding Plan 和 Start Plan 账号模型。协作任务本期仍使用 Personal 模型。Agent 沿用其本地 provider 配置，默认位置是用户目录的 `.zcode/v2/provider_config.json`；已有 `ZCODE_DATA_BASE_DIR` 或 `ZCODE_PERSONAL_PROVIDER_CONFIG_FILE` 覆盖会继承给 Agent。API 密钥仍由 ZCode Desktop 本地配置持有，不需要填写到 CrewVela 的模型字段。

ZCode Desktop 账号模型依赖桌面 Host 动态鉴权。CrewVela 从 ZCode Desktop 本地登录状态读取当前账号，查询原生账号权益后同步模型目录；模型请求校验当前会话、所选模型与账号身份，凭据仅在后端私有通信中使用，不写入 CrewVela 的聊天定位、模型列表或日志。账号登录与凭据更新仍在 ZCode Desktop 中完成；未登录、缓存缺失、身份变化或权益不可用会明确提示。登录或切换账号后，停止并恢复 CrewVela 中的 ZCode Desktop 会话以刷新目录与身份。Team 和 off-peak 模型暂不支持。

官方 MCP 鉴权是工作区级请求，可能在创建会话返回前到达，不携带 sessionId。CrewVela 按原生协议回复 `official_auth_unavailable`，具体 MCP 状态由 ZCode Desktop 按传输类型处理；此拒绝不会终止普通 Personal API 会话，也不会提供账号凭据或尝试匿名连接需要鉴权的官方 HTTP MCP。依赖官方鉴权的能力仍不可用，stdio MCP 可保留连接并向插件传递鉴权失败信息。

## 已实现的行为与边界

- 提交任务并等待匹配当前命令的真实终态；收到 accepted ACK 不算完成，历史已完成轮次不能作为本次结果。
- 支持原生完整帧和 base64/CRC32 分片，校验订阅、会话、水位和 epoch；异常帧拒绝推断成功。
- 恢复沿用 ZCode Desktop 会话，在激活前刷新本次 CrewVela MCP 令牌；恢复信息中的目录、权限和模型必须保持一致，损坏信息会报错。失败、取消或 CrewVela 中断后的公共检查点允许在原目录新建 Agent 会话重试。
- 默认权限使用 ZCode Desktop build 模式，权限请求交给 CrewVela 审批，只批准本次调用；重复业务请求复用同一次决定。取消或审批超时会撤销待处理审批。
- auto 权限仅允许已有隔离工作区；yolo 使用上游 yolo 模式。这里没有操作系统沙箱保证，强制只读任务会拒绝执行。
- 创建期运行偏好请求按上游明确支持的无 Host 兼容路径处理：Memory 默认关闭、增强搜索默认开启；不会因此授予工具权限。
- 取消先向 Agent 提交 stop，再清理进程；Windows 使用既有进程树清理实现。未支持的交互会明确失败。
- 同时支持独立原生聊天与协作任务 Runtime，不提供独立 PTY 终端。ZCode Desktop 升级仍通过其原安装方式处理；活跃独立连接持有 Runtime lease，阻止运行中禁用或升级。

## 验证

### 独立聊天的消息操作

- 用户消息提供复制；编辑入口仅接受原生投影 `actions.canEdit=true`、`editDisposition=rewind` 和稳定 `entityId`。保存走 `editUserQuery`，携带 `rowId/entityId`、当前 `baseRevision/baseLogEpoch`，由原生运行时重建对话。默认 `workspaceMode=preserve`，保留当前文件；不提供文件重置。
- 在助手正文中选中文本，可添加到主任务的引用草稿，或打开同进程辅助对话并添加引用。点击操作不会自动发送；发送时按原生 `# userselect:` JSON 格式进入模型输入。引用最多 8 条、单条 8000 字符、总计 16000 字符，超限报错，支持移除。
- 生成期间输入框显示“停止生成”，调用原生 `stop`，保留进程及部分回复。按钮等待权威执行状态结束后恢复；拒绝、超时和断连错误保持可见。排队发送仍可使用。
- 预览已有会话 URL 添加 `zcode-messages=1` 展示用户/助手示例，添加 `zcode-output=streaming` 检查停止入口。预览不验证原生调用。

无密钥协议回归：

```powershell
Set-Location .\src-tauri
rtk proxy cargo test zcode --lib
```

实际桌面 Agent 的无模型控制面验证（测试使用临时 storage/provider 目录）：

```powershell
Remove-Item Env:CREWVELA_ZCODE_ROOT,Env:CREWVELA_ZCODE_EXECUTABLE -ErrorAction SilentlyContinue
rtk proxy cargo test zcode_installed_control_surface_without_model_call --lib -- --ignored
```

该测试覆盖生产 v4 创建、初始快照解析和关闭；另通过上游无模型历史导入建立持久化测试会话，验证跨进程恢复。`zcode_desktop_empty_session_persists_without_model` 验证独立连接的空会话重建路径。账号目录和绑定验证可单独运行 `rtk proxy cargo test zcode_installed_account_catalog_and_binding --lib -- --ignored --nocapture`。不要使用 `--include-ignored` 泛跑：`zcode_installed_account_model_smoke` 会读取当前登录并实际调用账号模型，必须明确授权后精确运行。

协议回归覆盖审批去重、拒绝、错误会话、账号鉴权拒绝、恢复 MCP 刷新、ACK 不等于完成、取消和分片校验。独立管理器回归覆盖多轮发送、审批一次、并发发送拒绝、取消/停止清理、账号拒绝保留部分内容、存储失败不发布修改和跨进程定位恢复。2026-10-04 已在当前安装版和有效 Individual 账号上验证 6 个可用模型目录、模型绑定，以及隔离工作目录下一轮真实模型回复与成功终态。该调用工具全部拒绝、未挂载 CrewVela MCP；不能代表长连接套餐变化、真实账号切换恢复或完整原生窗口验收。真实自定义 API 模型执行、长时间人工审批及实际 Bash 子进程取消尚未做端到端验证；UI preview 是模拟数据，不验证 Tauri invoke 路径。

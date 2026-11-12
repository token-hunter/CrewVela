# CrewVela 开发与使用手册

> 多Agent协作工作台

CrewVela 将多种 Agent 会话、工具调用、项目协作与自动化集中到一个桌面应用中。基于 Tauri 2，沿用各 Agent 的原生能力和权限机制；第三方 Provider 的 API key 存入系统 keychain。

灵感来自 Reddit r/ClaudeCode《How to Set Up Claude Code with Multiple AI Models》（ThreeKiloZero），将其 shell 函数方案 GUI 化、安全化、跨平台化。

## 功能

- 预置 **GLM / Kimi / DeepSeek / MiniMax** 和自定义模板；使用前配置凭据并安装对应 Agent
- 支持 Claude Code、Codex、Kimi、OpenCode、Pi、ZCode 与 DeepSeek Harness 原生聊天及多种 CLI 会话；可用范围取决于本机安装和登录状态
- 自定义 provider：填 `base_url` + 模型映射 + key
- API key 走**系统 keychain**（Win 凭据管理器 / macOS Keychain / Linux Secret Service）
- 内嵌终端（xterm.js + portable-pty），直接与 claude 交互
- 持久化会话列表：显示执行状态、自动标题，支持切换、重命名、停止和删除
- 重启后可通过 Claude Code session ID 恢复历史对话
- 启动第三方 provider 时自动 `unset ANTHROPIC_API_KEY`，避免 key + 订阅冲突警告
- 启动时可选**初始任务自动派发**：会话就绪后自动把任务写进终端
- 可选**会话级 git worktree 隔离**：每个会话基于仓库当前 HEAD 落在独立 worktree 分支，并行改动互不影响；删除会话时自动清理（有未提交改动则保留并提示）
- **会话间消息**：会话内的 agent 可用 `CrewVela send --to <会话id前缀> --body "内容"` 给其他会话发消息（`CrewVela sessions` 列出目标），消息在对方就绪时自动投递并附带来源与回复指引

## 前置依赖

| 依赖 | 说明 |
|---|---|
| Rust 工具链 | `rustup` 安装 stable |
| MSVC C++ 构建工具 | Windows 必需（VS Installer 的 "Desktop development with C++" 工作负载）|
| Node.js 24 | 开发前端与运行包含 TypeScript 的测试（本轮使用 24.12.0 验证） |
| Claude Code CLI | `claude` 需在 PATH |
| WebView2 | Windows 11 自带 |

Linux 构建还需要系统 D-Bus 开发库（`dbus-1` / `pkg-config`）及 Tauri 的 GTK/WebKit 依赖。凭据持久化需要会话 D-Bus 和 Secret Service 提供者（如 GNOME Keyring 或 KWallet）；服务不可用时会明确报错。本轮只验证了 Windows 构建及 Linux 依赖选择，尚未完成 Linux 原生运行验收。

## 安装与运行

### 1. 安装 Rust（Windows）

PowerShell（管理员）：

```powershell
winget install Rustlang.Rustup
```

或下载 [rustup.rs](https://rustup.rs) 运行 `rustup-init.exe`。

还需 MSVC 构建工具：

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

### 2. 安装依赖

```powershell
cd CrewVela
npm ci
```

### 3. 开发模式

```powershell
npm run tauri dev
```

首次会编译 Rust 后端（数分钟），之后增量编译很快。前端热更新在 `http://localhost:1420`。

### 首次启用共享记忆（Windows x64 / macOS）

先安装需要接入的 Claude Code / Codex CLI，并确保应用能检测到它们。启动 CrewVela 后打开 **设置 → 共享记忆 → 概览 → 开始配置**，选择项目与智能体，点击 **安装并启用**。向导会安装兼容版本、初始化本地数据、启动服务，并合并 MCP 与钩子配置；默认无需模型 API Key。

- 动态读取 [官方最新稳定发行版](https://github.com/akitaonrails/ai-memory/releases)：Windows x64 使用 ZIP，Apple Silicon / Intel Mac 使用对应 TAR.GZ。下载或导入完整官方包时均校验 SHA256、大小和架构，无需 Homebrew、sudo 或编译 ai-memory。已有兼容安装可复用；“检查更新 → 安装更新”仅更新 CrewVela 托管版本，先备份数据与接入配置，跨大版本需确认。当前适配 2.6+ 的 2.x / 3.x；未适配大版本停止自动安装。具体备份、认证、失败恢复和 Mac 验收见 [快速启用说明](shared-memory-quick-setup.md)。
- 记忆与安装文件分别存于本机应用配置目录的 `ai-memory-data` / `ai-memory`，不从仓库分发个人记忆。向导使用 allowlist，为所选项目创建 `.ai-memory.toml`；其他已有标记项目也遵循 ai-memory 的 allowlist 策略。
- Claude Code 钩子为所选项目级配置；Claude MCP、Codex MCP 和 Codex 钩子为用户级配置。接入前备份原配置；重复执行复用兼容安装并保留记忆。不同版本、自定义路由、受限桥接或冲突策略会明确停止，请改用“接入配置”手动核对。
- 完成后，在所选项目新建或重新加载智能体会话，确认记忆工具和钩子信任提示，再完成一次会话并使用“接入检查”。**服务与配置通过不代表真实会话捕获已通过**；向导不会自动重启现有会话。Codex 的原生 hooks 若被显式禁用，需先手动核对设置。

当前快速启用的源码支持 Windows x64、macOS Apple Silicon / Intel 的 Claude Code 和 Codex；Mac 原生完整首次启用待实机验收，旧 Release 附件不会自动更新。其他平台及智能体继续使用接入配置中的手动方式。失败进度会保留，可重试；下载、校验或配置错误不会显示为完成。修改中的外部配置存在并发竞争时，向导会尽量保留外部更改并提示备份位置，避免自动覆盖恢复。

### 4. 打包

```powershell
npm run tauri -- build --no-bundle
```

Windows 发布程序为 `src-tauri/target/release/CrewVela.exe`。Tauri 构建会统一使用 CrewVela 文件名；内部配置与数据目录保持兼容。

此命令先构建前端，再启用 `custom-protocol` 将资源内嵌进程序，启动时无需 Vite/localhost 开发服务器。直接使用 Cargo 时须运行 `cargo build --release --features custom-protocol`；缺少该协议的 release 构建会明确失败。前端构建后，可运行 `npm run test:desktop-assets` 检查生产协议与内嵌入口资源。

### 新用户隔离测试

需要在已有 CrewVela 的电脑上验证首次打开效果时，Windows 构建后双击根目录的 `start-first-run-test.cmd`，或执行：

```powershell
powershell -NoProfile -File .\scripts\start-isolated-test.ps1
```

macOS 14+ 在 Mac 执行 `npm run build:macos` 后，双击根目录的 `start-first-run-test.command`，也可在终端运行 `bash ./start-first-run-test.command`。启动器会定位副本构建的 `.app`，直接传入隔离参数；不通过 `open -a` 复用已打开的正常实例。自定义应用或测试目录示例：

```sh
bash ./start-first-run-test.command --app "/path/to/CrewVela.app" --data-dir "$PWD/.test-data/mac-first-run-2"
```

测试配置和会话保存在副本根目录的 `.test-data/config/`，被 Git 忽略。Windows 的 WebView 缓存在 `.test-data/webview/`；macOS 使用测试目录派生的专属 WKWebView 存储身份，网页缓存由系统管理，不保证位于该文件夹。相同测试目录重开沿用同一存储身份，更换全新的目录则使用新身份。macOS 14 以下或无法确认系统版本时拒绝进入隔离模式，不回退正式存储；该限制仅针对隔离测试模式。

窗口顶部会显示“新用户隔离测试”。此模式不迁移旧历史、不读写系统凭据、不导入本机 Agent 账号，也不更新全局 Skill；真实 Agent、账号、共享记忆和全局集成操作会被后端拒绝。测试输入的 Key 仅在进程内存中保留，退出后清除。关闭测试窗口即退出。

测试目录会保留本次测试设置；若需再次验证首次打开，可以指定一个新的空目录：

```powershell
powershell -NoProfile -File .\scripts\start-isolated-test.ps1 -DataDirectory "$PWD\.test-data\another-first-run"
```

也可直接运行 `CrewVela.exe --test-data-dir <测试目录绝对路径>`。程序拒绝正式数据目录、符号链接和没有测试标记的非空目录，不会自动清空任何目录。无测试参数启动 EXE 时仍按正常方式加载本机数据；真实账号与模型功能应在正常模式验证。

macOS 专项构建和验收见 [Mac 隔离测试步骤](macos-isolated-test.md)。已合入 Mac 回传的原生隔离修复及 v2 普通启动缓存冲突修复；报告记录构建副本进入主页并正常退出，安装副本只完成启动观察。结果来源、未完成项和 Windows 回归见 [Mac 回传验证记录](macos-merge-validation.md)。真实 Agent 功能、普通/隔离并存和系统级 Keychain 访问仍未验收。

### Apple 芯片 macOS（第一阶段适配）

已增加 Zsh/Bash 终端、Finder 启动时的 CLI 路径发现和 POSIX Agent 任务执行路径，Windows 继续使用既有实现。macOS ARM64 构建命令为 `npm run build:macos`，需要在 Mac 上执行。用户回传报告记录 ARM64 打包及独立首次启动隔离验收通过；真实 Agent/模型等完整功能仍待逐项验收，不能把 Windows 测试或浏览器模拟视为 Mac 原生验收。环境准备、能力限制和验收步骤见 [macOS 核心适配说明](macos-core.md)。

## 使用

1. 在设置的模型服务中配置 Provider 与 API key；内置配置支持编辑、隐藏与恢复默认，自定义配置支持删除。
2. 打开工作区，在新建入口选择已安装并启用的 Agent，创建独立聊天或终端会话。
3. 使用第三方模型的 Claude Code 终端时，选择对应 Provider。原生聊天使用相应 Agent 的登录及配置。
4. 从左侧切换已有会话；停止保留历史，恢复能力取决于该 Agent 的协议和会话记录。

### 让 Agent 互相通信

首次打开 CrewVela 就会显示会话通信轻提示，无需先启动 Agent。关闭提示或查看教程后会记住已读状态，不再重复弹出。点击左侧 **会话通信**（侧栏收起时使用顶部同名按钮），仍可随时查看三步教程并复制当前对象的示例。

- **独立会话**：在 CrewVela 中打开两个受支持的 Agent 会话，在发送方输入类似“请先核对当前已打开的会话，给「代码审查员」发一句问候，只联系这个会话。”的指令。指南会读取运行中的会话并生成对应示例。
- **项目成员**：在多智能体工作台或协作中心点击 **会话通信**，选择同一项目的两位已启用成员；同一种 Agent 的不同成员也可互相联系。先启动成员任务或进入成员会话，再粘贴示例。
- **复制只写入剪贴板，不发送消息**。对方忙碌时可能排队；已排队不代表已回复。旧会话可能需要重新打开以加载通信工具。遇到问题，指南中的 **打开通信诊断** 会进入相应项目或独立会话的全局诊断。
- **升级时先退出旧版**：同一数据目录只应运行一个桌面主程序，否则可能争抢就绪事件和消息队列。新版会阻止重复启动；升级前请从旧窗口或托盘菜单选择“退出”，再启动新版。仅关闭窗口可能仍在托盘后台运行。

### 多开会话（网格分屏）

点 provider 后输入数量（1-9）→ 在目录面板为每个会话选工作目录（留空则用 home；点「全部使用同一目录」可一键填满所有行）→ 主区按 `⌈√N⌉ × ⌈N/⌈√N⌉⌉` 网格同时开 N 个 claude（同 provider，各会话可不同项目目录，典型场景：并行处理多个项目）。
每格独立交互；点格子右上角 `×` 关单个（其余位置不动），顶栏「全部关闭」清空所有会话。

不同 provider 的会话可混合并存：开 glm 后再点 deepseek 多开，glm 的格子保留、deepseek 新格子追加，累加不互斥。

### 会话管理

- 左侧会话栏显示「未执行 / 正在执行 / 等待确认 / 执行完毕 / 执行失败 / 已停止」状态。
- 点击「全部运行会话」查看网格；点击具体会话切换为单会话聚焦视图，其他会话继续在后台运行。
- 首次发送问题后自动根据问题生成标题，也可以手动重命名。
- 停止会话只结束当前进程并保留历史；再次点击会使用 `claude --resume <session-id>` 恢复上下文。
- 删除只移除 CrewVela 的会话索引，不会删除 Claude Code 自己保存的 transcript。

## Multi-Agent 协作工作台

1. 打开顶部 `Multi-Agent`，在“项目协作”中描述项目目标、绝对目录和角色分工，例如：`Claude Code + GLM 负责前端，Codex 负责后端，并互相 Review 后再集成`。
2. 确认协调器生成的 Agent、任务依赖、交叉 Review 和最终集成计划，再启动项目。
3. 项目启动后，主区域会自动打开协作工作台。每个 Agent 都有独立窗口，实时显示运行状态、当前任务、可见输出、工具调用和验收命令；可聚焦任一窗口查看完整内容。
4. 右侧“任务流”展示依赖与阶段，“协作记录”展示任务派发、Agent 消息、工作区交付与内容哈希。界面不会展示模型隐藏推理。
5. “人工接管”会让当前 Run 执行到安全结束，并暂停该 Agent 后续任务；可补充下一次执行指令，再“交还总控”恢复自动调度。其他 Agent 不受影响。
6. 写代码的 Agent 默认使用独立 Git Worktree；Reviewer 读取实现交付，Integrator 汇总各分支并在用户批准后合并回主工作区。

## 架构

```
前端 React+TS+Vite ── xterm.js 终端
        │ Tauri invoke / event
Rust 后端 (Tauri 2)
  providers.rs  provider 配置 (JSON)
  secrets.rs    keychain 存取
  launcher.rs   env 组装、claude 路径探测
  pty.rs        portable-pty 会话 + 字节流桥接
  sessions.rs   会话元数据、状态事件与持久化
        │ spawn (PTY + env 注入)
   claude CLI 进程
```

**数据流**：点 provider → Rust 取配置 + key → 组装 `ANTHROPIC_BASE_URL / AUTH_TOKEN / MODEL / ...` → spawn `claude`（带 PTY，注入 env，unset 旧 key）→ 前端轮询 PTY 增量并回显到 xterm.js。Claude Code Hooks 将 prompt、stop、等待确认等生命周期事件写入本地队列，用于更新会话状态和标题。

## 配置文件位置

- Provider 配置：`%APPDATA%\CrewVela\providers.json`
  （macOS：`~/Library/Application Support/CrewVela/`，Linux：`~/.config/CrewVela/`）
- 会话索引：同目录下的 `sessions.json`
- API key：系统 keychain（service `CrewVela`）

## 敏感配置与公开发布

模型 Provider 的 API key 在应用设置中填写，继续存放于系统凭据库。仓库不需要包含个人账号、密钥、登录状态或用户数据目录的备份。

可选的开发环境配置使用根目录 `.env`：

```powershell
Copy-Item .env.example .env
# 按需编辑 .env，然后让 Node 将变量传给 Tauri 及其 Rust 子进程：
node --env-file=.env --run tauri -- dev
```

首次配置时复制即可，已有 `.env` 请直接编辑，避免覆盖。仅复制文件或执行普通 `npm run tauri dev` 不会使 Rust 自动读取 `.env`。模板中的 `AI_MEMORY_AUTH_TOKEN`、`ZCODE_CREDENTIAL_SECRET` 仅用于对应的可选服务，未使用时保持注释；不要用 `VITE_*` 保存秘密，这些变量会进入前端构建产物。终端环境变量会由子进程继承，只应在可信的本地开发环境使用。

`.gitignore` 排除了 `.env` 及其环境变体、凭据导出、签名私钥、用户状态、构建产物与临时 QA 文件；`.env.example` 只含说明和空配置，可随源码发布。源码、测试夹具、应用图标、锁文件与第三方许可仍需保留。禁止使用 `git add -f` 提交私有文件。

原开发仓库的历史不属于本开源副本；**不要将原开发仓库历史合并或推入公开仓库**。`.gitignore` 和新的清理提交都不会删除旧提交内容；公开版应从清理并验证后的源码建立新的独立 Git 历史。详细范围、扫描结果及发布前检查见 [GitHub 发布审计](github-publication-audit.md)。

## Agent 自动化

打开「设置 → 智能体 → 自动化 → 新建自动化」，按单页表单依次填写名称、执行 Agent、任务和触发条件。

- **默认使用本地配置**：Claude Code、Codex CLI、Kimi Code、Pi、OpenCode、ZCode CLI 可以新建独立 CLI 会话；需要提前安装、登录。仅列出已安装且已启用的类型。
- **模型可选**：新建 Claude Code 会话时可指定 CrewVela Provider；Claude Code 和 Codex 可填模型标识。留空沿用本机默认值，不改写 CLI 全局设置。其他类型暂沿用本地模型。
- **指定目标**：也可复用已打开的同类型会话，或交给项目指定成员。已停止的独立会话需先恢复；未运行的项目成员通过项目调度器执行，沿用成员目录、模型和权限配置。已有交互会话保持当前配置。原生 Chat 可复用现有会话；DeepSeek Harness 与 ZCode Desktop 还支持项目成员方式。
- **广播范围**：选择「发送到此 Agent 的全部运行会话」时，执行时确定目标，包含其他工作区中的同类型会话；排除归档、已停止及普通 Shell 会话。
- **触发与防重叠**：支持一次性、间隔、每天、每周、cron 和指定来源的会话完成／失败事件。新任务默认跳过上一轮未结束时的触发，也可选择继续派发。高级设置可限制触发次数及连续派发失败熔断次数。

列表中的「已派发」表示已启动或进入发送队列，**不等于任务执行完成**；任务结果需到目标会话或项目中核对。目标停用、删除、类型变化、目录不可用或排队失败会记录具体原因。旧自动化保持原有动作和重叠策略，不会在升级时被扩大范围。

## 开源版范围

品牌与通信配置升级、已有本地设置迁移、通信沙箱修复和设置窗口验证见 [升级与回归说明](crewvela-upgrade-validation.md)。环境变量和 MCP 服务名已统一为 CrewVela；旧应用启动的会话需同步通信 Skill 后，在新构建中恢复或重启原会话。

保留多 Agent 原生聊天与 CLI 会话、项目协作、自动化、文件/Git/浏览器/电脑控制、插件市场、Skill 中心与 MCP、共享记忆、通知、桌面宠物和外观皮肤。
语音、赛博女友、Benchmark、创作工作台及工作流画布已从界面、后端和专用资源中移除。旧本地用户数据不会自动删除；旧快捷入口在读取时过滤失效路由。

## 开发验证

```powershell
npm test
npm run build
cd src-tauri
cargo test --lib
```

界面回归需要本机 Chrome 和运行中的 Vite。设置 `CREWVELA_UI_URL` 为实际开发服务器地址后执行 `node tests/uiPolish.ui.mjs`、`node tests/fileExplorerEditing.ui.mjs` 和 `node tests/asyncChatDrafts.ui.mjs`。
这些界面测试使用浏览器预览或 Mock IPC，不替代原生桌面、真实登录/模型及其他操作系统的验收。更多范围与结果见 [开源整理验证记录](opensource-cleanup.md)。

本轮运行时移除、自动化调度与界面可靠性修复的验证记录见 [项目质量审查](quality-hermes-removal.md)。

## 安全

- CrewVela 管理的 Provider API key 存入系统 keychain；第三方 Agent 的登录文件由其自身管理。
- 请求使用所选 Provider 的端点与配置；请确认该服务与所用 Agent 兼容。性能取决于模型服务和网络。

## 故障排查

- **未找到 claude**：确认 `claude` 在 PATH（`where claude` / `which claude`）。
- **spawn 失败**：检查 Agent 路径、Node 和安装完整性。Windows 启动器识别受支持的 CLI 包装器；无法安全解析的自定义脚本会明确报错。
- **仍报 key+订阅冲突**：本应用已自动 unset `ANTHROPIC_API_KEY`；若系统环境变量残留，手动清除。
- **Rust 编译报缺 MSVC**：安装 Visual Studio Build Tools 的 C++ 工作负载。
- **终端输出异常**：先在系统终端确认同一 Agent 能正常启动，再检查应用显示的启动错误与运行日志。

## License

MIT

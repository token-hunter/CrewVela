# 共享记忆快速启用

源码支持 Windows x64、macOS Apple Silicon / Intel，接入 Claude Code 和 Codex。打开 **设置 → 共享记忆 → 开始配置**，选择项目与智能体，点击 **安装并启用**。用户仍需先安装需要接入的 Agent CLI；快速启用安装的是记忆服务。macOS 原生完整首次启用仍待实机验收；旧 Release 附件不会自动包含此更新。

## 行为与保护

- 从官方 GitHub Releases 动态获取最新稳定版，跳过草稿和预发布版本。按应用目标平台与架构选择 ZIP 或 TAR.GZ；版本、大小和 SHA256 来自官方发行包元数据，不再把某个发行版本写死为安装默认值。下载和本地导入均校验完整包，使用暂存目录检查版本、模板及必要命令后才发布。
- 安装与记忆数据目录分开。进度记录、配置备份和个人数据保存在本机应用配置目录，仓库不包含个人记忆或 Agent 凭据。
- 下载请求要求新的发行包重定向，避免缓存过期的 GitHub 资产地址；网络错误保留为失败，可重试或导入对应平台的本地官方安装包。
- 仅接入选择的 Agent，生成所选项目标记，使用 allowlist。其他已标记项目仍遵循原生 allowlist 策略，不把它描述为仅允许唯一目录。
- 合并保留其他 MCP 和配置。已有兼容版本可直接复用；外部服务、自定义路由、受限项目桥、损坏配置、显式禁用 hooks 或冲突捕获策略会停止自动操作。已有项目标记不覆盖。
- 每个 Agent 写入前保存备份。部分失败恢复该 Agent 已修改的配置；如发现配置已被其他程序修改则保留外部更改，提供备份位置。原生安装器写入与外部程序之间仍存在很窄的文件替换竞争，不能视为跨进程事务。
- 不自动重启现有会话。配置与服务工具通过后标记“配置完成，待新会话验证”，真实新会话捕获需人工确认。

## 检查版本与显式更新

1. 点击 **检查更新**，显示当前版本、官方最新稳定版和安装来源。没有新版本时不显示更新按钮；不会定时静默替换正在运行的服务。
2. 已由 CrewVela 托管安装且发现新版时，选择项目和 Agent，暂停依赖记忆的会话，再点击 **安装更新**。跨大版本另需确认迁移及恢复范围。
3. 先下载、校验和检查候选程序，再停止本次应用持有的记忆服务。备份整个数据目录（包含数据库、Wiki、原始记录、配置和认证文件）、旧程序以及所选 Agent 的接入配置，然后启动新版并进行迁移。
4. 验证新版健康、认证、真实 MCP 工具列表与项目捕获范围。配置完成后，仍需重新加载 Agent 会话验证真实捕获。

当前适配范围为 **2.6.0 及以上的 2.x / 3.x 稳定版**，并对必要命令、服务及工具做运行检查；这不等于所有未来小版本已经实测。遇到新的未适配大版本或缺失平台包、官方校验值，停止安装并说明原因。GitHub 离线或限流时，现有兼容安装仍可继续配置；全新安装与更新需要联网取得官方校验信息，本地包导入也不绕过校验。

只自动更新 CrewVela 托管目录。Homebrew、Scoop、PATH 或自定义安装使用原方式更新。上次应用启动而本次未持有进程句柄的服务也按外部进程处理，需用户手动停止后再更新；不会根据 PID 文件杀进程。

完整快照保存在本机应用配置目录的 `ai-memory-upgrade-backups/<随机标识>/`。更新失败时尝试恢复旧程序、升级前数据及此次改动的接入配置；候选运行期间的数据另行保留，不自动合并。外部程序并发改动过的配置不会覆盖。恢复未完成或应用中断时保留恢复记录，阻止继续启动可能已经迁移的数据；重新打开向导后确认 **恢复升级前快照**。磁盘不足、文件被占用或外部配置冲突仍可能需要人工处理，错误与备份位置会显示。备份含认证文件，应按私人数据保管；当前不自动清理历史快照。

3.x 全新数据目录启用本地认证时，CrewVela 从该数据目录读取访问令牌，只发给匹配的本地服务地址；不会经前端 IPC 返回令牌。用户显式设置的 `AI_MEMORY_AUTH_TOKEN` 仍按已有配置生效。所选 Agent 的原生安装命令会把必要认证写入其本机接入配置，原有审批设置保留。升级已有 2.x 数据时沿用上游的认证迁移规则，不强制改变原认证策略。

参考：[官方发行版本](https://github.com/akitaonrails/ai-memory/releases)、[官方 3.0 升级说明](https://github.com/akitaonrails/ai-memory/blob/v3.4.5/docs/upgrading-to-3.0.md)。

## 当前验证（2026-11-11）

Windows 主机：50 项共享记忆 Rust、8 项隔离命令守卫通过；444 项 Node 通过、1 项既有跳过，TypeScript/Vite 和三套浏览器界面回归通过。额外显式运行的官方在线最新版本安装、3.4.5 首次/重复启用、2.6→3.4.5 迁移及故障回退/重试、两种 Mac 3.4.5 完整包校验/提取均通过。两个本机真实服务 opt-in 检查未运行。原有 Rust 警告和前端分块提示保留。

测试使用独立临时配置与数据，未升级真实用户服务。浏览器界面通过模拟 IPC 验证；Mac 原生运行和真实 Agent 捕获仍待验收。未提交、推送或更新安装附件。完整记录见 [任务 60](../issues/60-ai-memory-release-updates.md)。

## 历史验证（2026-11-07，v2.6.0）

| 检查 | 实际结果 |
| --- | --- |
| 官方包在线安装 | 真实下载、SHA256、解压、版本检查通过；调整请求缓存策略后在线测试约 9 秒 |
| 本地官方 ZIP 与新用户隔离目录 | 首次安装、启动、两种 Agent 配置、真实 MCP 工具探测通过 |
| 重复与边界 | 保留既有模型、其他 MCP、项目标记和数据；占用默认端口时选择独立端口；未标记目录拒绝捕获 |
| 错误保护 | 越界路径、符号链接 ZIP、错误校验和、旧捕获策略、损坏配置、部分失败回退和外部并发修改测试通过 |
| Rust / CLI | 库 1016 通过、38 忽略；CLI 1/1。38 个忽略项中，本轮在线安装及真实发行包隔离验收已分别显式运行通过，其余忽略项未作为本轮通过项 |
| Node | 393 通过、1 跳过 |
| 浏览器界面 | 向导、原共享记忆、修复界面测试通过；深浅色与 600px 宽度、失败重试、页签切换、进度恢复、卸载清理通过 |
| 设置布局 | 28 组窗口/主题/数据组合，420 次菜单切换通过 |
| 构建与源码检查 | TypeScript/Vite、Windows release、差异检查通过；候选源码 Gitleaks 无发现，扫描正反对照通过 |

网络的首次普通请求出现卡顿，未作为通过结果；调整下载请求后重新通过在线安装。界面测试使用明确标记的模拟服务；安装与服务测试使用官方二进制、完全隔离的配置和数据目录，不改动真实用户的 Agent 配置。

仍未验收：新构建在原生 Tauri 界面中的完整首次启用操作、真实模型会话的工具加载/信任/钩子捕获、macOS/Linux/Windows ARM 和其他 Agent 的快速启用。原有 Rust 警告、前端大分块提示保留，没有降低检查或新增忽略规则。演示页提示文字对比度已修正。

## 发布启动修正

上轮交付虽然完成了 release 编译，但遗漏 Tauri `custom-protocol`，关闭 Vite 后仍访问开发服务器。已增加 `npm run build:desktop` 生产构建命令和 release 防护，补充内嵌入口及 JS/CSS 检查，并替换原交付文件。2026-11-07 已实际验证：1420 端口无监听时，Windows 原生主界面、设置和共享记忆入口正常加载。此验证不等同于原生完整首次安装或真实会话捕获验收。详情见 [独立启动修复记录](../issues/16-desktop-offline-startup.md)。

## macOS 安装支持（2026-11-11）

以下是源码新增能力，不表示旧 DMG 已更新或 Mac 原生验收已完成。

| 平台 | 官方包名（版本与校验值动态发现） |
| --- | --- |
| Windows x64 | `ai-memory-windows-x86_64.zip` |
| macOS Apple Silicon | `ai-memory-macos-aarch64.tar.gz` |
| macOS Intel | `ai-memory-macos-x86_64.tar.gz` |

校验值来自 [官方发布信息](https://github.com/akitaonrails/ai-memory/releases)。Mac 安装器不依赖 Homebrew、sudo 或 Rust 编译，不修改系统 PATH 或关闭系统安全检查。以运行中 CrewVela 的目标架构选择包；在 Rosetta 下运行 Intel 构建时使用 Intel 包。

TAR.GZ 使用专用解析器解压，兼容官方包的 `./` 前缀；拒绝目录越界、链接、特殊文件和重复写入，限制压缩包与展开内容大小。Unix 只恢复必要执行位，不保留 setuid/setgid。下载、解压、权限、版本或模板校验失败时不会替换已有安装目录，错误会显示在启用进度中。

Windows 主机已实际下载并校验两种 Mac 完整包，使用生产 Rust 解压器通过；Windows 官方包的隔离启用与重复配置回归通过，界面模拟验证两种 Mac 包和 TAR.GZ 导入。Mac 执行权限、原生服务与 WKWebView 首次安装尚需实机验证。

### macOS 原生验收

1. 更新到含本改动的源码并运行 `npm ci`。先运行安装与接入模块回归：

   ```bash
   cargo test --locked --manifest-path src-tauri/Cargo.toml --lib ai_memory:: -- --test-threads=1
   ```

2. 下载当前 Mac 架构的官方完整 TAR.GZ（不要解压）。在项目根目录显式运行隔离验收；以下以本轮已验证的 3.4.5 为例，包必须来自对应版本，把路径改为自己的下载文件位置：

   ```bash
   CREWVELA_AI_MEMORY_TEST_RELEASE_ARCHIVE="$HOME/Downloads/ai-memory-macos-aarch64.tar.gz" \
   CREWVELA_AI_MEMORY_TEST_RELEASE_VERSION=3.4.5 \
     cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
     ai_memory::onboarding::tests::official_release_first_use_repeat_and_scope_acceptance \
     -- --ignored --exact --test-threads=1
   ```

   Intel Mac 将文件名换为 `ai-memory-macos-x86_64.tar.gz`。测试在临时 home/config/project/data 下启动真实记忆服务，检查安装、MCP/Hooks、重复执行及未标记项目拒绝捕获；不改动真实 Agent 用户配置，不调用模型。

3. 执行 `APPLE_SIGNING_IDENTITY=- npm run build:macos`（Intel 构建命令见 Mac 指南），退出旧版，直接打开本次生成的 `.app`。用专门的 macOS 测试用户验证首次使用，避免把个人 Agent 配置作为验收数据。不要使用 CrewVela 的“新用户隔离测试”模式来验收此功能，该模式会拒绝共享记忆写操作。
4. 在测试用户安装 Claude Code/Codex CLI 后，通过设置向导测试在线下载、TAR.GZ 导入、带中文和空格的项目目录；确认 Mac 包名和对应架构正确。测试错误架构包/损坏包会失败且保留旧安装及数据；反复执行不会丢失已有记忆和其他 MCP。
5. 检查本地服务健康、工具列表与新会话工具加载；完成一次真实模型会话后检查捕获记录。退出 CrewVela 后检查独立服务状态，再重新打开。记录 macOS/芯片、应用源码提交、包名和实际结果；配置成功不能替代真实捕获验收。

6. 使用两个独立下载目录分别保存 2.6.0 和 3.4.5 的同架构包。运行真实升级及故障回退验收（不接触原用户配置）：

   ```bash
   CREWVELA_AI_MEMORY_TEST_OLD_ARCHIVE="$HOME/Downloads/memory-v2/ai-memory-macos-aarch64.tar.gz" \
   CREWVELA_AI_MEMORY_TEST_RELEASE_ARCHIVE="$HOME/Downloads/memory-v3/ai-memory-macos-aarch64.tar.gz" \
     cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
     ai_memory::onboarding::tests::official_major_upgrade_failure_rollback_and_retry \
     -- --ignored --exact --test-threads=1
   ```

   Intel Mac 替换两处包名。该测试实际进行数据库迁移、注入末尾故障、恢复旧程序/数据/配置、保留候选写入并重试成功；随后还需专用测试用户的原生界面验收。

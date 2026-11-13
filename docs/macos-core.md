# Apple 芯片 macOS 核心适配

本阶段目标是让 CrewVela 的项目、原生聊天、Agent CLI、终端、会话恢复及 Skills/MCP 在 Apple Silicon 上运行。保留 Windows PowerShell/CMD 与原有进程管理路径。

当前状态：已合入用户从 Mac 回传的 ARM64 打包、首次启动隔离及普通启动缓存冲突修复，报告覆盖范围见 [合并验证记录](macos-merge-validation.md)。报告记录普通模式构建副本进入主界面并正常退出；安装副本仅完成启动观察，正常退出受工具干扰未验收。真实 CLI、模型及普通/隔离并存等整体功能仍待原生验收；浏览器模拟不代表 WKWebView 或真实 CLI 验收。

独立首次启动测试已增加 macOS `.command` 启动器与 macOS 14+ 专属 WebKit 存储身份；回传报告记录 Finder、A/B 存储隔离与持久化通过。该模式的构建、数据边界和复验步骤见 [macOS 隔离测试](macos-isolated-test.md)。

## 构建

需要 Apple Silicon Mac、Xcode Command Line Tools、Rust stable、Node.js 24 和 Git。Agent CLI 需要分别安装、登录，使用本机可运行的 macOS ARM64 版本。安装 Xcode 工具可运行 `xcode-select --install`。

```sh
uname -m # 应为 arm64；使用原生终端，不通过 Rosetta 构建
rustup target add aarch64-apple-darwin
npm ci
npm run build
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin --lib -- --test-threads=1
npm run tauri -- dev
# 调试完成后构建 .app 和 .dmg
npm run build:macos
```

产物位于 `src-tauri/target/aarch64-apple-darwin/release/bundle/` 下的 `macos/` 与 `dmg/`。测试构建可用 `APPLE_SIGNING_IDENTITY=- npm run build:macos` 做 ad-hoc 签名；这不等同 Apple Developer ID 签名与公证，不是正式分发验收。

`.github/workflows/desktop-core.yml` 提供 Windows 与 macOS ARM64 的测试/构建矩阵，支持手动触发和 PR 检查。macOS job 检查 `uname -m`，再运行 Node/Rust 测试并构建测试 DMG。工作流写入仓库不代表它已经运行或通过。

## 行为与限制

- macOS 新建终端提供 zsh/bash，默认 zsh；Windows 仍提供 PowerShell/CMD。会话的 Shell 类型保持持久化，旧 Windows Shell 名称不改写。
- macOS GUI 启动时读取登录 Shell 的 PATH，探测有超时，并补充 Homebrew、`.local/bin`、`.cargo/bin`、`.npm-global/bin`、`.bun/bin` 常用路径。非标准 CLI 安装位置应在登录 Shell 配置中加入 PATH；修改后重启 CrewVela。Windows PATH 不变。
- POSIX 可视 Agent 任务通过 Bash 执行，参数与 stdin 分离，保存真实退出码；内部任务宿主不加载用户 Shell 配置，避免覆盖所选 Provider 的环境变量，并关闭 job control。普通终端仍使用所选 zsh/bash。任务终端暂显示原始协议输出，最终任务结果仍由既有解析器处理。
- POSIX 可视任务日志上限为 5 MiB，写入失败或超限明确失败，不报告成功。Windows 保留现有日志裁剪与展示。
- POSIX 停止持有所启动的根进程组身份。普通交互终端若另建作业进程组且仍存活，会保留未确认停止状态并报错；需通过另一个终端结束残留作业后重试。不能保证清理用户主动脱离 session 的进程。
- macOS 电脑控制可配置 Open Computer Use、KimiCU 两种外部 MCP 适配器，安装、权限与实际工具接入需分别验证，见[接入与验收步骤](macos-computer-control.md)。内置电脑控制、防休眠仍未适配 macOS。浏览器面板已增加 macOS 14+ WKWebView 专用 MCP 接管，Windows 保留 WebView2/CDP；功能、限制与验收见[macOS 面板控制](macos-browser-panel.md)。关闭面板优先后仍使用外部 Chrome/Edge。
- 共享记忆没有 macOS 托管安装包；需手动安装兼容的 `ai-memory` 并加入 PATH，再配置与检查服务。
- Codex 账户工具在 macOS 直接调用 PATH 中的 `cid`；新增登录需在终端执行 `cid add`，外部 Codex App 需手动重启。
- 位图剪贴板使用跨平台插件；Finder 复制文件的原生文件列表读取尚未适配，文件可通过选择/拖入入口添加。

## 真机验收清单（待执行）

1. 从 Finder 启动 `.app`，验证 Homebrew/用户目录里的 Node、Git、已安装 Agent 被识别；再从终端启动比较结果。
2. 创建带空格、中文的工作目录；打开/切换项目、浏览和编辑文件，重启后恢复项目。
3. 创建 zsh/bash 终端，验证输入输出、中文、缩放、复制粘贴、关闭与重开。
4. 对每种实际安装并登录的 Agent 分别验证新聊天、流式输出、工具调用、权限确认、停止、恢复历史；不能以一个 Agent 通过代表全部通过。
5. 启动可视 Agent 任务，验证成功、失败退出码、取消、长输出限制、任务完成后可再次运行。
6. 普通交互终端运行后台作业后尝试停止；若返回未确认停止，应保留跟踪，不得删除会话或误报停止成功。结束作业后再停止。
7. 设置测试用 Provider 密钥并重启读取，验证 Keychain；测试自建 stdio/HTTP MCP 与 Skills 安装、撤下及路径引用。
8. 验证窗口操作、菜单栏/托盘、Cmd+Enter、图片粘贴、外部浏览器控制，以及尚未支持能力的提示。
9. 同版本在 Windows 回归 PowerShell/CMD、原生聊天、Agent 任务、停止/恢复与既有系统功能。

参考：[Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)、[macOS 签名](https://tauri.app/distribute/sign/macos/)、[GitHub Runner 平台](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)。

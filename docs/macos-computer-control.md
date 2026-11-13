# macOS 电脑控制安装与授权

CrewVela 在 macOS 14+ 接入 Open Computer Use、KimiCU 的 stdio MCP 服务。Windows 原有三种控制方式和安装路径保持；内置 PowerShell 电脑控制仍仅用于 Windows。

## 首次使用

1. 设置 → 电脑控制，在所需工具旁点击「安装并启用」。两种方式相互独立，不要求同时安装。
2. 查看官方版本获取、下载、哈希与签名检查、安装进度。失败会保留错误信息，可重试；不会执行远程 shell 安装脚本。
3. 安装完成后自动注册 CrewVela 管理的 MCP 条目并打开官方授权引导。按官方应用提示，在系统设置中确认辅助功能和屏幕录制。
4. 点击「检查权限」，确认两项权限与 KimiCU 服务状态。安装或配置成功不等于权限已授予或 Agent 工具已连接。
5. 重开 Agent 会话加载 MCP，再验证工具连接。授权引导失败时可点击「打开授权」重试；MCP 部分配置失败时，应用已安装的状态会保留，可启用并保存或重新连接。

## 安装来源和位置

### Open Computer Use

从 [官方 GitHub Releases](https://github.com/iFurySt/open-computer-use/releases) 选择正式发布的 macOS Universal 应用包，使用发布资产的 SHA256/大小、包内版本、Bundle ID `com.ifuryst.opencomputeruse`、开发者 Team ID `J9P29FA5BX`、安全时间戳、严格签名和 Gatekeeper 进行校验。

安装至 `~/Applications/Open Computer Use.app`。不要求全局 npm，不修改 shell 配置或系统 PATH。用户目录中的有效应用优先于旧 npm 启动器；仍兼容 PATH 中的 `open-computer-use` / `ocu` 以及系统 Applications 的已有有效安装。已有目标身份不符或为符号链接时拒绝替换；正常发布失败会恢复旧应用。

### KimiCU

读取 [官方版本清单](https://cdn.kimi.com/kimi-computer-use/latest/version.json)，按硬件选择 `app`（Apple Silicon）或 `app_x86_64`（Intel），不把 arm64 包兜底给 Intel。校验官方 SHA256、包内版本、Bundle ID `ai.kimi.cu`、Team ID `2J9472RW75`、安全时间戳、严格签名和 Gatekeeper。

遵循 [官方安装流程](https://cdn.kimi.com/kimi-computer-use/latest/setup_macos.sh)，由已验签应用执行 `install-bundle` 安装至 `/Applications/KimiCU.app`，让官方运行时管理服务和升级事务。CrewVela 不自行 kill/bootout 或编写 LaunchAgent。

**当前账户必须能写系统 Applications。** 标准账户无写权限时明确失败，不弹管理员认证、不执行 sudo，也不假定官方服务支持用户级应用位置。若官方安装已部分完成，会重新检测真实安装状态并报告未完成步骤。

## 状态与保护

- 官方 HTTPS 来源白名单限制重定向；清单/安装包有大小上限、连接/读取/总超时，解包前检查路径、类型、符号链接祖先/链接链及展开大小。
- 同进程和跨进程安装互斥；安装进度按工具和随机请求标识过滤，不混入另一窗口/任务。
- 读取设置不会安装、准备服务或申请权限；只有明确的安装/授权按钮才触发这些操作。首次启动隔离模式拒绝真实安装和电脑控制。
- 自动启用只更新 CrewVela 管理的当前工具 MCP。用户手工同名 MCP 会报冲突并保留；设置保存失败会恢复该工具原条目，不影响另一种控制方式。
- “已安装”“MCP 已注册”“权限已授予”“工具连接成功”分别判断，未知权限保持待检查。OCU 正式版调用 `doctor --json`，读取 `accessibilityTrusted`、`screenCaptureGranted` 并核对 `allGranted`；KimiCU 读取 `xpc-ping` 明确字段。不把自由文本当作授权证据。
- 安装成功后的授权失败不会删除应用；重新打开设置仍可继续授权。外部安装/服务命令失败不会无依据承诺第三方事务已回滚。

## 验证和边界

- `npm test`：既有电脑控制与相关功能 Node 回归。
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --no-default-features macos_computer_use`：平台/应用/权限解析、注册回滚、安装清单、摘要、归档链接安全、发布回滚、互斥测试。
- `tests/macosComputerUseInstall.ui.mjs`：真实 React + 隔离 Mock，验证两工具入口、进度过滤、下载失败重试、权限拒绝、部分配置失败和 Windows 界面。
- `official_downloads_validate_without_installing -- --ignored --nocapture`：下载真实官方包至临时目录并完成 SHA/元数据/签名/公证验证，不安装、不启动服务、不申请权限。

本轮官方包验证覆盖 OCU 1.1.4 与 KimiCU 0.7.6。Windows 无设备实测；Intel 包仅验证上游清单与元数据，未在 Intel 设备运行。真实 KimiCU `install-bundle`/launchd、用户 TCC 授权和所有 Agent 客户端待用户安装测试。OCU 测试覆盖正常错误恢复，不覆盖进程强制退出的完整恢复；异常退出时可能留下同目录隐藏旧包，需要保留以便恢复。

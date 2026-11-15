> 历史说明：大改分支已在 Beta 精简中移除此功能，本文保留为旧版本设计记录，不代表当前能力。

# macOS 电脑控制安装与授权

CrewVela 在 macOS 14+ 通过 KimiCU 的 stdio MCP 提供电脑控制。Windows 保留内置 PowerShell 电脑控制与 KimiCU；内置控制暂未适配 macOS。

## 首次使用

1. 设置 → 电脑控制，点击「安装并启用 KimiCU」。
2. 查看官方版本获取、下载、哈希与签名检查、安装进度。失败保留错误信息并可重试；不执行远程 shell 安装脚本。
3. 安装后自动注册 CrewVela 管理的 MCP 条目并打开官方授权引导。在系统设置确认辅助功能和屏幕录制。
4. 点击「检查 KimiCU 权限」，确认两项权限与后台服务状态。安装或配置成功不等于权限已授予或 Agent 工具已连接。
5. 重开 Agent 会话加载 MCP，再验证工具连接。授权引导失败可点击「打开 KimiCU 授权与后台服务」重试；MCP 部分配置失败可启用并保存或重新连接。

## 安装来源和位置

读取 [KimiCU 官方版本清单](https://cdn.kimi.com/kimi-computer-use/latest/version.json)，按硬件选择 `app`（Apple Silicon）或 `app_x86_64`（Intel）。不把 arm64 包兜底给 Intel。校验 SHA256、包内版本、Bundle ID `ai.kimi.cu`、Team ID `2J9472RW75`、安全时间戳、严格签名和 Gatekeeper。

由已验签应用执行 `install-bundle` 安装至 `/Applications/KimiCU.app`，让官方运行时管理服务和升级事务。CrewVela 不自行 kill/bootout 或编写 LaunchAgent。

**当前账户必须能写系统 Applications。** 无写权限时明确失败，不弹管理员认证、不执行 sudo。若安装部分完成，会重新检测真实状态并报告未完成步骤。

## 状态与保护

- 官方 HTTPS 来源白名单限制重定向；清单/安装包有大小上限及超时，解包前检查路径、类型、符号链接链及展开大小。
- 同进程和跨进程安装互斥；进度按工具和随机请求标识过滤。
- 读取设置不会安装、准备服务或申请权限。隔离测试模式拒绝真实安装和电脑控制。
- 自动启用只更新 CrewVela 管理的 KimiCU MCP。用户手工同名 MCP 会报冲突并保留；设置保存失败会恢复该工具原条目。
- “已安装”“MCP 已注册”“权限已授予”“工具连接成功”分别判断。KimiCU 读取 `xpc-ping` 明确权限字段；未知权限保持待检查。
- 安装后的授权失败不删除应用；重新打开设置仍可继续授权。外部服务命令失败不无依据承诺第三方事务已回滚。

## 升级清理

Open Computer Use 产品支持已移除，Windows、macOS 均不再提供设置、安装或运行时适配。启动清理旧版自动注册的 `builtin@open-computer-use` MCP 条目及专属设置；所有 MCP 读取路径也过滤该旧条目。用户手工添加的同名 MCP 和自行安装的外部应用不会被删除。

## 验证和边界

- `npm test`：电脑控制与相关 Node 回归。
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --no-default-features macos_computer_use`：平台、应用、权限解析及注册回滚；安装器原生测试仅在 macOS 编译运行。
- `tests/macosComputerUseInstall.ui.mjs`：真实 React + 隔离 Mock，覆盖单工具入口、进度过滤、下载重试、授权状态、部分配置失败、离页恢复、窄屏和 Windows 入口。
- `tests/collaborationAsync.ui.mjs`、`qa-kimi-computer-use.mjs`：保留控制方式的加载、保存、失败重试与异步防重。
- `mcp::tests::retired_builtin_is_not_loaded_and_migration_preserves_user_and_active_servers`：旧条目停止派发、清理幂等、手工 MCP 与有效配置保留、损坏注册文件不覆盖。

Windows 构建和 Mock 验证不能代替 macOS 原生编译、真实 KimiCU 安装/launchd 服务及用户 TCC 授权验收。

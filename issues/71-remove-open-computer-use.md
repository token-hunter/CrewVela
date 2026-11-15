# 移除 Open Computer Use 支持

Status: ready-for-human

## 范围

移除 Windows、macOS 的 Open Computer Use 设置入口、适配器、安装与权限分支、IPC 命令及专属测试。保留内置电脑控制、KimiCU 和用户自行配置的 MCP。

## 验收

- Windows 面板保留内置控制和 KimiCU；macOS 只显示 KimiCU。
- 加载、保存、安装、权限检查不再请求被移除的命令。
- 旧版自动注册的 MCP 不再派发，并在启动时清理；不按名称删除用户 MCP。
- 回归 KimiCU 安装、失败重试、权限未知/拒绝、部分保存、异步防重与窄屏。
- 执行前端构建、相关 Node/Rust 测试和隔离 UI 检查；明确 macOS 真机验证边界。

## Comments

- 现有 `Cargo.toml`、`lib.rs`、`skill_center.rs` 修改已备份于 `.scratch/remove-open-computer-use/baseline.patch`，不回滚原有工作。

## 实施与验证结果

- 已删除独立 Rust 适配模块、前端设置类型和专属 Node 测试；移除命令注册、设置读取/保存与搜索词、macOS 安装来源/签名身份/发现/授权分支。
- 保留的唯一旧标识用于历史注册迁移和拒绝旧命令的测试。启动只清理产品自动创建的条目及其设置，读取时也过滤；手工同名 MCP 和外部应用保留。
- 前端构建通过。全量 Node：490 通过、1 跳过、0 失败；首次运行发现既有技能部署测试硬编码旧品牌名，改为复用 `brand::LEGACY_NAME` 后重跑通过，品牌规则未放宽。
- Rust 定向回归：macOS 共享适配逻辑 8、MCP 37、KimiCU 6、内置控制与 MCP 15、历史路径部署回归 1，均通过。
- 隔离 UI：macOS 安装/进度/授权/重试/部分失败/离页恢复、Windows 两入口及精确 IPC/保存重试、协作相关异步防重均通过；已查看宽屏及窄屏截图。
- macOS 安装器经 Rust 语法解析检查；当前 Windows 环境未进行 macOS 原生编译与真实 KimiCU/TCC 验收。
- 本任务未重新打包 EXE/DMG；当前运行中的旧二进制不会随源码修改自动更新。

# macOS 共享记忆快速安装

Status: ready-for-human

## 需求

macOS 使用与 Windows 相同的“开始配置 → 选择项目和 Agent → 安装并启用”流程，无需手动安装 ai-memory；保留原有记忆、其他 MCP、用户配置及 Windows 安装行为。

## 实现

- 统一平台包描述，Windows x64 使用 ZIP，macOS Apple Silicon / Intel 分别使用 TAR.GZ 和独立官方 SHA256。初版以 v2.6.0 验证；后续动态版本与托管更新见 [任务 60](60-ai-memory-release-updates.md)，不再固定默认安装版本。
- 在线下载和本地导入使用相同校验；限时下载、限量解压，拒绝越界、重复文件、链接和特殊文件，Unix 恢复可执行权限，版本和 hooks 模板检查后才发布。
- 沿用现有备份、冲突检查、独立数据目录、allowlist、进度恢复和“待新会话验证”；不自动升级其他版本或重启 Agent。
- macOS 服务使用独立进程组和数据工作目录，不修改 launchd、系统 PATH、Gatekeeper 或真实账号凭据。
- 向导由后端返回平台与包名，Mac 导入使用 TAR.GZ 筛选，不再显示 Windows ZIP 文案。

## Windows 主机验证（2026-11-11）

- 下载两种 Mac 官方完整包，SHA256 与官方 v2.6.0 发布页匹配，使用真实 Rust 提取器解压成功；交叉架构校验拒绝通过。
- Windows 官方包的隔离首次安装、服务启动、Claude/Codex 配置、重复配置和作用域检查通过；测试配置与数据位于独立临时目录。
- 共享记忆 Rust 42 项通过，5 项默认 opt-in 未运行，其中两种 Mac 包验收与 Windows 官方包隔离启用两项已额外显式运行通过；其余在线安装/本机服务检查未运行。Node 444 项通过、1 项既有跳过；类型检查/前端构建、向导及原有接入/修复界面回归通过。Mac 界面测试是浏览器模拟 IPC，不能证明原生安装成功。
- 现有 Rust 警告、前端分块体积提示保留。

## 待验收

按 [共享记忆快速启用](../docs/shared-memory-quick-setup.md#macos-原生验收)在 Mac 运行完整原生安装和首次模型会话捕获；当前主机未执行 Mac 原生构建、WKWebView 向导和真实 Agent 捕获。此改动未提交、未推送、未更新 Release。

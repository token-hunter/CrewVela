Status: ready-for-human

# macOS 接入 Open Computer Use 与 KimiCU

按用户指定，仅适配这两种外部方式，不移植内置电脑控制。保留 Windows 行为，不自动授权或触碰用户历史数据。

已完成源码：程序发现、应用包验证、独立 MCP 保存/关闭/重新连接、明确的权限检查和官方授权/服务准备入口；Mac 页面按 Open Computer Use、KimiCU 排序。

待 Mac 实机验收，参见 [验收步骤](../docs/macos-computer-control.md)。Windows 及 Mock 验证结果在完成后追加。

## Comments

- 基于 origin/main `6c2003c` 的独立任务分支开发，未改动其他功能工作区；尚未提交、推送或打包。

## 验证记录

- Windows：Rust lib 1079 通过、46 项既有 opt-in 忽略；其中外部电脑控制针对性 19 项通过。
- Node：447 通过、1 项既有跳过；TypeScript、Vite 生产构建通过。
- cargo check 通过，仍有项目既有警告（48 项），未为此修改无关模块。
- 实际运行正式组件的隔离网页 Mock：Mac 两种方式顺序、未安装/不支持禁用、未知权限、授权入口、分别保存、重连、失败提示、明暗显示；Windows 三种方式与保存路径均已检查。调用记录确认 Mac 未调用 Windows 内置控制，也未改动另一种方式。
- 官方源码核对 Open Computer Use doctor --no-ui JSON；官方 KimiCU.app.zip 元数据确认 ai.kimi.cu / kimi-cu / macOS 14.0 / arm64（本次包 0.7.3），只读检查，未安装运行。
- Mac 原生构建、Finder 启动、系统授权、后台服务及真实 MCP/截图/点击/输入未验证；不将网页 Mock 或 Windows 编译结果当作 Mac 验收。
- 未提交、未推送、未打包，仓库可见性未变更。

# 隔离页面回归

使用 Vite 打开 `macos-computer-control-ui.html?page=computer`，该入口模拟 Mac 平台并复用正式组件，IPC 全部替换为本地 Mock，不运行任何控制程序。

- 初次加载只有 KimiCU；不调用内置电脑控制或已移除的适配命令。
- 检查权限显示待授权；打开授权用 Mock 更新为已授予，不代表实际系统授权。
- 启用、保存与重新连接只调用 KimiCU 对应命令。
- “下次保存失败”也可模拟权限检查失败，错误须显示。
- 增加 `&scenario=missing` 或 `&scenario=unsupported`，检查缺失、不支持时不能启用或运行检查。
- Windows 对照入口 `settings-workspace-ui.html?page=computer` 显示内置电脑控制、KimiCU 两种方式。

Mac 真机验收步骤见 `docs/macos-computer-control.md`。

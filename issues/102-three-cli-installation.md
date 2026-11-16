Status: ready-for-human

# OpenCode、Kimi Code、Pi 安装与版本

为三种 CLI 提供现有安装与版本流程，保留 ZCode CLI 当前行为。需求及验证边界见 `.scratch/runtime-install-three-20261116/spec.md`。

## 验收

- 三种 CLI 出现安装入口，展示实际版本、来源、路径和对应安装/登录说明。
- 安装到独立版本目录，校验与版本/帮助自检通过才切换，不覆盖原安装及配置。
- 取消、失败重试、旧版本保留有效，终端和原生聊天均可找到托管 CLI。
- 完成后记录实际测试和未验证项。

## Comments

2026-11-16：实现完成，等待 EXE 打包后的桌面人工验收。ZCode CLI 按用户补充要求暂不适配。

验证：前端全量 477 通过 / 1 跳过；后端全量 1084 通过 / 52 个 opt-in 跳过；新三种官方 Windows x64 包另行执行隔离下载、完整性校验、版本/帮助自检和入口发布，1 项通过；真实组件 IPC 模拟界面 10 组加依赖阻断场景通过；构建通过。

实际登录、模型请求及 Windows ARM64 运行未执行；未改用户 CLI 安装或账号配置，未重新打包 EXE。详细记录：`.scratch/runtime-install-three-20261116/verification.md`。

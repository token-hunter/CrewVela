Status: ready-for-human

# 协作入口仅保留具体 Agent，支持多选和全选

按用户选定方案移除类型群组和 @All 入口，保留明确成员选择。全选展开为具体成员，连续多选不关闭菜单，维持原通信和计划链路。

实施及验收记录见 `.scratch/agent-selection/`。要求完成界面运行验证、接收方 ID 及受影响链路回归，然后按约定命令重新打包 Windows EXE。

## Comments

已完成成员入口、多选、全选和候选菜单遮挡修复。相关 Node 测试 30 通过；具体选择、原控制链路、多实例和任务详情四组实际浏览器回归通过。核对了明确成员 ID，不将模拟 IPC 当作真实模型收发。本轮无 Rust 业务代码变更，未重复上轮模型互聊测试。EXE 已按约定构建，生产资源及 CLI 启动检查、隐私扫描通过。[验收记录](../docs/explicit-agent-selection-verification-20261115.md)。

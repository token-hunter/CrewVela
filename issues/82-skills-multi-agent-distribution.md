# Skills 分发支持多选 Agent
Status: ready-for-agent

## 需求
修复分发页 Agent 单选限制；支持同时选择多个 Runtime，将所选 Skill 分发给所有选中目标。

## 验收
- 增减 Agent 不清空选中的 Skill；没有目标时禁止部署、撤下及保存配置集。
- 部署、撤下和配置集包含全部选中目标，未选中的目标不受影响。
- 各目标状态和路径可辨识，部分同步不会显示成全部同步。
- 保留冲突预检、信任确认、任务隔离和重复提交保护。
- 验证实际 React 交互、IPC 参数及独立临时目录中的真实后端分发。

## Comments
- 已完成多选、逐目标状态、配置集及撤下链路；复用已有后端批量接口。
- 14 项前端逻辑检查、17 项 Rust 后端测试、TypeScript 与实际 IAB 交互通过。证据见 docs/skills-multi-agent-verification-20261115.md。

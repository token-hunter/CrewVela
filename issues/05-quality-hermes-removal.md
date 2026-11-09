Status: ready-for-human

# 项目质量与 Hermes Agents 移除

分支：quality-hermes-removal-audit。规格：`.scratch/quality-audit/spec.md`。

验收：移除 Hermes 前后端执行/入口/技能部署/专用桥及素材；未知历史运行时拒绝执行并保留可读性；修复 astra 证实的缺陷并回归；实际运行检查 UI。

## Comments

- 工作区有大量既有未提交修改，未回滚、未自动提交。
- astra 审查发现的账户存储错误、Cron 调度与旧数据迁移、原生状态事件自动化、Pi 模型错误与 ACP/Pi 提前写盘失败结算、新回合错误隔离等缺陷均已修复并回归。
- 最终前端构建通过；Node 379 通过/0 失败/1 平台跳过；Rust 库 932 通过/0 失败/28 既有忽略，CLI 1/1。实际 Chrome 多尺寸和关键交互回归通过并查看截图。
- 独立复核完成，生产代码 Hermes 引用为 0。详细证据、首轮失败和未验证边界见 `docs/quality-hermes-removal.md`；真实登录/在线模型及 macOS/Linux 实机不计为已通过。

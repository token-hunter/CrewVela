Status: ready-for-human

# 将工作区协作记录移到设置的协作中心

采用用户选定第 1 版时间线。移除常驻记录栏，保留所有原记录类型和历史数据，在设置中增加记录查询、筛选与正文展开。

方案与验证证据位于 `.scratch/collaboration-records/`。

## Comments

已完成记录迁移、时间线查询、原事件载荷保留和旧侧栏移除。相关单元测试 11 通过；记录/异步协作/任务视图/原控制四组浏览器回归通过；深浅色三种尺寸无横向溢出。原历史存储未改。Windows EXE 已重新构建，复制哈希、CLI 启动检查及隐私扫描通过；未在本轮运行原生 GUI、真实模型互聊或 macOS 验证。

[完整验收记录](../docs/collaboration-records-verification-20261115.md)。交付目录：`output/CrewVela-records-settings-20261115/`。

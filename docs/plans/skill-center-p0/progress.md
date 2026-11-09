# 进度：Skill 中心 P0

## 2026-08-19
- 已读取 Skills Manager 源码审计报告并确认 P0 五项范围。
- Skill 部署路径按受支持的 Runtime 分别配置。
- 已建立独立计划，开始模块接口和路径审计。
- 已复核 Skills Manager 的目标分类、全量 preflight、Windows 三级降级和 Copy Hash 保护实现。
- 下一步实现 `SkillCenter` Rust 深模块、数据库迁移和 Tauri 命令。
- 已新增 SQLite 中央资产库、来源/Hash/信任信息、文件预览与风险扫描。
- 已新增八 Runtime 路径适配器、发现扫描、全量 preflight、链接/Junction/Copy 安全部署及受保护撤下。
- 已新增全局、项目、任务/Agent 作用域和可并行生效的 Skill 配置集。
- 已接入设置 → Skill 中心，提供分发矩阵、批量部署、配置集、来源导入、扫描纳管和文件审阅。
- `cargo check`、Skill Center 3 项 Rust 单测、前端生产构建已通过。
- 安全测试扩展到 7 项：嵌套归档导入、实际 Git Commit 记录、重复部署去重、批量预检零部分写入均通过。
- Rust 全量：350 通过、3 忽略、0 失败；Skill Center 前端 5 项全部通过；生产构建通过。
- 全前端回归共 73 项，67 项通过；6 项为任务开始前即存在的 LiveKit、通知历史、首页额度同步与 Headless Chrome GPU 环境失败，本次未触碰相关实现。
- Playwright 在 1280×720 验证：八个 Runtime 列全部可见，分发状态无裁切；截图位于 `output/playwright/skill-center-p0-compact.png`。
- `git diff --check` 通过；未重新打包或重启 App。

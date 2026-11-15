# 协作记录迁入设置：实施与验证

用户选定第 1 版时间线，入口为“设置 → 协作中心 → 协作记录”。移除主工作区常驻记录栏和专属样式，包括窄窗口下遗留的 240px 列宽。原成员选择、终端、任务控制和错误提示保留。

复用原记录栏的数据汇总，迁入消息、即时投递、交付物、事件、运行和经验。记录按日分组、时间倒序，支持搜索、类型筛选和展开正文；统一入口保留原“全部活动”弹窗中的事件载荷。未修改存储、未删除历史。展示范围仍取现有快照与投递服务返回的数据，不承诺超出现有服务保留范围的无限历史。

只在记录页加载投递视图，加载失败显示原因和重试，保留其余已取得记录。切项目时旧记录页卸载，旧请求返回不能覆盖新项目；投递只显示收发双方至少一方关联当前项目的记录。

## 验证

- 相关 Node 测试 11 通过、0 失败，覆盖记录来源、正文、排序、项目范围、原始事件载荷、参与状态与诊断语义。
- `collaborationRecords.ui.mjs` 通过：正文、搜索、筛选、展开、投递失败重试、跨项目延迟隔离、无侧栏及终端实际宽度。
- `collaborationAsync.ui.mjs`、`collaborationTaskFirst.ui.mjs`、`retainedControlAsync.ui.mjs` 通过：原有任务、审批、接管、项目隔离及计划链路。
- 实际设置宿主在深浅色的 1440×1024、820×540、687×903 下通过，无横向溢出；记录页不展示无关 Runtime 和项目操作工具栏。
- IAB 实际进入页面并操作搜索；并列视觉对比 `.scratch/collaboration-records/reference-comparison.png` 已检查。`design-qa.md` 当前核验结果为 passed。
- TypeScript/Vite 构建通过；已有包体积警告保留。未修改 Rust 业务代码，未重跑模型互聊或 macOS 原生测试。

初次宽度断言失败，定位到 `panels.css` 的窄窗旧侧栏列宽后修复，未放宽断言。视觉检查发现的样式覆盖也已实际修复。日志位于 `.scratch/collaboration-records/`。

![协作记录时间线](../output/playwright/collaboration-records-dark-1440.png)

Windows EXE 按 `npm run tauri -- build --no-bundle` 构建，最终产物信息另附交付目录 README。当前用户窗口及正式历史未操作。

- 文件：`output/CrewVela-records-settings-20261115/CrewVela.exe`。
- 大小：46,385,152 字节。
- SHA256：`58969FB0E1998BEB28F81562DC80D77643B5AAFBF462ED7E2086066BF927E756`。
- 最终构建成功，已有 50 条 Rust 警告保留。CLI 启动检查、复制哈希及发布隐私扫描通过；未以 CLI 检查冒充原生 GUI 测试。

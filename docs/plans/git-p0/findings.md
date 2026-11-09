# 发现：Git P0 工作台

## 已确认产品边界
- P0 目标是 Git 日常操作和 Agent/Worktree 交付审核闭环。
- 比较基准与 PR/Rebase 目标必须独立；本阶段不实现 PR/Rebase。
- Discard 必须二次确认，并区分 tracked 与 untracked。
- Pull 默认只允许 fast-forward；强推只允许显式 `--force-with-lease`。
- 不假设 `origin/main`，必须解析仓库默认分支或当前 upstream。

## 规划文件说明
- 项目根目录已有其他任务的 planning-with-files 文件，不能覆盖；本任务使用 `docs/plans/git-p0/`。

## CrewVela 现有架构
- 当前只有 `worktree.rs` 的创建、是否脏、清理能力，没有通用 Git status/diff/stage/commit/remote 服务。
- 普通会话 Worktree 元数据随 `SessionRecord.worktree` 持久化，包含 repoRoot/path/branch。
- 多 Agent 任务 Worktree 由 executor 按 project/task/run 生成，当前没有面向 UI 的统一 Worktree 列表接口。
- 设置页采用单一 `SettingsDialog` + `SettingsSection` 联合类型，全屏内容区适合新增独立 `git` 页面。
- 设置组件已经拿到 sessions、projects，可先统一展示项目根目录和普通会话 Worktree；多 Agent Worktree 需要后端补充发现逻辑。
- 现有视觉方向是深色、克制、信息密集的开发工具；Git 页面应使用三栏工作台而不是继续堆嵌套卡片。
- 多 Agent Run 的 `sessionLocator.cwd` 可提供任务 Worktree 路径；项目快照已经包含 runs、tasks、agents，前端可通过 `ma_list_projects` + `ma_get_snapshot` 发现并命名这些工作区。
- 设置打开逻辑使用 `SettingsSection` 目标路由；新增 `git` section 后可从设置导航进入全屏工作台，无需创建第二套窗口。
- Git 服务应只接受前端选择出的绝对工作区路径，但后端仍必须以 `rev-parse --show-toplevel`、相对路径规范化和 Worktree 边界验证为准，不能信任 UI。
- 普通会话分支当前固定 `crewvela/session-*`；多 Agent 分支固定 `crewvela/task-*`，分支前缀设置必须同时接入这两条创建链路才算真实生效。
- 现有 `title_summary.rs` 已提供安全的单轮 Claude JSON 调用模式，可复用其进程约束实现 AI Commit message，但 Git 服务必须只提供 staged summary/patch，不能让生成 Agent拥有工具权限。
- Tauri 初始化和 handler 集中在 `lib.rs`；Git P0 可以用无状态命令，不需要新增全局 Manager，Git 设置单独持久化为配置目录 JSON。
- 浏览器预览确认 Git 导航、命令栏和三栏工作台在 1280×720 左右窗口内完整显示；工作区为空时仍保留明确空状态和禁用操作。
- `ui-preview` 无 Tauri bridge，因此页面会显示 invoke 缺失错误；这是预览环境限制，不代表真实 App 后端失败。真实操作由 Rust 临时仓库测试覆盖。
- `SettingsDialog` 的 Git section 已使用 wide content；不需要复用 automation/knowledge 的 feature-dialog 特殊布局。

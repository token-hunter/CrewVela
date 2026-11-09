# 任务计划：Git P0 工作台

## 目标
在 CrewVela 中实现围绕当前项目、普通会话 Worktree 和多 Agent 任务 Worktree 的 Git 日常闭环：状态、Diff、Stage/Unstage/Discard、Commit、Commit & Push、Fetch/Pull/Push/Publish、ahead/behind/upstream、比较基准和交付状态。

## 当前阶段
完成

## 阶段

### 阶段 1：现有架构与安全边界审计
- [x] 找到项目/会话/Agent Worktree 数据入口
- [x] 审计现有 Git 命令封装、Tauri 命令和测试方式
- [x] 固定 P0 类型、状态机和安全约束
- **状态：complete**

### 阶段 2：Rust Git 服务与命令
- [x] 状态、分支、upstream、ahead/behind、默认基准
- [x] staged/unstaged/untracked/conflict Diff
- [x] Stage/Unstage/安全 Discard
- [x] Commit/Fetch/Pull/Push/Publish/force-with-lease
- [x] 路径边界、脏状态和错误输出测试
- **状态：complete**

### 阶段 3：React Git 工作台
- [x] 设置导航与全屏页面入口
- [x] 工作区/Worktree 选择器与交付状态
- [x] 文件分组、Diff 查看、批量操作
- [x] Commit composer 与远程同步操作
- [x] 确认对话框、错误和空状态
- **状态：complete**

### 阶段 4：验证
- [x] Rust 全量测试
- [x] 前端回归测试与生产构建
- [x] 临时 Git 仓库真实闭环测试
- [x] `git diff --check` 和差异审计
- **状态：complete**

## 范围外
- PR/MR 创建、Rebase、Git History、Branch Picker、行级/Hunk 暂存、自动冲突修复。

## 保护范围
- 不修改或提交现有 `.playwright-cli/`、`.tmp/`、`artifacts/` 和其他任务规划文件。
- 不运行无条件 reset/clean，不覆盖用户未提交文件。

## 错误记录
| 错误 | 尝试 | 处理 |
|---|---:|---|
| Discard 测试在 Windows 得到 CRLF | 1 | 测试比较前统一换行，产品逻辑保持 Git 原生 checkout 行为 |
| TypeScript 目标不支持 `String.replaceAll` | 1 | 改用兼容当前 tsconfig 的全局正则替换 |
| Remote 测试复用了系统临时目录的固定 `remote.git` | 1 | 改为随测试仓库 UUID 生成唯一裸仓库路径并清理 |
| 全部前端测试 126 项中 6 项失败 | 1 | 核对均为既有 Chrome GPU、LiveKit 静态断言、notification localStorage 与额度路由测试；Git 新增测试 3/3 通过，生产构建通过 |

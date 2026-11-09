# Orca Git 与源代码控制源码核验及 CrewVela 取舍建议

> 研究日期：2026-08-18
> Orca 本地源码：`REFERENCE_ROOT/orca`
> 固定基线：`6ee265e579e4fd6ce895dd1be8149536451be2c5`（2026-08-17，`feat(agent-status): surface the model each Codex subagent is running (#8251) (#14627)`）
> 证据范围：只使用本地 Orca 源码；下文全部证据均为绝对路径和该固定提交下的行号。
> 成熟度口径：**已实现**＝设置有真实消费链路并最终执行 Git/Provider/Agent；**后端能力**＝存在 API/Provider/RPC，但当前 Source Control 面板没有对应入口；**兼容字段**＝仍参与迁移或 fallback，但已不是当前主 UI；**占位**＝只有界面或类型、没有消费链路。

## 1. 结论

截图中的 Orca Git 设置不是摆设：分支前缀、创建工作区时安全更新本地 `main/master`、变更分组顺序、默认对比基准、自动重命名分支、Source Control AI、Commit/PR 提示配置均能追到真实消费链路。当前源码已把截图中的两个简单 instructions 文本框升级为按操作配置的 **Action recipe**，可分别选择 Agent、CLI 参数和命令模板。

Orca 的 Source Control 面板也不是“只展示 `git status`”：它已形成 Renderer → runtime client → 本地 IPC / Runtime RPC → local/SSH Git provider → Git 命令的完整闭环，覆盖状态、Diff、文件级与批量 Stage/Unstage、受保护的 Discard、Commit、Fetch/Pull/Push/Force Push、Fast-forward、Sync、Rebase、冲突中止、提交历史、基准比较和托管 Review 创建。

但不应把 Orca 整套原样搬进 CrewVela：

| 优先级 | 建议能力 | 判断 |
|---|---|---|
| P0 刚需 | 状态列表、Diff、文件级和批量 Stage/Unstage、Commit、Fetch/Pull/Push、ahead/behind/upstream、明确错误反馈 | 构成日常 Git 闭环，几乎每次代码任务都会用到 |
| P0 刚需 | 分支/基准比较；默认对比基准可选 repository default / branch upstream | Agent worktree 审核必须知道“相对哪个基准改了什么” |
| P0 高频 | AI 生成 Commit message；保留一段简化的 Commit instruction/template | 高频、低风险、能直接减少机械操作 |
| P0 条件刚需 | 分支前缀 | CrewVela 自动创建 worktree/分支时是刚需；不创建分支时无意义 |
| P1 高频 | 创建 PR、Draft、使用模板、创建后打开；AI 生成 PR 标题/描述 | 适合把“代码完成”闭环到 Review，但依赖 GitHub/GitLab 等认证与 Provider 抽象 |
| P1 安全增强 | 创建 worktree 时安全更新本地 `main/master`，默认关闭 | 能消除 stale-main 对比错误，但必须复制完整安全门，不能简化成 `reset --hard` |
| P1 | 提交历史、单提交文件/Diff、复制 hash/message、打开远端提交 | 高频审查辅助，但不阻塞首版提交闭环 |
| P2 | 变更分组顺序、列表/树模式、文件过滤、Notes | 体验增强，不是首版刚需 |
| P2 条件功能 | 自动按首个 Agent 任务重命名随机分支/worktree | 强依赖 Orca 的随机分支命名和首次工作事件模型；只有 CrewVela 已有同类流程时才值得做 |
| P2 | 8 套完整 AI recipe、AI 修复 Commit/Push/Checks/冲突/评论 | 能力强但设置复杂；首版只需 Commit 与 PR 两项 |

上述“高频”是基于标准开发循环的产品判断，不是 Orca 使用遥测结论；本次源码没有提供可用于量化按钮使用频率的产品数据。

## 2. 截图设置逐项核验

### 2.1 分支前缀：已实现，建议 P0（有自动建分支时）

- UI 提供 `Git Username / Custom / None`，并持久化 `branchPrefix`、`branchPrefixCustom`：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\GitPane.tsx:169-242`。
- 类型为显式的 `BranchPrefixStrategy`，全局设置保存策略和值：`REFERENCE_ROOT/orca\src\shared\ui-chrome-types.ts:17-18`、`REFERENCE_ROOT/orca\src\shared\global-settings-types.ts:74-75`。
- 默认策略是 `git-username`：`REFERENCE_ROOT/orca\src\shared\constants.ts:181-182`。
- 创建 worktree 时，只有没有显式 `branchNameOverride` 才计算前缀分支名；自定义前缀非法会阻止创建，而 Git 用户名异常会安全回退为无前缀：`REFERENCE_ROOT/orca\src\main\ipc\worktree-branch-name.ts:19-61`、`REFERENCE_ROOT/orca\src\main\ipc\worktree-remote.ts:552-588`。

结论：不是单纯展示偏好，而是分支创建规则。CrewVela 若已经自动创建 worktree，建议直接支持 `codex/`、Git 用户名、自定义、无前缀四种语义；若只操作用户当前分支，则无需此设置。

### 2.2 保持本地 main 最新：已实现，但仅在创建 workspace 时触发，建议 P1、默认关闭

- UI 文案明确：创建 workspace 时刷新远端 base，并安全 fast-forward 匹配的本地 `main/master`；有未提交变更或本地独有提交则跳过：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\GitPane.tsx:244-290`。
- 默认值为 `false`：`REFERENCE_ROOT/orca\src\shared\constants.ts:177`。
- 本地创建链路会把该设置传入实际 Git worktree 创建流程；SSH 创建链路也有专门的远端调用：`REFERENCE_ROOT/orca\src\main\ipc\worktree-remote.ts:1679-1713`、`REFERENCE_ROOT/orca\src\main\ipc\worktree-remote.ts:2323-2420`。
- 安全判断要求本地分支 `ahead === 0`，本地 OID 必须是远端 OID 的祖先；若该分支被某个 worktree checkout，tracked 状态必须干净：`REFERENCE_ROOT/orca\src\main\git\worktree.ts:214-320`。
- 真正更新时，已 checkout 的 owner worktree 用 `reset --hard <已验证远端OID>`；未被 checkout 的分支用带 expected-old-OID 的 `update-ref` CAS：`REFERENCE_ROOT/orca\src\main\git\worktree.ts:866-928`。
- 这里检查使用 `git status --porcelain --untracked-files=no`，因此只有未跟踪文件不会阻止快进：`REFERENCE_ROOT/orca\src\main\git\worktree.ts:282-295`。

结论：它不是后台持续同步，也不等同于普通 `git pull`。移植时必须保留 `ahead=0`、祖先关系、owner worktree tracked-clean、OID CAS 和失败不阻断 worktree 创建等门禁；否则风险高于收益。

### 2.3 源代码管理分组顺序：已实现，P2 体验设置

- UI 支持 `Changes first / Staged first / Untracked first`：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\GitPane.tsx:75-135`。
- 默认 `changes-first`：`REFERENCE_ROOT/orca\src\shared\source-control-group-order.ts:1-8`。
- 面板真实映射三种顺序，冲突条目始终单独置顶：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\section-order.ts:26-36`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\section-order.ts:80-129`。

结论：真实生效，但只是展示偏好。首版固定为 `Changes → Staged → Untracked` 即可，等用户反馈后再开放设置。

### 2.4 默认对比基准：已实现，建议 P0

- UI 提供 `Repository default / Branch upstream` 两种策略，并明确只影响已提交变更比较，不改变 PR 和 Rebase 的目标：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\CompareAgainstUpstreamSetting.tsx:24-35`、`REFERENCE_ROOT/orca\src\renderer\src\components\settings\CompareAgainstUpstreamSetting.tsx:46-103`。
- 默认是 repository default（布尔值 `false`）：`REFERENCE_ROOT/orca\src\shared\constants.ts:275`。
- 实际优先级为 worktree pin / repo pin 优先；启用 branch-upstream 后再取当前 upstream，最后回退到 repository default：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\base-ref-resolution.ts:21-37`。
- 面板通过 owner host 获取仓库默认 base，失败时保持 `null`，不会虚构 `origin/main`：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\use-base-ref-default.ts:20-47`。
- 分支比较会解析真实 OID、merge-base 和变化条目，并处理 unborn HEAD 与无效 base：`REFERENCE_ROOT/orca\src\main\git\status.ts:1403-1475`。

结论：这是 Agent/worktree 场景的核心能力。CrewVela 必须把“Diff 的比较基准”和“PR/Rebase 目标”建成两个概念，避免一个下拉框同时改变审查视图和发布目标。

### 2.5 自动重命名分支和 worktree：已实现、默认开启，但强产品耦合

- UI 是真实 toggle，并带可编辑的 Branch-name Action recipe 模板：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\AutoRenameBranchFromWorkSetting.tsx:126-188`、`REFERENCE_ROOT/orca\src\renderer\src\components\settings\AutoRenameBranchFromWorkSetting.tsx:190-310`。
- 默认开启：`REFERENCE_ROOT/orca\src\shared\constants.ts:179-180`。
- 只有首次收到 live working 事件且存在 prompt 才尝试：`REFERENCE_ROOT/orca\src\main\agent-hooks\first-work-branch-rename.ts:95-133`。
- 只允许重命名 Orca 自建、仍是自动 creature 名、没有 upstream 的分支；不会覆盖用户手选分支：`REFERENCE_ROOT/orca\src\main\agent-hooks\first-work-branch-rename.ts:183-207`。
- 生成 slug 后会重新校验分支状态，再执行 Git rename，并同步 sidebar display name / folder：`REFERENCE_ROOT/orca\src\main\agent-hooks\first-work-branch-rename.ts:210-317`。

结论：如果 CrewVela 没有“先建随机临时分支，再根据 Agent 首次任务命名”的模型，不建议移植。更简单可靠的方案是在创建 worktree 前，根据用户输入的任务标题直接生成一次分支名并允许确认。

### 2.6 Source Control AI 开关：已实现，不是实验占位

- UI 的 `Show Source Control AI actions` toggle 会写入 `sourceControlAi.enabled`：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\CommitMessageAiPane.tsx:128-130`、`REFERENCE_ROOT/orca\src\renderer\src\components\settings\CommitMessageAiPane.tsx:156-214`。
- 默认 `enabled: true`，但默认 `agentId: null`；因此“按钮可见”不代表已经具备可运行 Agent：`REFERENCE_ROOT/orca\src\shared\source-control-ai.ts:456-479`。
- 可见性支持仓库级 override：`REFERENCE_ROOT/orca\src\shared\source-control-ai.ts:1148-1158`。
- 面板会依据解析结果控制 AI 操作和生成对话框：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\ai\use-ai.ts:63-101`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\ai\use-ai.ts:233-258`。

结论：CrewVela 首版应避免“总开关开着但没有可用 Agent”的隐性无效状态。建议开关旁直接显示当前 Agent/模型；没有可运行 Agent 时，生成按钮应给出可操作的配置提示。

### 2.7 Commit message / PR instructions：真实功能已升级为 Action recipe

- 当前一共有 8 类 recipe：`commitMessage`、`pullRequest`、`branchName`、`fixCommitFailure`、`fixPushFailure`、`fixChecks`、`resolveConflicts`、`resolveComments`：`REFERENCE_ROOT/orca\src\shared\source-control-ai-actions.ts:7-55`。
- 每项用途描述不是占位：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\source-control-action-recipe-options.ts:22-56`。
- 每项可配置 Agent、CLI args、command template，并支持 Save/Discard：`REFERENCE_ROOT/orca\src\renderer\src\components\settings\SourceControlActionRecipeRow.tsx:78-219`。
- 默认模板都是 `{basePrompt}`：`REFERENCE_ROOT/orca\src\shared\source-control-ai-actions.ts:67-80`。
- Commit 模板可插入 branch、stagedFiles、stagedPatch、linkedIssue；PR 模板可插入 baseBranch、currentTitle、currentBody、commitSummary、changedFiles、patch、linkedIssue：`REFERENCE_ROOT/orca\src\shared\source-control-ai-action-variables.ts:8-29`。
- Commit 的模板变量会实际渲染并交给所选 Agent：`REFERENCE_ROOT/orca\src\main\text-generation\commit-message-text-generation.ts:1112-1147`。
- PR 的模板变量也会真实渲染、生成并解析为 title/body/draft：`REFERENCE_ROOT/orca\src\main\text-generation\commit-message-text-generation.ts:1183-1226`。
- 旧 `instructionsByOperation` / `customPrompt` 仍是兼容字段，并参与迁移或 fallback；不是死字段，但已不是当前主 UI：`REFERENCE_ROOT/orca\src\shared\source-control-ai-types.ts:25-41`、`REFERENCE_ROOT/orca\src\shared\source-control-ai.ts:1011-1031`、`REFERENCE_ROOT/orca\src\shared\source-control-ai.ts:1089-1131`。

结论：CrewVela 不需要首版复制 8 套 recipe 编辑器。建议只提供：统一 AI 开关、默认 Agent/模型、Commit instruction、PR instruction。内部数据模型可预留 `operation -> recipe`，UI 后续再逐步展开。

## 3. Source Control 面板的真实能力与调用链

### 3.1 面板不是旧式单文件组件，而是已拆分的完整功能域

- 右侧栏按需加载 `SourceControl`：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\right-sidebar-panel-content.tsx:7-28`。
- `SourceControl.tsx` 仅是入口，实际挂载 `SourceControlPanel`：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\SourceControl.tsx:19-58`。
- Panel model 组合 status、commit、review、PR intent、entry mutation、history、upstream 和 notes 等 hooks：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\panel\use-panel-model.ts:1-20`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\panel\use-panel-model.ts:70-179`。

### 3.2 Status、Diff 和比较：完整实现

调用链：

1. 面板的 status refresh 调用 `refreshGitStatusForWorktree`：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\use-status-refresh.ts:24-89`。
2. 刷新器调用 `getRuntimeGitStatus` 并写回 store：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\git-status-refresh.ts:99-144`。
3. Runtime client 在本地走 `window.api.git.status`，远端 Runtime 走 `git.status` RPC：`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:124-158`、`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:210-227`。
4. IPC 会验证注册 worktree，按 connectionId 分派 SSH provider 或本地 `getStatus`：`REFERENCE_ROOT/orca\src\main\ipc\filesystem.ts:1145-1195`。

Diff 同样有完整 local/runtime 分派：Renderer 打开 unstaged/staged/branch/commit Diff，runtime client 分别调用 `git.diff`、`git.branchCompare`、`git.commitCompare`、`git.branchDiff`、`git.commitDiff`：`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:354-410`、`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:583-631`。IPC 对文件路径做边界校验并分派真实 provider：`REFERENCE_ROOT/orca\src\main\ipc\filesystem.ts:1371-1403`。本地 `getDiff` 会处理普通文件和 submodule 内部 Diff：`REFERENCE_ROOT/orca\src\main\git\status.ts:1283-1348`。

面板层还真实提供：

- Changes / Staged Changes / Untracked Files / Conflicts 分区、冲突置顶、整区 combined diff：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\section-order.ts:5-25`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\section-order.ts:53-72`。
- 文件名过滤、列表/树模式、选择基准、刷新比较、Notes：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\panel\header-toolbar.tsx:21-45`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\panel\header-overflow-menu.tsx:68-92`。
- 已提交分支变化与未提交变化同时显示：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\panel\panel-content.tsx:121-220`。

### 3.3 Stage / Unstage / Discard：完整实现，但粒度是文件和批量路径

- 每行依据 area、冲突和 submodule 状态决定 Stage / Unstage / Discard 是否可用：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\entry-actions.ts:4-26`。
- 行按钮真实调用对应 handler：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\uncommitted-entry-row.tsx:206-248`。
- 分区头提供 Stage all / Unstage all / Discard all：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\uncommitted-sections.tsx:91-180`。
- Renderer mutation hook 调用 runtime Git client：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\commit\use-entry-mutations.ts:27-71`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\commit\use-entry-mutations.ts:77-147`。
- Runtime client 同时支持 local preload 和远端 RPC：`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:794-918`。
- IPC 最终调用 SSH provider 或本地 Git：`REFERENCE_ROOT/orca\src\main\ipc\filesystem.ts:2161-2230`。
- 本地真实命令是 `git add -- <path>`、`git restore --staged -- <path>`；批量操作会分块避免参数过长：`REFERENCE_ROOT/orca\src\main\git\status.ts:2019-2051`、`REFERENCE_ROOT/orca\src\main\git\status.ts:2310-2355`。
- Discard 会先验证目标仍在 worktree 内；tracked 文件用 `git restore --worktree --source=HEAD`，untracked 走专门的安全删除逻辑：`REFERENCE_ROOT/orca\src\main\git\status.ts:2132-2173`。

边界：在当前固定提交中，没有发现 Source Control 面板按行或按 hunk stage/unstage 的调用链；公开 contract 和 UI handler 的粒度都是 `filePath` / `filePaths[]`。因此不应把“行级暂存”算作当前源码已实现能力。Provider contract 也只暴露文件/批量路径接口：`REFERENCE_ROOT/orca\src\main\providers\git-provider-contract.ts:32-43`。

### 3.4 Commit 与 AI Commit message：完整实现

- Commit composer 会根据状态选择 `Commit / Stage All / Push / Pull / Sync / Create PR` 等主操作：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control-primary-action.ts:34-126`。
- Commit 行为要求非空 message 和 staged snapshot，并通过 owner host 调用 `commitRuntimeGit`：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\commit\use-commit-action.ts:50-102`。
- Runtime client 本地调用 `window.api.git.commit`，远端调用 `git.commit` RPC：`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:637-651`。
- IPC 在边界拒绝空白 message，并分派 SSH provider 或本地 `commitChanges`：`REFERENCE_ROOT/orca\src\main\ipc\filesystem.ts:1406-1430`。
- 本地最终执行 `git commit -m <message>`，并把 hook/commit 失败返回给 Renderer：`REFERENCE_ROOT/orca\src\main\git\status.ts:2099-2114`。

AI Commit message 的链路同样闭合：composer 的生成按钮 → `use-commit-message-generation` → `generateRuntimeCommitMessage` → 本地 IPC / Runtime RPC → 读取 staged summary + patch → 运行所选 Agent → 规范化结果。关键证据：

- 生成按钮及停止态：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\commit\commit-message-composer.tsx:58-107`。
- Runtime local/RPC 分派：`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:657-731`。
- IPC 入口及上下文读取：`REFERENCE_ROOT/orca\src\main\ipc\filesystem.ts:1433-1538`。
- prompt 变量渲染和 Agent 执行：`REFERENCE_ROOT/orca\src\main\text-generation\commit-message-text-generation.ts:1112-1147`。

### 3.5 Fetch / Pull / Push / Force Push / Sync / Rebase：完整实现

- 下拉菜单实际包含 Push、Force Push、Pull、Fast-forward、Sync、Rebase from Base、Fetch、Publish；Force Push 语义明确是 `--force-with-lease`：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control-dropdown-remote-items.ts:54-94`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control-dropdown-remote-items.ts:98-198`。
- Renderer runner 会区分普通 push 和 force push，不会静默把普通 push 升级成强推：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\use-remote-action-runner.ts:123-188`。
- Compound action 会先 Commit，只有 Commit 成功才继续 Push/Sync：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\use-remote-action-runner.ts:270-287`。
- Runtime client 的 local/RPC 分派：`REFERENCE_ROOT/orca\src\renderer\src\runtime\runtime-git-client.ts:439-572`。
- IPC 的 Fetch/Push/Pull 真实分派和 push target 校验：`REFERENCE_ROOT/orca\src\main\ipc\filesystem.ts:1868-1894`、`REFERENCE_ROOT/orca\src\main\ipc\filesystem.ts:1927-1995`。
- 本地 push 命令按条件加入 `--force-with-lease` 和 `--set-upstream`：`REFERENCE_ROOT/orca\src\main\git\remote.ts:179-211`。

结论：CrewVela 首版至少应支持 Fetch、Pull、Push、Publish（首次 push 设置 upstream）和显式 Force Push with lease。Sync、Fast-forward、Rebase from Base 可以作为 P1 下拉操作，避免首屏按钮过多。

### 3.6 Git History：已实现，但以审查为主，不是分支管理器

- 历史只在用户展开 Commits 时加载，避免大仓库/远端仓库持续起 Git 进程：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\use-git-history.ts:74-110`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\use-git-history.ts:149-165`。
- 可以展开单个 commit 的文件、打开整提交或单文件 Diff：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\git-history-panel.tsx:88-152`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\git-history-panel.tsx:354-387`。
- Context menu 支持打开远端提交、复制 hash、复制 message、让 Agent 解释提交：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\git-history-commit-context-menu.tsx:13-56`、`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\sync\use-git-history-commit-actions.ts:190-289`。

当前 UI 没有在 commit history 上提供 reset、revert、cherry-pick 或 checkout。不要把后端存在的任意 Git exec 能力误算成已产品化操作。

### 3.7 分支能力边界：创建/选择分支不属于当前 Git 面板完整链路

- Git provider contract 确实有 `checkoutBranch`、`listLocalBranches` 和可选 `renameCurrentBranch`：`REFERENCE_ROOT/orca\src\main\providers\git-provider-contract.ts:47-48`、`REFERENCE_ROOT/orca\src\main\providers\git-provider-contract.ts:87-92`。
- Runtime RPC 也暴露 branch checkout/list：`REFERENCE_ROOT/orca\src\main\runtime\rpc\methods\git.ts:89-100`。
- 但 Source Control 顶部工具栏只接收过滤、创建 Review、列表/树、选择比较基准、刷新和 Notes 等 handler，没有 checkout/create branch handler：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\panel\header-toolbar.tsx:21-45`。
- 当前 Renderer 源码中没有对 `git.checkoutBranch` / `git.listLocalBranches` 的调用；它们主要是 Runtime/CLI 能力。Orca 的新分支主路径是创建 worktree，而自动 rename 走首次工作 hook。

结论：如果 CrewVela 要做传统 IDE 式 Branch Picker，不能简单认为“照搬 Orca Git 面板”即可；需要单独设计创建、切换、已被其他 worktree checkout、dirty tree、detached HEAD 等状态机。

### 3.8 PR / MR 创建：完整实现且支持多个 Forge Provider

Renderer 调用链：

1. Source Control store 绑定 `getCreationEligibility`、`createHostedReview`、`createStackedHostedReview`：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\listing\use-store-actions.ts:11-17`。
2. Review hook 检查 eligibility，构造 input，调用普通或 stacked create，并处理成功打开 URL、已有 Review 和部分创建成功：`REFERENCE_ROOT/orca\src\renderer\src\components\right-sidebar\source-control\review\use-hosted-review-creation.ts:66-211`。
3. Store 按 owner host 分派到 Runtime RPC 或本地 preload API：`REFERENCE_ROOT/orca\src\renderer\src\store\slices\hosted-review.ts:300-348`、`REFERENCE_ROOT/orca\src\renderer\src\store\slices\hosted-review.ts:351-370`。
4. IPC 重新做主进程 eligibility / 创建校验，再调用 source-control service：`REFERENCE_ROOT/orca\src\main\ipc\hosted-review.ts:111-163`。
5. 主进程在创建前校验 provider、认证、detached/default branch、dirty、upstream、sync、push、base 是否存在和重复 Review：`REFERENCE_ROOT/orca\src\main\source-control\hosted-review-creation.ts:338-423`、`REFERENCE_ROOT/orca\src\main\source-control\hosted-review-creation.ts:541-616`。
6. 最终 Forge adapter 调用真实 provider create：`REFERENCE_ROOT/orca\src\main\source-control\hosted-review-creation.ts:624-664`。

当前固定提交的 Forge adapter 都有真实 `createReview`：GitLab、GitHub、Bitbucket、Azure DevOps、Gitea，见 `REFERENCE_ROOT/orca\src\main\source-control\forge-provider.ts:88-116`、`REFERENCE_ROOT/orca\src\main\source-control\forge-provider.ts:154-198`、`REFERENCE_ROOT/orca\src\main\source-control\forge-provider.ts:200-264`、`REFERENCE_ROOT/orca\src\main\source-control\forge-provider.ts:266-303`。

PR 默认项也是真实消费：Draft、Use template、Generate details on open、Open after create；UI 在 `REFERENCE_ROOT/orca\src\renderer\src\components\settings\HostedReviewCreationDefaults.tsx:19-128`，默认均为 false：`REFERENCE_ROOT/orca\src\shared\source-control-ai.ts:40-46`。

结论：CrewVela 首版建议只做 GitHub，并把 Provider interface 设计成可扩展；不要为了“对齐 Orca”一次接入五个平台。先跑通 eligibility → publish → create draft PR → open URL → refresh linked PR 的单平台闭环。

## 4. 已实现、实验/占位与兼容项的明确区分

| 项目 | 判定 | 说明 |
|---|---|---|
| 分支前缀 | 已实现 | 创建 worktree 分支时真实消费 |
| 保持本地 main 最新 | 已实现、默认关闭 | 仅 worktree 创建时执行安全快进，不持续同步 |
| 变更分组顺序 | 已实现 | 纯 Renderer 展示偏好 |
| 默认对比基准 | 已实现 | 影响 committed-change compare，不改变 PR/Rebase 目标 |
| 自动重命名分支/worktree | 已实现、默认开启 | 受严格 eligibility 限制，非任意分支重命名 |
| Show Source Control AI actions | 已实现 | 默认显示；无 Agent 时仍无法生成 |
| Commit/PR instructions | 已实现但 UI 已演进 | 主路径是 Action recipe；旧 instructions 字段用于兼容/fallback |
| AI Commit / AI PR fields | 已实现 | 能追到 staged/branch patch、Agent 进程和结果解析 |
| AI 修复 Commit/Push/Checks/冲突/评论 | 已实现的 recipe/launch 能力 | 不等于 Agent 一定修复成功；属于高阶操作，不是首版刚需 |
| Stage/Unstage/Discard | 已实现 | 当前粒度为文件与批量路径；未证实行/hunk 暂存 |
| Fetch/Pull/Push/Force-with-lease/Sync/Rebase | 已实现 | 有 local、SSH、Runtime 路径 |
| PR/MR 创建 | 已实现 | 五类 Forge adapter 均有 createReview |
| Branch checkout/list | 后端能力 | Provider/RPC 存在，但 Source Control 面板没有 Branch Picker |
| Reset/Revert/Cherry-pick | 当前面板未实现 | Git History UI 没有这些操作 |
| 截图设置中的占位项 | 未发现 | 所列设置都存在真实消费链路 |

## 5. 建议的 CrewVela MVP 边界

### 5.1 P0：先完成一条短而完整的日常闭环

1. 面板状态：当前分支、upstream、ahead/behind、Changes/Staged/Untracked/Conflicts。
2. Diff：单文件 staged/unstaged Diff、相对默认 base 的 committed changes、整组 Diff。
3. Mutation：文件级与批量 Stage/Unstage；Discard 必须二次确认、路径边界校验，并区分 tracked/untracked。
4. Commit：message、快捷键、Commit、Commit & Push；错误完整回传，尤其保留 Git hooks 输出。
5. Remote：Fetch、Pull、Push、首次 Publish、显式 Force Push with lease。
6. Base：repository default / branch upstream；允许每个 worktree 临时 pin，不改变 PR/Rebase 目标。
7. AI：一个生成 Commit message 按钮；设置只保留 Agent、模型和 Commit instruction。
8. 自动建 worktree 时增加分支前缀，默认采用 CrewVela 现有 `codex/` 规则，并允许用户覆盖。

### 5.2 P1：再扩展发布和安全能力

1. GitHub Draft PR：标题、描述、base、模板、创建后打开。
2. AI PR title/body；与 Commit instruction 分开。
3. Git History：按需加载、单提交 Diff、复制 hash/message、打开远端 URL。
4. 创建 worktree 时安全更新本地 main，默认关闭，并完整复刻 Orca 的安全判断。
5. 冲突检测、Review、Abort merge/rebase；不要首版就让 Agent 自动执行所有修复。

### 5.3 P2：有真实需求再加

1. 分组顺序和列表/树视图偏好。
2. Diff Notes → 发送 Agent。
3. 自动根据 Agent 首次任务重命名分支/worktree。
4. 8 套完整 Action recipe 编辑器及 repo-level override。
5. GitLab/Bitbucket/Azure DevOps/Gitea、多远端 fork sync、stacked PR。
6. Branch Picker、checkout/create branch、cherry-pick/revert/reset；这些需要单独的安全设计，不能靠任意 Git 命令按钮快速补齐。

## 6. 关键设计警告

- 不要把“刷新本地 main”实现成无条件 `reset --hard origin/main`。Orca 在它前面有 ahead、ancestor、owner worktree、tracked-clean 和 OID CAS 多重门禁。
- 不要把普通 Push 自动升级成 Force Push。Orca 特意保持二者分离，并只提供 `--force-with-lease`。
- 不要虚构 `origin/main`。仓库可能是 `master`、多 remote、无 remote、fork 或自定义默认分支；Orca 在默认 base 解析失败时保持 null。
- 不要把 Diff compare base 与 PR/Rebase target 共用一个状态字段。
- 不要默认显示可点击的 AI 按钮却在点击后才发现没有 Agent/模型；配置有效性应在按钮旁可见。
- 不要把后端 `exec` 或 RPC 方法存在，等同于产品功能已完成。Branch checkout/list 就是当前 Orca 中“后端存在、Git 面板未产品化”的典型例子。
- 不要把旧截图的两个 instructions 文本框当成当前 Orca 的完整设计；当前主模型已是 operation-scoped Action recipe，旧字段主要承担迁移和 fallback。

# skills-manager 对 CrewVela 多 Runtime Skill 管理的借鉴分析

日期：2026-08-18
结论口径：源码审计，不以 README 宣传文字单独作为“已实现”证据。

## 结论

最适合融入 CrewVela 的不是 Skills Manager 整个产品，而是 3 个纵向能力：

1. **统一 Skill 资产库 + 现有安装发现/纳管**；
2. **面向 7 个 Runtime 的安全分发矩阵（全局/项目作用域）**；
3. **来源、版本、更新差异和撤部署所有权保护**。

CrewVela 已经有完整的 7 Runtime 注册、探测、启动、能力声明和协作 MCP 注入。它当前缺少的是 Skill 目录发现、来源元数据、跨 Runtime 部署状态和更新生命周期，不应再造第二套 Runtime 注册表。CrewVela 的现有 Runtime 列表与能力表见 [.\src-tauri\src\multi_agent\runtime.rs:27](.\src-tauri\src\multi_agent\runtime.rs:27)，协作 MCP 是运行时临时注入的另一条链路，见 [.\src-tauri\src\multi_agent\executor.rs:1664](.\src-tauri\src\multi_agent\executor.rs:1664)。

## 审计基线

- 目标仓库：`REFERENCE_ROOT/skills-manager`
- 固定提交：`e2e797c2a0a5c998a08f8c1ad7f38490f9343e62`
- 版本：`1.34.2`
- 提交时间：2026-08-17T00:41:37+10:00
- 分支：`main...origin/main`
- 工作树：`git status --short --branch` 除分支行外无输出，即无已跟踪或未跟踪改动。
- 限制：沙箱无法读取用户级 `%USERPROFILE%\.config\git\ignore`，因此 Git 给出 warning；仓库索引与工作树状态仍正常返回。
- 目标仓库没有 `AGENTS.md`/`CLAUDE.md`。已阅读 README（中英文）、`docs/skill-format-detection-spec.md`、package/Cargo manifests、内置 `manage-skills` Skill、前端 Tauri API、核心 Rust 模块和测试。

目标项目使用 React 19 + Tauri 2/Rust；SQLite、git2、reqwest、keyring、AES-GCM、notify、clap 均为真实依赖，而不是文档设想，见 `REFERENCE_ROOT/skills-manager\package.json:20` 与 `REFERENCE_ROOT/skills-manager\src-tauri\Cargo.toml:18`。

## 7 个 CrewVela Runtime 的 adapter 映射

Skills Manager 的 `ToolAdapter` 将“主部署目录、安装检测目录、额外只读发现目录、是否递归、项目目录”分开；额外目录明确只用于发现、不用于部署，见 `REFERENCE_ROOT/skills-manager\src-tauri\src\core\tool_adapters.rs:17`。CrewVela 应把这些字段并入现有 `AgentRuntimeKind` 的扩展元数据，而不是复制一套 `ToolAdapter.key`。

| CrewVela Runtime | skills-manager key | 用户级部署根 | 项目级根 | 额外发现/特殊语义 | 融合判断 |
|---|---|---|---|---|---|
| Claude Code `claude_code` | `claude_code` | `~/.claude/skills` | `<repo>/.claude/skills` | 平铺扫描 | 可直接借鉴；key 一致。证据：`tool_adapters.rs:161` |
| Codex `codex` | `codex` | `~/.codex/skills` | `<repo>/.codex/skills` | 额外发现 `~/.agents/skills`，但仍只部署到 `.codex/skills` | 可直接借鉴，但必须保留“发现根 ≠ 部署根”。证据：`tool_adapters.rs:190` |
| Kimi Code `kimi_code` | `kimi` | `~/.config/agents/skills`（Windows 还会尝试系统 config dir） | 默认回退为 `<repo>/.config/agents/skills` | adapter key 与 CrewVela 不同，且根是生态共享目录 | 需适配：做 `kimi_code → kimi` 映射，并在实现前用 Kimi Code 当前契约验证项目根，不能直接硬编码。证据：`tool_adapters.rs:409` |
| Pi `pi` | `pi` | `~/.pi/agent/skills` | `<repo>/.pi/skills` | 额外发现 `~/.agents/skills` | 可直接借鉴；必须保留全局/项目路径不对称。证据：`tool_adapters.rs:637` |
| OpenCode `opencode` | `opencode` | `~/.config/opencode/skills` | `<repo>/.opencode/skills` | 全局与项目根不同，已有单测 | 可直接借鉴。证据：`tool_adapters.rs:228`、`tool_adapters.rs:1084` |
| Hermes `hermes` | `hermes` | `~/.hermes/skills` | 默认 `<repo>/.hermes/skills` | `recursive_scan=true`，支持分类/嵌套 Skill | 可直接借鉴目录，但 CrewVela 必须保留递归发现；`Lobster` 只是 Skills Manager UI 分类，不应进入 CrewVela 领域模型。证据：`tool_adapters.rs:727`、`tool_adapters.rs:5` |

CrewVela 此处列举的 CLI 领域 key 包括 `claude_code/codex/kimi_code/pi/opencode/hermes`。历史数据中的 `oh_my_pi` 仅供读取，不能再用于部署。

### 安装探测不能照搬

Skills Manager 以配置目录存在判断 Runtime“已安装”，自定义/覆盖路径则直接判定可用，见 `tool_adapters.rs:130`。CrewVela 已通过可执行文件和版本探测 Runtime，见 [.\src-tauri\src\multi_agent\runtime.rs:120](.\src-tauri\src\multi_agent\runtime.rs:120)。融合后应采用：

- `runtimeInstalled`：CrewVela 可执行文件探测结果；
- `skillsRootDetected`：Skill 根是否存在；
- `skillsRootWritable`：是否可写；
- `skillsRootOverridden`：用户是否覆盖。

四者不可压成一个 `installed` 布尔值。

## 真实功能与调用链

### 1. Skill 发现与格式

真实实现：

- 只把精确命名的 `SKILL.md` 与兼容的 `skill.md` 当作 Skill；README/CLAUDE 不算，见 `skill_metadata.rs:31`、`skill_metadata.rs:78`。
- 平铺或递归扫描由 adapter 决定；递归扫描跳过 `.hub/.git/node_modules`，避免中央库软链回环，见 `scanner.rs:33`、`scanner.rs:161`。
- 发现结果记录 Runtime、路径、名称猜测、内容 hash；同名同 hash 的多位置副本聚合展示，见 `scanner.rs:88`、`scanner.rs:217`。
- 全局 Runtime 目录中的外部 Skill 可以纳入中央库；项目 Skill 也能双向导入/更新。项目扫描与同步状态包含 `in_sync/project_newer/center_newer/diverged/project_only`，前端类型见 `REFERENCE_ROOT/skills-manager\src\lib\tauri.ts:164`。

### 2. 安装与来源

真实来源：本地目录、`.zip`/`.skill`、任意 Git URL、skills.sh。前端到后端的已注册调用不是占位，见 `REFERENCE_ROOT/skills-manager\src\lib\tauri.ts:265` 与 `REFERENCE_ROOT/skills-manager\src-tauri\src\lib.rs:990`。

安装链路：

```text
UI/CLI 选择来源
  → clone / 解压 / 读取本地目录
  → 解析 SKILL.md frontmatter、规范化目录名
  → 复制进中央库
  → 计算内容 hash
  → SQLite 写 source_type/ref/subpath/branch/revision/hash
  → 写可重建 Git metadata
  → 用户另行选择 Runtime 部署
```

安装默认只进入中央库，不自动进 Preset 或 Runtime，源码有明确注释，见 `commands/skills.rs:878`。这条语义适合 CrewVela：安装与分发必须分开，避免从市场点一下就立刻影响全部 Agent。

归档导入有 Zip Slip 防护；复制时跳过 `.git/.DS_Store` 和符号链接，防止从 Skill 根外带入文件，见 `installer.rs:167`、`installer.rs:240`。

### 3. 多 Runtime 分发真实链路

核心数据模型将 Skill 本体与 Runtime target 分表：Skill 保存来源、commit、hash、中央路径；target 保存 `skill_id/tool/target_path/mode/status/source_hash`，见 `skill_store.rs:18`、`skill_store.rs:41`，SQL 唯一约束为 `(skill_id, tool)`，见 `migrations.rs:70`。

```text
Skill 中央副本
  → 选择 Runtime adapter + global/project scope
  → 计算目标目录
  → preflight 分类目标所有权
  → symlink / Windows junction / copy
  → 记录 (skill, runtime, target_path, mode, source_hash)
  → UI 卡片/矩阵显示真实部署状态
```

`sync_skill_to_tool` 进入共享 scenario service 做单 Skill 部署，见 `commands/sync.rs:35`。同步模式默认为 symlink；Windows 创建 symlink 失败后尝试 junction，再降级 copy，见 `sync_engine.rs:255`、`sync_engine.rs:323`。Copy 用上次 source hash 避免每次启动递归复制，见 `sync_engine.rs:384`。

撤部署不是盲删：如果 target 已被用户换成另一种对象，只删记录并保留文件；若多个 Runtime 共用同一路径，仍有其他 target 引用时保留实物，见 `commands/sync.rs:105`。这正适合 Kimi/共享 `.agents` 类目录。

### 4. 全局/项目作用域、启用/禁用

- **全局分发**：创建 target 记录，代表 Runtime 真正可见。
- **项目分发**：按 adapter 的项目相对路径写入当前 workspace；支持一次选择多个 Agent，见 `commands/projects.rs:937`。
- **项目禁用**：把目录在 `<skills-root>` 与 `<skills-root>-disabled` 之间移动，见 `project_scanner.rs:45`、`commands/projects.rs:1111`。这只是 Skills Manager 自己的文件约定，不是 Runtime 原生统一协议。
- **Runtime 全局禁用**：会撤下该 Runtime 的全部受管 target；重新启用只会同步旧的 active scenario，见 `commands/tools.rs:118`。

CrewVela UI 应使用“部署/撤下”表示 Skill 是否对某 Runtime 可见；不要复用“启用/禁用 Agent”，也不要把 MCP capability 与 Skill 可见性混为一谈。

### 5. 更新、版本与来源检查

版本模型不是 SemVer 包版本，而是：

- Git/skills.sh：`source_revision`（已安装 commit）与 `remote_revision`；
- 本地/import：目录内容 SHA-256；
- 部署 Copy：上次分发时的 source hash。

更新检查默认 60 分钟 TTL；“检查全部”按 `(clone_url, branch)` 合并同一仓库请求、并发查询，网络阶段不占中央库锁，见 `commands/skills.rs:1270`、`commands/skills.rs:2635`。

更新会 clone 到临时目录、checkout 精确 revision、stage 新目录、比较内容 hash，再交换中央目录。若新版本会删除现有路径，则返回删除列表，不应用更新；确认 token 绑定“revision + 精确删除列表”，避免确认窗口内源又变化，见 `commands/skills.rs:1868`。批量更新无人可问时标记 `held_back`，见 `commands/skills.rs:1567`。

关键缺口：它只能发现“路径消失”；如果用户修改了仍存在的同名文件，更新会静默覆盖。内置 Skill 文档也明确提示这个限制。因此 CrewVela 应在 P0 更新流程里增加“本地修改 vs 上游”的文件级 diff/三方状态，而不是只复制删除保护。

### 6. 删除与批量操作

真实支持批量删除、更新、标签编辑、Preset 成员开关、向多个 Runtime/项目批量添加，UI 调用见 `REFERENCE_ROOT/skills-manager\src\views\MySkills.tsx:555`、`MySkills.tsx:609`、`REFERENCE_ROOT/skills-manager\src\components\AddSkillsSheet.tsx:329`。

删除中央 Skill 会撤下所有 Runtime target、删除中央目录与 DB 行，见 `commands/skills.rs:739`。但这条路径对 target 调用了无条件 `remove_target` 且忽略部分错误，安全性低于日常 undeploy 的 `remove_recorded_target`，不应原样照搬。

### 7. 市场、搜索、导入导出

- skills.sh 总榜/趋势/热门与关键词搜索是真功能：榜单缓存 5 分钟，搜索 limit 限制 1–300，后端直接请求 skills.sh，见 `commands/browse.rs:10`、`skillssh_api.rs:217`。
- 市场安装实际转换为 GitHub repo + skill subpath，再按 Git Skill 安装，见 `commands/skills.rs:1006`。
- CLI 支持从 Runtime 目录 adopt、任意目录一次性 export、`--dry-run`、机器可读 JSON 和显式 `--yes` 删除；命令定义见 `skills-manager-cli.rs:77`。
- 项目工作区支持中央库与项目双向导入/更新/导出，见 `commands/projects.rs:824`。

### 8. Preset

Preset 是命名 Skill 集合；成员关系与真实部署分开。新版 deploy/undeploy 是加法式，旧的 active scenario/sync 是排他式兼容路径。数据库仍使用 `scenario` 命名，见 `migrations.rs:127`。

CrewVela 可以借用“Skill 集合 + 多 Runtime target”能力，但产品上应叫“Skill 配置集”或挂在现有协作模板上；不要引入第二个“活动 Preset”，否则会与项目协作成员、Runtime 选择和权限模板形成三个互相覆盖的状态源。

### 9. Git 备份、多设备冲突与版本恢复

这部分是真功能，不是占位：

- 120 秒静默后自动 commit/push，见 `auto_backup.rs:28`。
- full sync 是单锁事务：commit → fetch/object merge → snapshot → push，并对 non-fast-forward 重试 3 次，见 `commands/git_backup.rs:425`。
- 同一 Skill 冲突保留本地并进入 pending；用户可 `keep_local/use_remote/keep_both`，决议前先建安全快照，见 `commands/git_backup.rs:534`。
- 版本列表和恢复存在真实命令，恢复前也建安全点，见 `commands/git_backup.rs:624`。
- 单 Skill 超过 100 MiB 自动排除，仓库超过 1 GiB 警告，见 `core/git_backup.rs:1147`。

这套 object-aware merge 很完整，但体量大、与 CrewVela 核心多 Runtime 分发无直接依赖，适合 P2，不适合先移植。

## 安全与信任边界

已有安全措施：

- Skill 名跨平台清洗，阻止路径穿越和 Windows 设备名，见 `skill_metadata.rs:90`。
- Zip Slip 防护、导入时不跟随 symlink。
- 部署前区分缺失、自有链接、外部链接、真实目录、真实文件；默认 no-clobber，见 `sync_engine.rs:89`。
- Git backup token 从 URL 剥离到 OS keychain，通过 askpass 环境变量临时注入，见 `git_credentials.rs:1`。
- `proxy_url` 与 `git_backup_remote_url` 在 SQLite 中 AES-256-GCM 加密，见 `skill_store.rs:10`。

没有实现的安全能力：

- 没有 Skill 发布者签名、来源信誉、commit pin/allowlist 策略；
- 没有检查 SKILL 指令中的 prompt injection；
- 没有扫描 scripts 的危险命令、网络/文件权限需求；
- 没有“该 Skill 可在哪些 Runtime/权限档运行”的兼容清单；
- 市场安装前没有统一信任确认。

因此 CrewVela 不能照搬“一键市场安装后部署”。至少应在首次部署前展示：来源 URL、commit、文件清单、可执行脚本、声明/推断权限、目标 Runtime、全局/项目作用域；来自未信任源的 Skill 默认只进中央库，不自动部署。

## 文档声明与真实实现差异

| 声明 | 审计结果 |
|---|---|
| “Marketplace + SkillsMP AI search” | **部分失实/陈旧**。skills.sh 榜单与关键词搜索真实存在；仓库中没有 SkillsMP API key 后端、设置项或 UI 调用。README 声明见 `README.md:55`，真实 browse API 只有 `fetch_leaderboard/search_skillssh`，见 `commands/browse.rs:12`。 |
| “52 agents” | adapter 列表确实很大，但只是目录/检测规则，不代表对每个 Runtime 的 Skill 语义做过深度兼容。CrewVela 只应支持自己的 7 个 Runtime。 |
| “版本” | 是 Git commit/snapshot/content hash，不是包管理器式 SemVer 依赖解析。 |
| “权限/安全” | 有文件系统与凭据安全，没有 Skill 内容信任/权限模型。 |
| “启用/禁用” | 全局是受管 target 存在与否；项目是移到 `-disabled` 目录；不是统一 Runtime 原生开关。 |
| “Preset 同步” | 新路径是加法部署；仍保留旧 active scenario 排他同步，二者共存。 |

## 与 CrewVela 当前能力去重

| 能力 | CrewVela 当前状态 | 是否借鉴 |
|---|---|---|
| 7 Runtime 注册、可执行文件/版本探测、能力声明 | 已有，且是 CrewVela 核心领域模型，[runtime.rs:27](.\src-tauri\src\multi_agent\runtime.rs:27) | 不复制 Tool registry，只扩展 Skill roots 元数据 |
| Runtime 会话启动、恢复、中断、权限模式 | 已有 | 不属于 Skill Manager 融合范围 |
| CrewVela 协作 MCP 注入 | Claude/Codex/Kimi 有实际注入；其他 Runtime 当前为空分支，[executor.rs:1664](.\src-tauri\src\multi_agent\executor.rs:1664) | 与 Skill 分发分层，不合并为一个开关 |
| Skill 根发现、中央库、来源/commit/hash | 未发现现有实现 | P0 新增 |
| Skill→Runtime 全局/项目部署矩阵 | 未发现现有实现 | P0 新增 |
| Skill 更新/diff/安全撤部署 | 未发现现有实现 | P0/P1 新增 |
| 项目协作模板/Agent 组合 | CrewVela 已有 | 用作 Skill 配置集的挂载点，不另造第二套 Preset 导航 |
| Git 多设备 Skill 备份 | 未发现现有实现 | P2，可后置 |

## 推荐路线图

### P0：必须先做

#### P0-1 统一 Skill 资产库与发现/纳管

范围：

- 在 CrewVela 现有 7 Runtime 定义上增加 `globalSkillsRoots`、`projectSkillsRoot`、`scanMode`、`discoveryOnlyRoots`；
- 扫描全局与当前项目的 `SKILL.md/skill.md`；
- 用内容 hash 聚合同一 Skill 在多个 Runtime 的副本；
- 提供“纳入管理”，保留原始来源未知状态；
- UI 先做“Skill 中心”列表 + 来源/路径/部署角标，不先做市场。

直接借鉴：`SkillRecord`/`SkillTargetRecord` 分离、平铺/递归扫描、discovery-only root、adopt。
必须适配：key 统一使用 `AgentRuntimeKind`；Runtime 安装探测使用 CrewVela probe；中央库位置纳入 CrewVela app data，而不是另建 `~/.skills-manager`。

#### P0-2 Runtime 安全分发矩阵（全局/项目）

范围：

- 一个 Skill 可选择多个 Runtime 和 `global/project` scope；
- 默认 symlink，Windows junction，再降级 copy；
- target 写入前 no-clobber；
- 撤部署只删除仍符合记录类型的对象；共享 target path 有引用计数/幸存者检查；
- UI 显示目标路径、模式、健康状态和失败原因。

这是 Skills Manager 最适合 CrewVela 的核心功能。CrewVela 天然知道会话属于哪个 Runtime 和项目，能比原项目更准确地把“协作成员组合”翻译成 Skill target 集合。

#### P0-3 来源、更新与本地修改保护

范围：

- 安装记录 `source_type/url/subpath/branch/commit/content_hash`；
- 检查更新与应用更新分开；
- 更新前展示文件 diff；
- 删除路径与同路径本地修改都要保护；
- approval token 绑定 commit + diff 摘要；
- copy target 更新后重新分发，symlink target 自动跟随中央库。

直接借鉴：同仓库 revision 请求合并、stage/swap、held-back removals。
必须增强：同路径文件修改也必须进入冲突/确认，弥补原项目会静默覆盖的缺口。

### P1：核心闭环后做

#### P1-1 Skill 配置集与批量分发

把 Preset 的“命名 Skill 集合 + 多 Runtime target”能力挂到 CrewVela 现有项目协作模板/Agent 组合：启动项目时提示应用配置集，用户确认后做一次性部署；不做后台永久强制同步。支持批量部署、撤下、标签、更新。

#### P1-2 市场/本地/Git 安装入口 + 信任门

复用本地目录、zip/skill、Git、skills.sh 搜索与 Git 多 Skill preview；增加 CrewVela 自己的首次信任门：来源、commit、脚本、文件、目标 Runtime、作用域、权限风险。skills.sh 安装量只作为参考，不当安全结论。

### P2：可选增强

#### P2-1 Git 版本历史、多设备同步和 object-aware 冲突

先提供本地快照/回滚，再考虑私有 Git 远端、自动备份、keep mine/use remote/keep both。原项目实现可作为算法参考，但不建议直接复制整套 Tauri command、SQLite metadata 投影和旧协议兼容层。

## 不应照搬

1. **不要复制 52 个 adapter 与自定义 Agent 管理 UI。** CrewVela 的 7 个 `AgentRuntimeKind` 才是唯一事实源；否则 Runtime、会话和 Skill 会出现双重身份。
2. **不要照搬 SkillsMP AI 搜索宣传。** 当前提交没有实现，应删除这项预期或另立需求。
3. **不要把配置目录存在当作 Runtime 已安装。** 使用 CrewVela 的可执行文件 probe，并独立显示 Skill 根状态。
4. **不要照搬项目 `-disabled` 目录作为统一开关。** 它不是 7 个 Runtime 的共同标准；CrewVela P0 先用“部署/撤下”。
5. **不要照搬旧 active scenario 排他同步。** 采用加法式 Skill 配置集，避免切换一个配置集时意外撤下其他项目/Agent 需要的 Skill。
6. **不要照搬删除中央 Skill 的宽松删除路径。** 所有删除都走 recorded ownership + preflight + 共享路径幸存者检查，失败必须可见。
7. **不要允许未信任市场 Skill 一键部署到全局。** 原项目没有内容安全/权限模型；CrewVela 有 Auto/Yolo 等高权限运行模式，风险更高。
8. **不要先移植 Git 多机同步。** 它的复杂度和旧协议兼容成本远高于首版价值；先完成中央库、分发矩阵和安全更新。
9. **不要把 MCP 与 Skill 合并成同一状态。** MCP 是运行时连接注入，Skill 是文件发现；CrewVela 的 Kimi MCP 还会临时改写并恢复 `.kimi-code/mcp.json`，见 [executor.rs:1913](.\src-tauri\src\multi_agent\executor.rs:1913)。

## 建议的数据与模块边界

```text
AgentRuntimeKind（CrewVela 唯一 Runtime 身份）
  └─ RuntimeSkillAdapter
      ├─ global deploy roots
      ├─ discovery-only roots
      ├─ project root
      └─ flat/recursive scan

SkillAsset
  ├─ source provenance + pinned revision/hash
  ├─ trust state + inspected risk summary
  └─ central content path

SkillDeployment
  ├─ skill_id + runtime_kind + scope + project_id
  ├─ target_path + materialization mode
  └─ source_hash + health + ownership proof

SkillSet
  └─ skills + desired Runtime/scope targets
```

这个边界比原项目更适合 CrewVela：Runtime adapter 归多 Runtime 执行层所有；Skill 资产归资产层；分发是两者之间的应用服务；MCP 仍留在运行时连接层。

## 测试与可信度

目标仓库核心逻辑有大量 Rust 单元/集成测试，覆盖格式识别、路径清洗、adapter、扫描、symlink/copy/junction、目标所有权、更新删除保护、Git merge/冲突/恢复、CLI deploy/undeploy。典型证据：

- OpenCode adapter 路径测试：`tool_adapters.rs:1084`
- 同步所有权决策测试集中在 `sync_engine.rs:614`
- 多设备 merge 集成测试从 `merge/integration_tests.rs:222` 开始
- CLI Skill/Preset 部署往返测试：`skills-manager-cli.rs:3120`

本次执行 `cargo test --locked`：Rust lib 测试共 440 项，**438 通过、2 失败**。失败项是 `remove_workspace_skill_target_removes_directory_symlink_without_touching_target` 与 `remove_target_removes_directory_symlink`；两者都在测试创建 Windows 目录 symlink 时收到 OS error 1314“客户端没有所需的特权”，并非业务断言失败。由于 lib test target 返回非零，Cargo 未继续执行后续 binary test target，因此不能表述为“全套测试通过”。这也再次说明 CrewVela 在 Windows 上必须保留 junction/copy fallback，且测试应对无 symlink privilege 环境做条件跳过或使用 junction fixture。

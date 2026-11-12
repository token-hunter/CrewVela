# 全项目代码审计与整体优化（2026-11-11）

## 起点与范围

从 `main`（`88351ee`）切出 `chore/codebase-audit-optimization`，验证通过后合并回 `main`。先在未改动的 main 副本上取得基线：`tsc`、Node 测试 467（462 通过、5 跳过）、Rust 库测试 1053 通过、前端构建，以及完整浏览器 UI 套件（main 原有 51 个脚本，35 通过、16 失败；新写的 `modalOverResizers` 在该副本上同样失败）。之后用 CodeGraph 梳理结构，再在 820×540、1280×800、1920×1080，明暗主题及其余四种主题下逐页截图并用布局扫描脚本检查溢出、重叠、裁切，对照基线失败集合区分「产品缺陷」与「测试自身过期」。

本轮只做前端与测试的缺陷修复，Rust 未改动。不是对所有运行环境或未知缺陷的零缺陷保证。

## 已确认的问题与修复

| 问题 | 修复及验证方式 |
| --- | --- |
| 左栏拖拽条层级（40）高于弹窗遮罩（20），弹窗覆盖区域内的点击被拖拽条截走 | 拖拽条降到 `--z-dock-max`；新增 `modalOverResizers`，基线失败、修复后通过 |
| 首次使用的「会话通信」提示卡层级 40，盖在新建会话等弹窗之上并截走点击 | 提示卡降到 `--z-dock-overlay`；新增 `hintBelowDialogs`（687px 与 1280px，真实点击落到弹窗内） |
| 设置对话框、技能运行时弹窗各自写死 `#1479ed` 与白字，覆盖全局主题强调色，其他主题/强调色下主按钮颜色和对比度不一致 | 删除写死色值，沿用 `--accent` / `--on-accent`；`uiThemeContrast` 与相关设置用例恢复通过 |
| 模型设置服务行：操作按钮越出卡片，「编辑」被挤成竖排 | 末列按内容宽度、按钮不换行，紧凑布局断点 1000px→1100px；新增 `providerRowLayout`，覆盖 820–1600px 九个宽度 |
| 窄窗格里会话标题被 Claude 桌宠和状态文字挤成 0 宽 | 容器查询：窗格 ≤340px 先隐藏 Claude 桌宠，≤300px 状态文字收成色点（保留悬停说明），副标题先于标题收缩；新增 `paneHeaderNarrow`，基线标题宽度为 0 |
| 设置 › 快捷指令：搜索框双层边框、与页面标题重复的说明、最小窗口首屏看不到分类页签 | 单层边框并与「重新检测」同行，去掉重复说明，内嵌时不再限制主体高度；新增 `quickCommandsSettingsLayout`（1280×800 与 820×540） |
| 空工作区只剩一个孤零零的按钮 | 增加标题、说明、主操作与提示，窄窗口不裁切；新增 `emptyWorkspaceState` |
| 通知设置文案写死 Windows 行为，macOS 下误导 | 文案按平台生成；新增 `notificationPlatformCopy` |
| 电脑控制设置里 macOS 专属说明没有样式 | 增加 `runtime-note` 样式 |
| 总控侧栏信任状态每 250ms 无条件调用 IPC 并重渲染整个侧栏 | 仅在有会话启动或等待信任时保持 250ms，空闲 1s，页面隐藏不请求，内容不变不更新；`controlTrustPolling` 单元测试 |
| 常驻的 2s（Agent 接力记录）与 5s（token 用量）轮询每次都写入新对象，空闲时整棵 App 约每秒提交 1.2 次 | 新增 `keepIfUnchanged`（结构相等，键顺序无关，因为 Rust HashMap 序列化顺序会变）；空闲提交 14 次/12s → 0；`pollIdleRender` 在守卫关闭时失败（6s 内 8 次），内容变化仍会更新界面 |

测试自身的过期假设一并修复，不属于产品缺陷：默认浏览器通道在 macOS 不可用（改用 Chrome 通道，可由 `CREWVELA_BROWSER_CHANNEL` 覆盖）、若干用例写死 Windows 形态（固定平台）、文案与可访问名称已更新（「选择会话」「关闭设置」「内置电脑控制暂未适配」）、`skillsRuntimeFirst` 改用平台对应的 End 键并等待平滑滚动、`collaborationAsync` 补齐自动化与浏览器片段。

## 验证记录

| 验证 | 实际结果 | 日志 |
| --- | --- | --- |
| `tsc --noEmit` | 退出码 0，无输出 | `.scratch/codebase-audit-optimization/tsc-final.log` |
| Node 完整测试 | 473 项：468 通过、0 失败、5 跳过（基线 467 / 462 / 0 / 5，新增 6 项全部通过） | `node-final.log` |
| 浏览器 UI 完整套件 | 59/59 脚本通过，在全新启动的 Vite 上执行（main 原有 51 个脚本基线为 35 通过、16 失败；本轮新增 8 个脚本） | `ui-suite-final.log` |
| 前端构建 | `tsc && vite build` 通过；主 chunk 1,429 kB（基线 1,429 kB），大 chunk 提示仍在 | `build-final.log` |
| Rust | 未改动，沿用基线 1053 通过、0 失败、40 忽略，本轮未重跑 | `rust-baseline.log` |

除 `pollIdleRender` 外，新增的 UI 回归均在未修复的 main 副本上先观察到失败，再于修复后通过；`pollIdleRender` 依赖 main 没有的夹具钩子，改为临时关闭 `keepIfUnchanged` 守卫验证会失败（6 秒内 8 次多余提交），确认后已恢复。原始日志、截图与审计脚本保留在被 Git 忽略的 `.scratch/codebase-audit-optimization/`。

## 验收边界

- 浏览器 UI 使用项目已有的 `?ui-preview=1` 预览数据与隔离 IPC fixture，证明前端显示和交互；不是真实 Tauri 窗口、真实账户或模型服务的端到端验收。只在 macOS 上执行，Windows 与 Linux 原生界面未重新检查。
- Rust 没有改动，沿用基线 1053 通过，本轮未重跑。
- 左栏拖拽条现在位于弹窗遮罩之下、右侧栏之上：弹窗打开时不能拖动侧栏宽度（模态应有的行为），面板最大化时仍可拖。
- 基线与中途各留有一轮失败日志，没有删除断言或忽略异常来获得通过：首轮 UI 套件因 macOS 没有 `timeout` 命令全部误报失败（已换成 `perl alarm`）；一次 `sharedMemorySettings` 失败来自长时间运行的 Vite 依赖预构建状态（重复 React），重启 Vite 后恢复，不是代码回归。
- 本轮发现但未处理，仅作建议：前端主 chunk 约 1.4 MB 并伴随构建提示；数张 500–800 KB 的 PNG 头像/皮肤资源可压缩；`App.tsx` 内 `UI_PREVIEW` 分支较多；部分测试仍带 Windows 假设。

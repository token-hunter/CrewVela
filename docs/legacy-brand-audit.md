# CrewVela 品牌清理与兼容迁移验收（2026-11-08）

当前产品源码、包与库标识、界面说明、内置 Skill、新 EXE 和 Windows 发布者已统一为 CrewVela；技术名称使用 `crewvela`。配置、WebView 数据、凭据与 Skill 通过兼容迁移继续使用。主工作区及两份公开源码副本均完成验证。未提交、未推送，保留开始时已有的未提交修改。

## 构建结果

按用户指定命令在主工作区执行，未为本次构建覆盖 `CARGO_TARGET_DIR`：

```powershell
npm run tauri -- build --no-bundle
```

- 默认产物：`REPO_ROOT\src-tauri\target\release\CrewVela.exe`。
- 当时的 ProductName / FileDescription 含旧中文副标题；CompanyName：`CrewVela`。当前名称以打包配置为准。
- 文件大小：43,573,248 字节。
- SHA256：`AFFE3910F4AC6E9E21D7D5E00A6BB6F42C07BC337A5E91D631279C42C7B70B22`。
- 已实际启动此文件验证，生产资源内嵌，不依赖 Vite。程序返回原有工作区，未停止用户会话。
- README、CLAUDE 和 AGENTS 的 Windows EXE 打包约定均采用该命令。它只生成程序，本轮没有重新制作 MSI / setup 安装器。

## 名称和迁移

| 对象 | 当前行为 | 保护与兼容 |
| --- | --- | --- |
| npm / Cargo | 包名 `crewvela`，Rust 库 `crewvela_lib`，作者、发布者 `CrewVela` | 同步入口与锁文件，无依赖版本升级 |
| 应用身份 | `com.crewvela.app`，Windows 通知使用同一身份 | WebView 建立前接入旧 profile，保留 localStorage 等数据 |
| 配置根 | `%APPDATA%\CrewVela` | 旧根 `cc-launcher` 存在时用 junction 指向同一底层数据；不复制正在写入的 SQLite/WAL、邮箱或锁 |
| WebView profile | `%LOCALAPPDATA%\com.crewvela.app` | 本机已建立到原 `com.cclauncher.app` 的 junction，原文件保留；其他平台使用对应目录及链接策略，原生未验收 |
| 双目录冲突 | 新旧路径指向不同数据时停止并报错 | 不自动合并、不覆盖、不悄悄创建空配置；失效路径或权限失败明确报错 |
| 凭据 | 优先系统凭据服务 `CrewVela`，缺失则从原 `cc-launcher` 读取并复制 | 复制失败保留可用旧值并提示重试；更新已有旧条目时同步两处，失败回滚；删除先清理旧条目，防止回退复活 |
| 通信 Skill | 源码目录 `skills/crewvela`，frontmatter 名 `crewvela`；启动时同步 Claude / Agents / Codex 的 Skill 根 | 只更新已知内置版本，留备份；自定义及 Skill 中心管理内容不覆盖；已安装旧入口保留兼容，已识别的旧指南更新正文 |
| 外观与本地设置 | `crewvela-appearance` / `crewvela-theme` 等新键 | 在导入 App 前迁移已知旧命名空间；新值优先，冲突和失败保留原值，失败阻止默认配置自动保存 |
| 消息 / hook | `_crewvela_app_session_id` / `_crewvela_queued_at` | 接受旧 `_cc_launcher_*` 字段，排队输出兼容字段；身份或时间冲突拒绝消费并保留事件 |
| 其他旧数据 | 保留现有项目根、用户标题、共享记忆项目 ID 与自定义设置 | 已有 Git 默认前缀、受管 Skill 所有权标记和生成的品牌引用使用已有精确迁移；不批量改用户内容 |
| MCP | 内置自有命令能识别旧 EXE 名并更新 | 保留自定义命令、参数和启用状态 |

本机实际启动检查：22 个会话（21 AI、1 终端）、原有项目与工作区 7、侧栏宽度和主题仍存在；6 个 Provider 显示凭据已设置，已有服务凭据可读取。共享记忆服务 v2.6.0 运行中，原有 173 页 / 176 会话 / 36,464 观察可见。未输出密钥值、未重装记忆服务、未改写用户项目路由。Codex 接入仍显示“待检查”，此界面状态不代表真实共享记忆捕获已验收。

## 历史产物处理

此前检查基线为：主工作区 87 个文件 / 332 处旧名称匹配、176 个旧名路径、公开源码副本 201 个文本文件命中。该统计是清理前记录，并非当前残留数。

64 个旧产物（原件共 1,525,741,191 字节）已归档到：

`output/archives/legacy-builds-20261108-06cb7377.zip`

归档含原始路径、大小、修改时间与逐文件 SHA256 清单 `ARCHIVE_MANIFEST.json`。CRC 与逐文件摘要复核通过后才移出原目录；没有仅把旧二进制改名后作为新版发布。归档包含旧 CC Dock portable/setup/MSI、旧 launcher EXE、旧发布者元数据的历史 CrewVela EXE，以及相关证据。原安装器留在归档，不能用作本轮新版安装器。

为保护已有快捷方式、Skill 和 hook 中的有效路径：

- 10 个原有 `output/CrewVela-日期.../CrewVela.exe` 路径和已有验证 release 入口已接入当前构建的硬链接，没有新增日期交付目录；旧说明文件加注当前摘要与历史记录边界，原文另有备份。
- `src-tauri/target/release/cc-launcher.exe` 仅保留一个同内容兼容硬链接，供旧命令引用使用；它与当前主 EXE 内容一致，产品属性已是 CrewVela。
- 3 份历史交付内置 Skill 目录已迁移为 `skills/crewvela`，指南更新为当前内置版本。
- 硬链接表示本次构建快照，不是今后每次构建自动同步旧交付目录的机制；后续一律以默认目录产物为准。

## 发布源码副本

`output/CrewVela-open-source` 与 `output/CrewVela-public` 均同步了品牌及所需迁移代码，保留各副本原有实现、公开许可和素材处理；未拿主工作区整体覆盖副本。两个独立仓库的原索引和提交历史未重置、未暂存本轮改动。验证中发现的移植编译错误已修复，再次运行完整测试通过。

## 验证

| 范围 | 结果 |
| --- | --- |
| 主工作区 Node 全量 | 401 通过，1 跳过，0 失败 |
| 主工作区 Rust 库全量 | 1036 通过，38 忽略，0 失败 |
| 迁移 / 品牌针对性 Node 回归 | 9 / 9 通过；覆盖已知命名空间、原值保护、失败重试与脚本目录发现 |
| 真实 Claude Code ↔ Codex 终端回归 | 4 轮连续双向投递全部得到对应模型的实际回复，忽略验收用例单独启用并通过 |
| open-source 副本 | Node 379 通过 / 1 跳过；Rust 925 通过 / 25 忽略；tsc + Vite 构建通过 |
| public 副本 | Node 379 通过 / 1 跳过；Rust 917 通过 / 25 忽略；tsc + Vite 构建通过 |
| 用户指定 Windows EXE 构建 | 通过，默认路径、名称、版本属性与摘要已核验 |
| Windows 实际桌面 | 旧工作区、会话、凭据及共享记忆服务保留，新程序可见并可返回主界面 |
| 候选上传文件密钥扫描 | 主工作区 789 文件、open-source 652 文件、public 649 文件，gitleaks 均 0 命中；合成样例的准入与应拦截对照均通过 |
| 品牌回归 | 主工作区及两副本的当前源码检查通过；只允许声明行及本报告引用历史身份 |
| Git 差异格式 | `git diff --check` 通过；行尾转换提示保留 |

真实终端验收使用独立测试会话和目录，未向用户现有会话发消息；它验证真实收件及模型回复，不声称本轮同时验证了模型主动调用 MCP/Skill 的全部组合。常规忽略测试包含外部或显式验收项目，未把忽略算作通过。

本地证据位于 `.scratch/brand-migration/`：`build.log`、`node-full.log`、`rust-final.log`、`real-cli-current.log`、两副本 `*-current.log` / `*-frontend.log`、`installed-skill-verification.json`、`compat-exe-entries.json`、`native-memory.png`、`archive-result.json`。密钥扫描报告已脱敏，不复制用户配置或凭据到仓库。

## 有意保留与验收边界

- 唯一兼容常量集中在 `brand.rs`、共享记忆脚本的旧根回退声明；旧协议键、旧凭据服务、物理旧目录、旧 Skill frontmatter 和上述 EXE 兼容入口属于迁移机制。不能删除这些后又声称旧集成仍兼容。
- 工作目录仍是 `REPO_ROOT`，已有项目名称由用户保存，未强行重命名实际仓库或用户项目。第三方 CC Switch 等名称保持原样。
- Git 历史、归档 ZIP、`.scratch` / `tmp` 历史快照、旧 Cargo 编译缓存和图片内历史文字不重写；它们不属于当前发布源码或新版程序。编译缓存仍可能出现旧 crate 文件名及调试 EXE，不作为交付文件。
- macOS/Linux 原生迁移、通知点击及安装器升级链路未实测。本轮按 `--no-bundle` 生成 EXE，未宣称 MSI 安装/卸载身份迁移通过。
- Rust 原有未使用代码等警告、Vite 大 chunk 和 bundle identifier 的 `.app` 提示仍保留；未通过关闭检查制造通过。
- 密钥扫描覆盖当前 Git 候选文件，不等于历史提交或所有本地忽略数据的安全证明。本轮没有推送任何文件到 GitHub。

## 品牌提交范围复核

用户随后要求仅提交本次旧名称清理和兼容迁移，暂不推送。按迁移前快照剥离此前的通信修复与首次提示功能，仅保留品牌迁移所需的实例锁依赖；其余既有改动仍在工作区。本次主仓库提交不包含忽略的 EXE、归档和两份独立发布仓库的提交。

对拆分后的精确提交树独立验证：Rust 1028 通过 / 38 忽略，Node 400 通过 / 1 跳过，tsc + Vite 构建通过，gitleaks 零命中，差异格式检查通过。前述 1036 / 401 及四轮通信结果记录的是迁移完成时含既有改动的完整工作区，不能据此认定这些既有改动全部进入本次品牌提交。验证日志见 `.scratch/brand-commit/`。

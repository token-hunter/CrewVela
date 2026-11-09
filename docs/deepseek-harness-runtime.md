# DeepSeek Harness runtime

CrewVela 的协作项目和工作流支持 `deepseek_harness`。它启动 Harness 原生 `dsh --profile acp`，通过 ACP 调用已有 Agent、工具和持久会话；不会重新实现 Agent，也不会降级到 PTY。独立使用入口见下文，采用 CrewVela 自己的 ACP 会话面板。

## 独立 ACP 会话（2026-10-03）

“更多工具 → 新建会话 → DeepSeek Harness”选择工作目录后，左侧创建真实会话，中央显示 CrewVela 自己的聊天面板。消息、流式回复、模型选择、工具状态和逐次审批均通过 `dsh --profile acp` 调用 Harness。不会打开 Harness 独立窗口或嵌入其 Web 页面，也不创建虚假 PTY ID；现有团队 ACP 执行保持独立。

模型和推理等级选项来自 Harness 的 ACP configOptions，模型与凭据沿用 Harness 自身配置，不使用 CrewVela Provider 密钥。独立会话初始使用 `workspace-write`；权限选择通过 Harness 的 permissionPresets 服务作用于当前会话。需要人工审批时等待“允许一次”或“拒绝”，不会默许 `allow_always`。插件管理、自动化任务及其他 profile 的管理界面不包含在此入口。

每个会话有独立 ACP 进程及恢复 ID，运行期间持有 Runtime lease。全局禁用后不能启动/恢复；运行时禁止禁用。消息历史、附件引用、目录、权限和 ACP 恢复 ID 持久化在配置目录的 `deepseek-harness-sessions.json`；新增字段保持旧历史兼容，附件不复制文件内容。停止、取消、归档和退出会清理连接，恢复沿用原 ACP 会话；上游恢复失败明确报错，不创建新会话冒充恢复。删除会话同步删除本地 Harness 历史；旧失败启动记录即使没有恢复定位也可删除。重启后工具卡片/审批依赖新的运行事件，持久化历史保存用户消息与文本回复。

### 实时回答与思考显示

安装版 Harness 的 ACP assistant 更新来自落盘消息，不能单独提供实时首片段。CrewVela 的临时控制插件另订阅公开 `agent/assistant-stream` 事件，经令牌验证的独立 TCP 连接实时投影每次模型尝试；首片段即时显示，后续最多按 100ms 合并。`session/event` 的规范结果用于最终对账，ACP 工具、权限和配置更新继续沿用原通道，避免回复重复。

实际暴露的 reasoning 与回答分别保存和显示，生成时思考默认展开，完成后可折叠。上游不提供 reasoning 时不显示思考区，也不生成替代内容。重试失败、未完成和取消的部分内容有独立状态；连接断开明确报错并释放旧连接，恢复创建新的订阅。历史新增可选字段兼容原有记录。此功能未执行真实模型端到端请求。

实时显示验证：原生会话回归 13 项通过、2 项默认忽略，包含生成中已看到 reasoning/回答首片段、最终 ACP 对账无重复、取消部分内容保留和流断开报错及恢复。控制桥回归通过，覆盖 100ms 尾刷新、失败重试分离及规范结果对账；真实安装版无模型订阅握手、控制和恢复烟测通过；前端构建通过，Playwright 预览检查思考展开/折叠、独立回答和生成中状态。

### 技能发现与显式调用

输入 `/` 或点击“＋ 功能”可查看当前会话实际可供用户调用的技能，输入技能名或描述搜索，选择后填写 `/技能名 参数` 并发送。仅用户技能有明确标记；上游命令名优先于同名技能，未支持的命令仍不可通过技能入口执行。技能目录与正文由 Harness 的 `skills.list/get({cwd, scope: agent})` 解析，遵从工作区、作用域和调用策略；CrewVela 不自行扫描、复制技能或安装插件。

发送前再次检查技能仍可供用户调用，再通过 ACP 用户消息保留原始斜杠与参数，复用 `tool-skill` 的 `agent/pre-step` 显式技能注入，执行仍经过会话现有权限和审批。支持与普通消息相同的文件引用和取消；未知或禁止用户调用的技能在写入历史和提交模型前拒绝。真实模型执行未端到端验证。

技能集成验证：控制桥回归通过，覆盖作用域、仅用户技能、隐藏技能及同名命令拒绝；原生会话回归 11 项通过、2 项默认忽略，覆盖未知技能不写历史、参数保留和附件传递；前端类型检查及构建通过。真实安装版无模型烟测发现 169 个可供用户调用的技能，首项为 `3d-chladni-render`；已安装 `tool-skill` 的 pre-step 烟测确认仅用户技能产生 `skill-invocation` 注入，禁止用户调用后不再注入。Playwright 预览验证技能列表、名称和描述过滤、选择填入、中文参数发送及无匹配提示。

### 第一阶段控制功能

本阶段范围为文件附件、目标、压缩及 `/goal`、`/compact` 命令入口，另外提供权限、模型与推理等级选择；计划、反馈、日志导出、插件管理和自动化任务暂不集成。

文件由用户选择后校验为真实文件，随普通消息通过 ACP `resource_link` 引用，由 Harness 自身工具读取；不把二进制伪装成文本上传。目标和压缩通过同一 ACP 进程内的 companion 插件调用 `ctx.commands.execute`，命令不会变成普通模型 prompt。目标自主执行仍由 Harness 的 goal-round-driver 负责，回复及工具事件由原 ACP 通道传回。

本阶段不支持把附件直接提交给 `/goal`：当前 ACP profile 没有 Web 附件上传服务，目标命令会明确提示先移除附件。普通消息仍可携带文件引用。

companion 插件由 CrewVela 内嵌提供，通过临时 `--patch` 注入 ACP profile，不修改上游源码、用户的 profile 配置或安装依赖。控制请求仅使用绑定 `127.0.0.1` 的临时连接，携带每进程令牌与请求标识，并核验当前活动根会话；停止进程后清理临时控制文件。

权限目录及当前值来自 `permissionPresets.catalog/current`，切换通过原生 `set` 写入 Harness 的会话权限事件。仅可查看、工作区内修改和完全权限沿用上游策略；Auto review 仅在兼容实验插件提供 `auto` 权限选项时可用。当前安装的 `dsh 0.1.7-rc.1` 缺少该插件，因此显示不可用，不以自动批准冒充审查，也不混用本地更新版本源码中的插件。

控制功能验证记录：前端类型检查与构建通过；原生会话回归 10 项通过、2 项默认忽略，ACP 回归 17 项通过、6 项默认忽略；控制桥测试 1 项通过，覆盖令牌、根会话校验、命令白名单、中文跨分片、权限与压缩取消。真实安装版 Harness 的无模型烟测 1 项通过，验证临时插件加载、目标状态读取、权限切换和停止恢复。Playwright 使用明确标记的演示数据验证目标新增/暂停/编辑/恢复、附件添加、权限与推理等级选择及未知命令拒绝。真实模型自主目标与有历史内容的压缩尚未端到端验证；本轮未重新执行完整前端测试集，原有 Windows 测试失败未修复或隐藏。

快照和事件使用单调序号与完整快照，首开/切换会话时按序应用；旧请求不会覆盖停止或重启后的状态。工具增量按 ID 合并，审批结束清理卡片。空闲进程意外退出会回收连接及 lease。低速小段回复也会定时发布，中文输入法确认候选时不会误发送。

基础会话入口的验证记录（控制功能集成前）：

- 前端类型检查和构建、Rust 编译检查与 debug 构建均通过。Rust 全库测试 592 项通过、12 项默认忽略；其中原生 ACP 自动回归 7 项通过，人工界面烟测默认忽略。ACP 模块 14 项通过、6 项默认忽略；Runtime 前端测试 11 项通过。已安装 dsh 的无模型控制面测试单独执行 1 项通过。构建保留现有 Rust 警告和前端大 chunk 提示。
- 无密钥假 ACP peer 回归，覆盖流式文本、工具增量、模型选择、审批、取消、停止/恢复历史、并发启动、握手期间停止、进程退出、禁用边界、删除回滚与旧记录兼容。
- Playwright 验证主工作区入口和独立面板交互，包括输入法回车、Shift+Enter 换行、模型选择、流式显示、审批和停止恢复。
- 隔离的 Windows WebView2 窗口使用真实 Tauri IPC 与假 ACP 进程，实际验证发送、流式工具/审批、拒绝、停止、恢复和关闭清理；所有测试会话记录存于临时目录。没有替换用户已安装的 CrewVela，也没有执行真实模型编码请求。
- 浏览器 `ui-preview=1` 仍有已有的 Tauri 监听报错；不将预览模式等同于原生验证。

复现自动回归：

```powershell
rtk npm run build
rtk cargo test --manifest-path src-tauri/Cargo.toml native_acp_sessions::tests --lib
rtk cargo test --manifest-path src-tauri/Cargo.toml acp::tests --lib
```

原生界面人工烟测：先运行 `rtk npm run dev -- --host 127.0.0.1 --port 1432`，再运行下列命令；打开的测试窗口只连接假 ACP peer，关闭窗口结束测试进程并清理连接。该测试按设计默认忽略。

```powershell
rtk cargo test --manifest-path src-tauri/Cargo.toml native_harness_ui_without_model_calls --lib -- --ignored --nocapture
```

Windows MSVC 构建通过链接器统一嵌入 Tauri 默认的 Common Controls v6 依赖，使应用与 MockRuntime 单测均能加载 `TaskDialogIndirect`；应用资源不再重复嵌入清单，其他平台保持原构建流程。

### Windows 工作目录兼容修复（2026-10-03）

Rust `canonicalize` 在 Windows 返回 `\\?\G:\tmp`。Node 的 `realpath.native` 解析其盘符根目录 `\\?\G:\` 时会抛出 `EISDIR … realpath 'G:'`，Harness 首次消息加载项目指令、向上遍历目录时会触发此错误。CrewVela 现在校验并规范化目录后，将盘符及 UNC 路径转换为普通 Windows 格式，再次校验其仍指向同一目录；不能安全转换的设备路径明确拒绝。进程工作目录与 ACP 新建/恢复参数统一使用兼容路径，其他 ACP runtime 保持原行为。

恢复时仍严格比较目录及原权限。上游恢复实现保留原 session.header.cwd，ACP 的恢复目录参数不能覆盖它；因此旧版本已经用 Windows 扩展路径创建的上游会话无法通过参数转换修复。此类旧会话会明确提示新建会话，并保留原 ID、消息历史和定位；不修改上游存储，也不伪装恢复。尚未创建上游会话的旧本地记录可在创建时安全转换，修复版新建的普通路径会话可正常恢复。退出旧应用并启动修复版后，新建 DeepSeek Harness 会话使用原工作目录即可。这次根因复现使用本机 Node 文件系统调用，没有发送真实模型消息。
本次修复验证：原生 ACP 测试 8 项通过、1 项忽略；ACP 模块测试 17 项通过、6 项忽略；前端构建、Rust check 和内嵌前端的 debug 构建通过。真实 UNC 网络共享与真实模型任务未验证。

## 配置

先按 DeepSeek Harness 自身文档安装并配置 CLI，使启动 CrewVela 的环境能够找到 `dsh`。凭据由 Harness 原有配置或继承环境提供，CrewVela 不另外保存模型密钥。CLI 缺失或 ACP 不兼容时，任务明确报错。

需要使用本地源码时，在启动 CrewVela 前设置 `CREWVELA_DSH_ROOT` 为绝对仓库路径。源码仓库必须已经按上游流程安装依赖，包含 `apps/cli/src/bin.ts`、`node_modules/tsx/dist/esm/index.mjs`，并能解析所需上游包；环境中需要 Node.js。该变量优先于全局 CLI。CrewVela 直接使用源码入口和本地 tsx 启动，子进程工作目录保持任务目录，不自动安装依赖或升级。

在协作项目的成员编辑页选择 **DeepSeek Harness**。模型留空使用 Harness 默认配置；显式模型可填写 Harness 公布的模型名（必须唯一）、`provider/model`，或完整 ACP selector（如 `["provider","model"]`）。不存在或跨 Provider 有歧义的模型会在提交任务前报错；模型及 Provider 必须已在 Harness 中配置。不会把这些模型作为 Claude/Codex Provider 调用。

## 权限和会话

| CrewVela 设置 | Harness 启动边界 | ACP 权限请求 |
| --- | --- | --- |
| 只读任务（优先于所有权限档） | `read-only` | 拒绝写入权限；保留现有只读控制面调用规则 |
| default | `workspace-write` + 上游 ask | 创建项目审批卡片，等待人工批准后只允许本次调用 |
| auto | `workspace-write` + 上游 ask | 自动批准本次调用，保留工作目录写入边界 |
| yolo | `danger-full-access` | 上游不再要求审批 |

default 的审批进入 CrewVela 现有项目审批流程，等待期间续租；取消或 30 分钟超时清理待审批记录并结束执行。关闭或暂停任务可中止等待。独立 PTY 的审批策略不适用于此 ACP runtime。

协作控制面通过带任务令牌的 HTTP MCP 注入。会话使用 `session/new` 创建，正常完成时 `session/close`，下一轮通过 `session/resume` 恢复；恢复失败不会假装建立新会话。Harness 的 resume 返回值可以没有 `sessionId`，CrewVela 保留请求中的原会话 ID。

恢复定位保存任务目录和权限配置。目录或权限变化、来源不明的旧会话定位都会拒绝恢复，需要新建会话，以免旧会话保存的权限覆盖新的启动限制。新任务的 ACP 握手、模型设置和任务执行失败会明确报告。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml deepseek --lib`：无需密钥的假 ACP peer，覆盖 HTTP MCP、新建、恢复、模型选择、审批回调、只读拒绝、取消和恢复权限校验。
- `cargo test --manifest-path src-tauri/Cargo.toml deepseek_installed_control_surface_without_model_call --lib -- --ignored`：需要已安装 dsh；真实握手、新建、设置当前模型、关闭、跨进程恢复、再关闭，完全不发送 prompt。
- `npm.cmd run build`：前端类型检查和构建。

这些检查不等同于真实模型完成编码任务；正式端到端验收仍需要有效 Harness 配置及模型调用。

### 本次验证记录（2026-10-02）

前端构建通过，成员表单可选择 DeepSeek Harness，默认成员名称显示正常；草稿状态与 Runtime 更新测试共 8 项通过。最新 `cargo test deepseek --lib` 为 7 项通过、1 项默认跳过；ACP 模块为 14 项通过、6 项默认跳过。回归覆盖仅携带工具 ID 的审批详情关联、双向 JSON-RPC 请求 ID 碰撞、只读拒绝、取消及恢复权限校验。已安装 `dsh` 的无模型控制面烟测单独执行通过，验证新建、选择当前模型、关闭及跨进程恢复。

`src-tauri/src/media.rs` 的缺少返回值编译阻断已由工作区现有修复解除，本轮 Rust 验证成功。工作流界面测试另有一条工具栏 `onClick={addNode}` 源码断言失败，未削弱断言；因此不宣称全部前端测试通过。尚未执行真实模型编码任务，也未打包或替换已运行的 CrewVela 应用。

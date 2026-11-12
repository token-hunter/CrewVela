# 后端可靠性与性能审计（2026-11-12）

基线：`main@ec2a095`。工作分支：`fix/backend-reliability-performance`。
本次未修改前端代码，未调用子智能体。

## 范围与方法

- 根目录 CodeGraph 初始索引：539 文件、10,866 节点、41,278 关系，无解析错误。
- 默认被排除的 `src-tauri/vendor/portable-pty` 单独建立索引：10 文件、277 节点、699 关系。
- 对 Git 跟踪的 Rust、JS/TS、Python、PowerShell 等源码核对：549 文件中 543 被图谱覆盖；其余 6 个 PowerShell 文件使用 PowerShell AST 解析补充，全部语法通过。
- 非前端源码清单共 140 文件（含运行时资源、脚本、示例、vendor）；全量做结构、调用关系和风险模式扫描，再对进程生命周期、会话、存储、IPC、文件、网络、Skill、自动化、MCP 和平台桥接等关键链路审阅，结合全量现有测试验证。
- 图谱用于导航、调用关系及影响分析，不是“自动证明没有 Bug”的工具；全量扫描不等于每条运行分支都经过真实业务环境测试。
- CodeGraph `affected` 返回 95 个受影响测试文件；因此执行完整 Node/Rust 回归，而非只运行新增测试。

## 已修复问题

| 问题 | 修复与证据 |
| --- | --- |
| 项目并发保存丢失数据 | 16 个线程保存均成功，但磁盘只保留 1 个项目。读改写加进程内互斥，复用已有原子替换；修复后保留全部 16 个。桌面已有按配置目录的跨进程独占锁。 |
| 启动预设并发写坏 JSON | 并发创建可产生 `trailing characters` 或覆盖其他预设。读改写串行化、原子发布，保留现有权限与错误处理。 |
| Runtime/会话切换后累计用量错误 | 相同应用会话换到等长 transcript 时复用旧 offset，90 tokens 错显为 10。缓存按实际 transcript 路径重置；无 transcript 时清除旧值。全局注册表锁不再覆盖磁盘扫描。 |
| 历史回看随全部历史线性变慢 | 改为固定块反向读取，收集到需要的渲染消息数量即停止；保留中文、Emoji、跨块长用户原文、块内顺序、时间戳、末行无换行和错误处理。 |
| 端点测速串行累计超时 | 最多 4 路并发，完成一项补入下一项，保留输入顺序、去重、无效地址和逐项错误；本地 HTTP 屏障测试证明请求确实重叠。 |
| 通知批量导入慢且失败后部分落盘 | 单连接、单事务、复用预编译 INSERT；中途注入 SQLite 错误会整批回滚；保留去重、已读状态和过期规则。 |
| 高频和重 I/O 命令阻塞窗口或异步工作线程 | 25 个会话统计/历史、通知、工作区读写、Skill 导入/发现/部署命令移入阻塞任务池，成功 IPC 数据形状保持兼容；工作线程错误正常返回，事件循环计时器在工作任务等待时仍能运行。 |
| Windows PTY 属性列表暴露未初始化字节 | vendor 的 `Vec::with_capacity` + `set_len` 改为初始化缓冲区，保留 Job 句柄与进程属性生命周期。由 Windows PTY 回归和构建验证，未声称曾观测到该未定义行为的运行时崩溃。 |
| Windows PTY 测试清理偶发失败 | 基线遇到共享冲突 32。测试现在确认子进程退出，使用真实退出排空路径收集末尾输出，仅对共享冲突做有截止时间的清理重试；保留输出、退出及清理失败断言。此项为测试生命周期修正。 |

## 可复现测量

Windows 本机 debug 测试夹具，数值受机器负载影响，不能代替用户环境的端到端延迟。

| 场景 | 修改前 | 修改后 |
| --- | --- | --- |
| 10 万条日志中读取末尾 2 条 | 读取 5,300,058 bytes；约 233–245 ms | 读取 65,536 bytes；约 0.27–0.74 ms |
| 导入 500 条通知 | 约 1,299 ms | 约 2.8–4.2 ms |

新增回归以结果、实际读取字节、HTTP 请求重叠和事务回滚为断言，不用易抖动的耗时阈值制造通过。

## 验证记录

- 初始 Node：472 通过、1 跳过。
- 初始 Rust lib：1,104 通过、1 失败、48 忽略；失败为上表 PTY 清理共享冲突。
- 修复前新增复现：6 项失败，分别覆盖项目、预设、用量、历史读取、端点并发、通知事务；另有通知批量兼容性基准通过。
- 修复后新增回归：11 通过。
- Node 全量回归：472 通过、1 跳过。
- Rust 全目标回归：1,117 通过、49 忽略（包含 1 个单独启用的打包资源检查）。
- `npm run build`：通过；保留现有大 chunk 警告。
- `cargo clippy --all-targets -- -W clippy::correctness`：完成；未屏蔽现有 warning，也未使用 `cargo fix` 全库清理。新增测试的读取返回值警告已修正。
- PowerShell：6 个文件 AST 语法检查通过；`test-computer-use-runtime.ps1` 的树索引、Unicode 输入事件构造、窗口捕获契约测试通过，无真实桌面操作。
- 最终源码再次执行 `cargo test --all-targets --features custom-protocol`：1,117 通过、49 忽略。
- PTY 共享冲突回归另连续执行 20 次：20/20 通过。
- `npm run test:desktop-assets`：单独启用的资源检查 1 项通过，确认入口及引用的 JS/CSS 已内嵌且不指向 Vite 开发服务。合计 Rust 1,118 项通过，仍有 48 项外部/人工验收忽略。
- `npm run tauri -- build --no-bundle`：Windows release 构建通过，产物 `I:\tmp\CrewVela-backend-audit-target-20261112\release\CrewVela.exe`。
- EXE SHA-256：`B27B41575790C75CEF0CA86DDE6DF13733BE331D3FA0B40784BEC3AAEBB38636`。最后仅对两个函数整理缩进（脚本断言除此之外字节不变），Clippy 最终检查完成；保留 196 项 lib test 警告，其中 148 项与 lib 重复，未降低检查等级。
- 原生 WebView2 冒烟：通过 `--test-data-dir I:\tmp\CrewVela-backend-audit-smoke-20261112` 独立启动，确认主工作区、设置、通知设置和空通知历史可加载，正常退出；未读取正式会话或发起模型调用。模型额度显示查询失败，Skill 中心明确显示隔离模式禁止访问全局集成，均不能计作真实账号/Skill 集成通过；应用 stderr 为空。

构建缓存为避免 F 盘不足复制到 `I:\tmp\CrewVela-backend-audit-target-20261112`，未删除原缓存。复现日志及逐文件清单位于本地忽略目录 `.scratch/backend-reliability-performance/`。

复跑命令（PowerShell）：

```powershell
$env:CARGO_TARGET_DIR = 'I:\tmp\CrewVela-backend-audit-target-20261112'
rtk npm test
rtk proxy cargo test --manifest-path src-tauri/Cargo.toml --all-targets --features custom-protocol
rtk npm run test:desktop-assets
rtk npm run tauri -- build --no-bundle
```

## 验证边界

忽略项包括真实模型调用、账号授权、外部服务、真实桌面交互及跨平台验收，不计作通过。本次不会将 Mock 协议测试、编译成功或 Windows 测试解释为所有 Agent/服务真实对话成功，也不代表 macOS/Linux 实机验收。

仍有仓库既有编译/Clippy 警告及前端包体警告；本次没有关闭检查或隐藏异常。“所有环境绝无报错、绝无延迟”无法由有限审计与回归保证。网络、磁盘和外部模型均存在真实等待，验证目标是正确处理这些等待且不阻塞关键交互线程。

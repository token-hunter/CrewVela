Status: ready-for-human

# Mac Codex 误报未检测到与共用 CLI 进程状态

## 需求与诊断

- 维护者在 Mac 从 main 打包后启动 Codex CLI 报“未检测到 codex Runtime”，担心各平台路径写死。
- 维护者终端检查确认 Homebrew 的 Codex 可正常输出版本，Node 在用户 local/bin，默认 shell 为 zsh。用户名和个人完整路径不记录到仓库。
- 现有 Finder PATH 初始化覆盖 Homebrew 标准位置和根据当前 home 推导的 local/bin，没有新增任何个人路径。
- 维护者执行系统 true 的最小复现：waitid(WNOWAIT) 确认自有子进程已退出，但 getpgid 返回 ESRCH；已有 Mac 云端日志也出现同类进程组身份失败。
- Codex 是版本探测失败后进一步吞掉错误、误报未安装；其它 CLI 版本探测和受管进程退出也共用该 POSIX 管理层。

## 实施

- 只对 Darwin 已退出但仍未回收的自有子进程接纳 getpgid 的 ESRCH。缺失组查询时不发送终止信号；必须核对原组及可能的原会话没有活动成员后才能完成清理。
- 继续拒绝已回收/非自有进程、活进程查询缺失、组身份变更、其他系统查询错误以及仍有活动成员的情况。
- Codex 候选继续进行版本验证；保留探测失败具体原因，并在独立 CLI 启动和 Runtime 状态中展示，不以删除检查方式放行。
- 修正 Codex 回归夹具无条件写 Windows cmd 的问题，Unix 使用带执行权限的 shell 夹具。
- 保留上一个成员窗口修复；按维护者后续授权提交本任务源码，不推送、不更改 Release 或仓库私有状态。

## 验证

- 红绿回归：Codex 文件存在但版本命令失败时，修复前丢失实际错误，修复后保留诊断；Darwin 已退出未回收子进程的 ESRCH 状态模型修复前失败、修复后通过。
- Windows：运行时 9 项、标题/CLI I/O 与超时清理 19 项、PATH 2 项、进程状态 3 项、实际已安装 Codex CLI/Chat 共享可执行路径 1 项，共 34 项针对性 Rust 验证通过。外部联网/模型测试仍未执行。
- Node：434 项通过、1 项既有跳过。
- 将实际 POSIX 实现与原生测试放入忽略目录的最小编译宿主，使用与项目相同版本的 anyhow/libc/uuid，aarch64-apple-darwin 的库及测试类型检查通过；不等于整个 Tauri Mac 应用已构建或运行。
- 新增 Unix 原生测试验证：已退出自有子进程保持 WNOWAIT、延迟登记、检查清理后再回收。该原生运行用例尚未在 Mac 执行。
- 无新增个人安装绝对路径；Windows/Linux 的 PATH 初始化行为不变。所有诊断与试验留在忽略目录，原有用户数据未读取或修改。
- 维护者于 2026-11-10 授权提交本任务源码；本次不推送、不打包或更新 Release。上一任务的成员弹窗修复已单独提交并保留。

## 其它 Runtime 检查

| 入口 | 现有发现方式 | 本次影响 |
| --- | --- | --- |
| Claude Code | PATH；Windows 使用 npm shim/原生入口解析 | 版本和受管退出复用修复 |
| Codex CLI / Chat | PATH、平台 npm 候选；版本校验 | 修复误报与保留错误 |
| Kimi / Pi / OpenCode | PATH | 版本和受管退出复用修复 |
| ZCode CLI | 显式配置、PATH、脚本入口解析 | 受管退出复用修复 |
| ZCode Desktop / DeepSeek Harness | 应用布局或显式配置、PATH | 共用进程层受益；未逐一执行真实业务 |

Finder 的 PATH 从用户登录 Shell、继承环境、系统标准安装根及当前 home 派生目录合并，不替换为开发者个人目录。

## Mac 复核

带上本次修复后在 Mac 项目根目录执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib acp::process_group::tests::
cargo test --manifest-path src-tauri/Cargo.toml --lib multi_agent::runtime::
APPLE_SIGNING_IDENTITY=- npm run build:macos
```

再从 Finder 启动 .app，分别打开 Codex、Claude、Kimi、Pi、OpenCode、ZCode CLI 中实际已安装的 CLI，确认探测、创建、退出。未安装或缺少登录/订阅的 Runtime 应保留真实失败提示，不视为全部已支持。当前 GitHub main 尚无本次修改。

## 原始依据

维护者在本机的 waitid/getpgid 复现为直接证据。Apple 的 getpgid 系统实现通过 proc_find 查找目标、未找到时返回 ESRCH，参考 [Apple XNU](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_prot.c)。此处不复制上游实现或存储维护者个人终端路径。

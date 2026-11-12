# macOS 外部电脑控制

CrewVela 在 Mac 上接入 Open Computer Use、KimiCU 的 stdio MCP 服务。内置 PowerShell 电脑控制仍仅用于 Windows。此改动不附带第三方安装包，不移植内置方式，也不会自动授予 macOS 权限。

## Open Computer Use

1. 按[上游安装说明](https://github.com/opensymph/open-computer-use)安装 macOS 14+ 运行时。当前 npm 入口为 `npm install -g @opensymph/open-computer-use`，命令为 `open-computer-use`（也支持 `ocu`）；已有旧包所提供的同名命令仍可发现。
2. 重新打开 CrewVela，进入设置 → 电脑控制，重新检测运行时。Finder 启动时复用应用已有的登录 Shell PATH 初始化；MCP 配置也保留该 PATH，供 npm 启动器查找 Node。
3. 点击“打开 Open Computer Use 授权”，由上游 `doctor` 打开权限引导。辅助功能、屏幕录制由用户在系统设置中授予。
4. 点击“检查 Open Computer Use 权限”：使用上游 `doctor --no-ui` 的明确布尔状态。旧版本不支持该格式时显示待检查及原因，不把无法识别的输出视为通过。
5. 启用并保存，再新开或恢复 Agent 会话，核对其 MCP 工具加载状态。

支持 PATH 中的启动器、`~/Applications/Open Computer Use.app`、`/Applications/Open Computer Use.app`。应用包读取实际 `CFBundleExecutable`，验证应用标识、最低系统版本及 Mach-O 架构。npm 启动器负责选择其自带的当前平台运行时。

## KimiCU

1. 按[Kimi 官方说明](https://www.kimi.com/code/docs/kimi-code-cli/customization/plugins.html#kimi-computer-use-v0-5-4)安装 KimiCU.app。当前官方 Mac 安装包为 Apple Silicon、macOS 14+；CrewVela 不会在 Intel Mac 上启用此入口。
2. 支持 `~/Applications/KimiCU.app`、`/Applications/KimiCU.app`；PATH 中指向其他位置有效 KimiCU.app 的 `kimi-cu` 链接也可发现。读取官方应用标识 `ai.kimi.cu` 和实际执行入口，不拼接 Windows 路径或个人目录。
3. 点击“打开 KimiCU 授权与后台服务”。检查 `service-status`，服务未运行时执行官方 `install`，然后通过 macOS `open -a` 打开已检测到的应用。不会执行卸载、强制退出服务或自行编写 LaunchAgent。
4. 在 KimiCU 官方窗口完成辅助功能与屏幕录制授权，再点击“检查 KimiCU 权限”。通过官方 `xpc-ping` 的权限字段和 `service-status` 分别报告权限与服务；失败或未知格式保持待检查。
5. 启用并保存，在新开的 Agent 会话中检查 `kimi-cu` 工具加载。

## 状态与配置

- “已安装”只说明找到执行入口，“MCP 已注册”只说明配置存在；都不等于工具已加载、连接成功或操作完成。
- 读取设置不会准备后台服务或申请系统授权。权限检查、授权引导和服务准备均通过明确按钮执行；程序探测有超时和输出上限。
- 启用后只更新 CrewVela 自己管理的 MCP 条目。用户手动添加的同名 MCP 保持不动并报冲突。
- 迁移目录、更新运行时或从 Windows 迁移配置后，点击“重新连接”更新该条目的本机执行入口及 PATH。随后重开会话。
- 保存失败会恢复该适配器原有 MCP 条目；配置损坏会明确报错。关闭某种方式只移除 CrewVela 管理的对应条目，不卸载软件或清理第三方数据。
- 独立首次启动测试模式继续禁止真实电脑控制；网页 Mock 通过不代表 Mac 原生验收通过。

## Mac 实机验收（待执行）

在独立测试账号或可回滚的测试环境验收；不要使用包含真实密码、验证码、支付或发布操作的页面。

1. 普通模式从 Finder 启动、终端启动各一次，分别验证 Homebrew/npm 自定义前缀、含空格中文的应用路径和有效 PATH 链接。
2. 缺少运行时、macOS 13、KimiCU 在 Intel Mac、损坏应用包、不可执行文件、架构不匹配时显示原因，不启动 MCP；原有已启用配置仍可关闭。
3. 两种方式分别验证未授权、只授予一项、两项均授予、撤销权限；界面必须区分未知、待授权和已授予。禁止借用 CrewVela 自身的权限作结论。
4. KimiCU 服务未准备时，按钮可完成官方服务准备；重复点击不卸载或强制结束已有服务。拒绝权限时应正常显示原因。
5. 分别保存启用，重新打开 Claude Code/Codex 等受支持会话，完成 MCP initialize/tools/list；在测试应用中读取界面、截图、点击、输入、滚动，确认真实行为。
6. 同时启用两种方式及单独关闭其中一种，不应删除用户自定义 MCP。手动同名条目、损坏配置、只读目录、更新后路径漂移均应有明确反馈。
7. Windows 回归原有三种控制方式；不要求 Windows 使用 Mac 的 `.app` 路径、权限检查或服务命令。

本开发环境是 Windows，不能据此声称上述 Mac 原生操作已通过。测试记录应注明系统版本、架构、第三方运行时版本及成功/失败步骤，不记录账号或密钥。

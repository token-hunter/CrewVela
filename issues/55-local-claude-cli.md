# 使用本地 Claude Code 配置创建 CLI 会话

Status: ready-for-human

## 范围

- Provider 选择页增加本地配置入口，不要求配置 CrewVela Provider/Key。
- 复用既有本地 Runtime 启动与工作目录选择，保留项目归属、错误提示和运行时启用检查。
- 不注入模型、账号、服务地址等覆盖；保留通信身份与生命周期 hooks。
- 原有 Provider 创建流程保持；不复制、读取或改写用户凭据文件。
- 保留其它未提交改动；本次不提交、推送或打包。

## 验收

- 无 Provider、无 Key 仍可选本地配置；原 Provider 与其它 Runtime 入口不受影响。
- 实际 UI 检查深浅主题与窄窗口；真实 App 预览创建路线回归。
- Rust 命令契约验证不覆盖本机模型、端点、密钥，保留通信 hooks。
- 构建、前端回归测试；原生账号和模型请求若未执行则明确记录。

## 结果

- 新入口复用 `launch_runtime_session(claude_code)`，无需 CrewVela Provider 或 Key；普通会话选择目录，项目会话沿用项目目录。
- 生产后端不变；增强既有 Rust 通信契约测试，确认 settings 不含模型覆盖，仅注入 CrewVela 身份和 hooks，不注入 ANTHROPIC 环境变量。
- Rust 针对性验证 1 项通过；Node 439 项通过、1 项既有跳过；类型检查与前端构建通过，既有大包警告保留。
- CUA 验证无 Provider、无 Key、本地/Provider 分别路由、键盘选择、深浅主题两档尺寸；真实 App 预览验证本地普通/项目会话、原 Provider 数量选择和 Codex CLI 创建。控制台 error 零条。
- 可复用 UI 脚本语法检查通过；此次交互通过 CUA 执行，未单独执行 Playwright CLI 脚本。
- 未读取或改写本机 Claude 凭据文件，未发起真实模型请求；原生 Windows/macOS 模型和登录验证待实测。未重新打包、提交或推送。

## 发布集成回归（2026-11-11）

- 用户已授权将本地待提交功能合入 main 并更新 Windows 安装包；前述不提交或不打包仅描述原实施轮次。
- 在当前 main 的独立工作区重新集成，Node 全量 442 通过、1 项既有跳过；类型检查和前端构建通过。
- 使用本机 Chrome 完成 7 组界面回归，保留原脚本全部断言：置顶、会话菜单、本地 Claude 入口、浏览器单列、协作任务布局、成员弹窗和设置交互。
- 本地 Claude 启动契约 Rust 测试 1/1 通过；此次 28 个范围内文件凭据扫描零命中。截图仅来自隔离夹具；未使用个人配置或历史。
- 本轮未执行真实模型登录、调用或 macOS 原生界面验收；安装与卸载完整流程仍需实测。

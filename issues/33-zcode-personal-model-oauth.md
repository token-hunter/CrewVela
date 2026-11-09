Status: ready-for-human

# 恢复 ZCode Desktop 个人套餐 GLM-5.3 目录和真实回复

- 本地选择 Individual Coding Plan，但缺少该套餐的旧缓存凭据，适配器仅开放 Start Plan。当前 Start Plan 权益仅包含 GLM-5.3-Flash，其模型调用此前受服务商 3012 拦截。
- 复用安装版原生 ProjectAccessTokenClient，用当前账号 OAuth 刷新已有个人项目凭据；保持账号/套餐绑定、权益校验和登录变更失效。
- 不生成永久 API Key，不改写原生凭据或设备文件；仅允许官方端点的已有凭据刷新。
- 已连接会话也能重新加载模型，保留原历史；验证具体 GLM-5.3 模型选择和一轮真实回复。

## 实际结果

- 复现明确：个人套餐选择有效但对应旧缓存不存在；旧适配器只返回 Start Plan 的 Flash。新增回归先失败后通过。
- 修复后权益接口确认个人套餐有效；真实安装版目录返回 9 个模型/推理级别选项，原生模型绑定通过。
- 精确选择个人套餐 GLM-5.3 后，真实一轮请求返回助手回复并到达 completedSuccess；此前将 Start Plan 的 3012 视为整个账号不可聊天的结论已纠正。
- 新增回归覆盖官方端点限制、禁止新建永久 Key、登录在查询/签发期间变化即拒绝、身份绑定及凭据不落盘。Node 全量 415 通过、1 原有跳过，最终账号 Host 专项 8/8；相关 Rust 67 通过、13 默认忽略，真实目录/绑定及 GLM-5.3 回复检查单独执行通过。
- Chrome 隔离页面使用真实 ZCodeChat 组件，验证已有模型时的刷新、原历史保留、选择 GLM-5.3 和发送流程。页面模拟回复不充当真实模型验收；截图/日志位于 `.scratch/zcode-model-diagnostics/`。
- 当前复用的是安装版 ZCode 3.17.2 的账号凭据组件；未来原生组件布局改变时会拒绝开放未经校验的模型。macOS/Linux 未实机验证。
- 首次 release 构建因默认 EXE 被 3 个现有进程占用而无法覆盖。将旧 EXE 在同一 release 目录改名为 `CrewVela.running-backup.exe`，没有停止进程或会话；随后仍按用户指定命令重试，不改变默认产物目录。
- `npm run tauri -- build --no-bundle` 重试通过，类型检查、Vite 构建及 Windows release 均完成；保留已有 Rust 警告。新产物为 `src-tauri/target/release/CrewVela.exe`。当前仍在运行的旧进程需用户退出重开，新模型刷新后选择 GLM Coding Plan / GLM-5.3；不默认改写旧会话的模型选择。
- 未提交、未推送；未停止现有用户会话。Start Plan 的服务端 3012 拦截未绕过，正常个人套餐路径已完成真实回复验收。

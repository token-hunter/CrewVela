Status: ready-for-human

# 修复 ZCode Desktop 账号模型权益查询参数错误

- 当前已连接但账号模型为空；实际权益 GET 返回 HTTP 400 / code 3001。
- 保留原生登录和设备状态，读取既有 deviceMid，仅发往原生 ZCode 权益端点，不进入 UI 配置或日志。
- 验证原生目录加载、账号绑定、相关回归，按指定命令重新打包。

## 结果

- 确认根因为权益请求缺失原生设备标识请求头 X-Device-Mid；相同账号/版本/接口仅补该字段，HTTP 400 / code 3001 变为 HTTP 200 / code 0。
- 从原生 telemetry-state.json 读取已有标识；缺失/损坏/非法头字符时保留原来的校验失败行为，不改写原生文件。
- 新增回归先失败后通过，覆盖 Start Plan 模型目录恢复、缺失/畸形设备状态、业务接口隔离及配置快照不包含设备标识；Node 全量 414 通过、1 跳过。
- 真实安装版目录及绑定验收通过，model_count=3；保留原有 Individual 专项验收，新增 Start Plan 独立验收，没有放宽旧检查。
- 真实模型回复验收失败：原生 Agent stderr 明确返回 ProviderBusinessError / providerCode 3012 / request has been blocked due to unusual activity。这是服务商的异常活动拦截，停止重试，没有更换账号或尝试绕过。需要用户在原生客户端确认账号状态或联系服务商。
- 临时诊断均位于 `.scratch/zcode-account-models/`，输出已脱敏；产品源码的临时 DEBUG 探针已移除。没有修改界面组件。
- 最终相关 Rust 回归 67 通过、13 默认忽略；上述真实目录/绑定检查另行执行通过。按 `npm run tauri -- build --no-bundle` 完成类型检查、前端构建及 Windows release 打包，输出 `src-tauri/target/release/CrewVela.exe`。保留原有构建警告，没有宣称真实模型回复通过。
- 未提交、未推送；需退出旧版 CrewVela 后打开新 EXE，再重新加载模型。服务商风控需要另行确认，当前不能宣称聊天完全恢复。

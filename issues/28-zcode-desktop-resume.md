Status: ready-for-human

# 修复 ZCode Desktop 恢复失败

## 验收

- 复现并修复 `session/resume -32603`，空会话可安全重开，有历史会话不得静默丢弃。
- 保留会话、模型选择和用户配置，未知内部错误不能一律当作会话不存在。
- 验证真实安装版无模型请求的创建/恢复，以及隔离协议回归；按指定命令打包。

## Comments

- 当前安装版 ZCode 3.17.2。目标 G:\\xiaohongshu 会话已有 nativeSessionId，本地 rows 为 0。
- 真实安装版隔离测试已复现原报错；根因为缺失空草稿在前置归属校验被映射为 -32603 / project.session-lineage-invalid，旧恢复只识别 -32004。
- 已加入精确原会话 ID 查询确认，只有本地无历史且服务端合法返回空列表才重建；工作区不匹配、未知内部错误、查询异常、有历史等均不重建。
- 真实安装版空草稿恢复、合成历史跨进程恢复均通过，不调用模型。原生回归 27 项通过；完整 Rust 1039 通过 / 39 默认忽略，Node 411 通过 / 1 跳过。
- 证据和限制：`.scratch/zcode-desktop-resume/qa.md`。临时探针已移除，未写入用户配置或会话文件，未自动退出用户程序。
- `npm run tauri -- build --no-bundle` 已成功，默认产物 `src-tauri/target/release/CrewVela.exe`；已有 Rust warnings 和大 bundle 提示保留。完全退出旧进程后重开并恢复原会话。未提交或推送。

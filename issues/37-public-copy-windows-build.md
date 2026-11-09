Status: ready-for-human

# 验证干净开源副本能独立构建 Windows EXE

- 在公开副本中安装锁文件指定的 npm 依赖，不借用原项目的依赖链接或构建目录。
- 执行用户指定的 `npm run tauri -- build --no-bundle`，检查默认 EXE 输出与资源嵌入。
- 构建遇到错误时保留原始证据并定位，不绕过有效检查；不操作用户真实会话或账号。
- 更新公开副本验证记录，说明实际结果、警告与未验收边界。不提交、不配置远程、不推送。

## 验证结果

- `npm ci` 独立安装成功（203 个包），未借用来源依赖或 target；Cargo 仅复用本机下载缓存。
- 指定 EXE 构建命令通过，产物名称、产品名称及 `--version` 确认为 CrewVela；默认输出位于本副本 `src-tauri/target/release/CrewVela.exe`。
- 发布资源集成检查通过 1/1，确认入口及 JS/CSS 嵌入，并拒绝开发服务器引用。测试后再次执行指定命令生成正式构建产物。
- 独立依赖下 Node 回归 420 通过、1 项既有跳过；导出源码及两个锁文件没有被构建改动。
- 保留已有 Rust 48 条 warning、大 chunk 和 macOS identifier 提示。依赖审计 16 项漏洞（6 中危、9 高危、1 严重），严重项为间接依赖 `proxy-addr`；本任务未升级或自动修复依赖。
- 没有重新运行 Rust 全量、GUI、真实模型及 macOS/Linux 验收。不提交、不推送；产物及日志保持被忽略。

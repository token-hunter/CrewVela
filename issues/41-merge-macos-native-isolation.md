Status: ready-for-human

# 合并 Mac 原生窗口存储隔离修复并验证 Windows

- 已校验 Mac 变更包与原始 ZIP 的基线、所有包内散列、826 个最终文件清单及真实源码差异。
- 合并 7 个内容修改；启动器只有解压权限差异，保留仓库现有 100755。
- Mac 报告的 5 项 Node 跳过均为既有 Windows 专属条件，没有增加跳过。
- 保留 Mac 的延迟窗口创建和显式 builder 存储身份；调整测试临时目录的 Windows/Unix 差异。
- 验证 Windows Node、Rust、资源与 EXE，记录 Mac 原生报告与本地复验的区别。不复制真实报告机器路径或历史数据到发布文档，不提交或推送。

## 本机合并结果

- Windows Node 全量 425 通过、1 项既有跳过，0 失败；Mac 报告中跳过的 Windows 专属检查已在本机实际执行。
- Rust 隔离 8/8、凭据 5/5、旧目录兼容 3/3、CLI 1/1、发布资源 1/1 全部通过。没有运行全部 Rust 库测试。
- `npm run tauri -- build --no-bundle` 通过，TypeScript/Vite 构建通过；保留 48 条既有普通库 Rust warning 和已有前端 chunk 提示。
- 原生 Windows EXE 在新测试目录实际打开，0 会话、首次引导及隔离标识正常；关闭提示交互及窗口正常退出通过。截图留在忽略目录 `.scratch/macos-merge/windows-native-first-open.png`。
- 826 个可发布候选文件的 Gitleaks 脱敏扫描零命中；原始 Mac 报告/截图未加入发布清单。未改动原开发仓库或正式历史。
- 未提交、未推送，旧交付 ZIP 未替换。去除私人路径的验收摘要见 `docs/macos-merge-validation.md`。

## Comments

- 2026-11-09：用户后续授权与 v2 普通启动修复一起提交至开源副本，不推送。上述未提交状态为合并验证阶段记录。

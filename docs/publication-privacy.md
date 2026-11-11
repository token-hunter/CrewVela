# 源码公开与本地数据边界

## 为什么本地存储仍需要发布检查

用户的账号、凭据、会话与共享记忆主要保存在用户机器上。GitHub 公开的是 Git 跟踪文件、历史提交、标签以及 Release 附件；正常情况下不会自动上传应用读取的本地数据。

但文档、截图和编译产物可能另行包含本机信息：例如目录名、历史任务文字、源文件位置或构建用户目录。它们与账号数据库不同，却同样会随对应文件公开。因此需要分别检查源码、历史、媒体和安装包。

## 当前修复

- 公开文档使用仓库相对路径、通用用户目录和脱敏的参考路径。
- 含私人任务或目录的截图移出当前发布树，采用不含个人会话的原生选择器实拍。
- npm 发行命令自动映射编译路径并去除调试信息；用户命令仍为 `npm run tauri -- build --no-bundle`。
- 发行程序从安装位置与运行时 PATH 查找依赖；开发构建才查开发源码目录。Codex 的包装桥同时用于本地安装和 npx 回退。
- npm 锁文件的已知告警通过兼容依赖更新处理；验证记录与历史边界单独保留。

本轮 Windows 回归为 Node 433 通过、1 项既有跳过；已执行受影响的 Rust 回归及资源集成检查。独立测试窗口显示空会话、首次通信提示、教程和 Agent 菜单。它不代表真实账号或模型通信的本轮验收。Rust 依赖漏洞审计尚未完成。

## 构建后复核

Windows：

```powershell
npm ci
npm test
npm run tauri -- build --no-bundle
node scripts/check-release-privacy.mjs src-tauri/target/release/CrewVela.exe --forbid-path "$PWD" --forbid-path "$env:USERPROFILE"
npm run bundle:windows
```

路径检查只报告命中类别和数量，不回显实际匹配值。安装包还需解包复核其中的应用及资源，检查 SHA-256、许可和安装流程。它不替代独立密钥扫描。

Mac：在原生 Apple Silicon Mac 上执行，不能在 Windows 声称完成此项。

```sh
npm ci
npm test
APPLE_SIGNING_IDENTITY=- npm run build:macos
node scripts/check-release-privacy.mjs \
  "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/CrewVela.app/Contents/MacOS/CrewVela" \
  --forbid-path "$PWD" --forbid-path "$HOME"
```

若应用包名不同，从 `bundle/macos/` 选择实际 `.app`，不要跳过检查。还需从最终 DMG 安装后验证启动、正常退出、独立目录隔离、适配器发现与真实双向通信。ad-hoc 签名不等于 Developer ID 签名或公证。

## 旧历史与已发布附件

按维护者选择，原仓库改名后留作独立私有备份；本仓库采用扫描后的干净初始历史，不导入旧提交和旧附件。Windows 安装包重新构建；Mac 新包待原生复核，旧 DMG 不在此提供。

应用数据目录不作为源码发布内容，也不为了发布清空用户账号和历史。

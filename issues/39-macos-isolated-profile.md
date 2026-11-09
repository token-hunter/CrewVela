Status: ready-for-human

# 补齐 macOS 启动器与独立测试存储

- 范围：仅开源副本；保留 Windows 行为及已有未提交改动。
- 实现 macOS 14+ 专属 WKWebView 存储身份、旧系统拒绝策略和 Finder .command 启动器。
- 回归启动参数、路径安全、独立配置/凭据、重启身份稳定性及 Windows 打包。
- 文档写清 Mac 构建、首次启动、重开、第二测试目录、失败保护及正式数据边界的验收方法。
- 不推送、不执行原历史数据读取、不声称完成 Mac 原生验收。

## 实际结果（2026-11-09）

- 增加 `start-first-run-test.command`，支持副本 .app 发现、自定义应用/测试根、中文/空格及字面量参数；直接执行带测试参数的二进制，保留失败退出码。LF 换行和 Git 100755 可执行位已记录（仅 intent-to-add，无提交内容）。
- Mac 测试 WebView 改用由测试根派生的稳定专属 dataStoreIdentifier，旧版本或版本探测失败在创建配置/WebView 前拒绝；普通模式不增加这个版本门槛。Mac 网页存储由 WebKit 管理，实际位置不保证在测试根。
- Windows Git Bash 启动器测试 4/4；全量 Node 测试 425 通过、1 项既有跳过。后续启动器移除对 BSD dirname 参数的依赖后，4 项启动器测试再次通过。
- Windows Rust 隔离测试 7/7、凭据 5/5、旧目录兼容 3/3、CLI 1/1；原生 EXE 参数/非空目录拒绝 2 项通过，合成夹具保持不变。
- `npm run tauri -- build --no-bundle` 通过；实际打开新构建的 Windows 隔离窗口，0 会话、首次通信引导和独立测试根标识均正常。
- 发布资源集成检查最终 1/1 通过；首次因本轮测试进程占用 EXE 无法重新链接，关闭该隔离窗口后重试通过，没有削弱检查。822 个可发布文件的 Gitleaks 脱敏扫描零命中。
- Mac 编译、签名包、Finder、WKWebView、Keychain、旧 macOS 拒绝的原生验收均未执行。验收说明在 `docs/macos-isolated-test.md`；未推送、未触发远端 CI、未提交。

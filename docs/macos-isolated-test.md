# macOS 独立首次启动测试

状态：已合入用户从 Mac 回传的 ARM64 原生打包、Finder 和 WebKit 隔离修复；结果与证据范围见 [合并验证记录](macos-merge-validation.md)。下方保留完整复验步骤，新机器仍需实际执行。本说明只覆盖隔离测试模式，项目整体 macOS 支持边界见 [macOS 核心适配](macos-core.md)。

## 环境与构建

隔离测试模式要求 macOS 14+。当前打包目标为 Apple Silicon ARM64；Intel Mac 尚未验收。需要 Xcode Command Line Tools、Rust、Node.js 24、Git。只在新建的测试账号或没有私人数据的 Mac 测试环境执行下面步骤，勿复制任何实际账号、密钥、聊天或共享记忆。验收使用合成数据，不能通过扫描正式用户目录来证明隔离。

若使用不含 `.git` 的源码 ZIP，先按根目录 `MACOS-CODEX-HANDOFF.md` 完成校验和本地 Git 初始化。`npm test` 会验证启动器的 Git 可执行位，不能因压缩包缺少 Git 元数据而删除或跳过该检查。

```sh
cd /path/to/CrewVela-public
rustup target add aarch64-apple-darwin
npm ci
npm run build
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin --lib test_profile::tests -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin --lib secrets::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin --lib brand::tests
APPLE_SIGNING_IDENTITY=- npm run build:macos
```

`.app` 位于 `src-tauri/target/aarch64-apple-darwin/release/bundle/macos/`，`.dmg` 位于同级 `dmg/`。ad-hoc 签名仅用于测试，不代表 Developer ID 签名、公证或公开分发通过。系统安全提示按正常签名流程处理，不关闭 Gatekeeper，也不批量清除隔离属性。

## 启动入口

源码中的 `start-first-run-test.command` 应以 Git `100755` 模式和 LF 换行保存；Finder 可双击运行。若通过 ZIP/手工复制丢失可执行位，先只对这个文件运行 `chmod u+x ./start-first-run-test.command`，或使用下面的 Bash 入口：

```sh
bash ./start-first-run-test.command
```

默认按顺序寻找副本 ARM64 构建目录、通用 release 构建目录、启动器同目录里的 `CrewVela.app`。不会自动搜索或启动 `/Applications` 的正式安装，也不会通过 `open -a` 转交给已在运行的正常实例。找不到程序时明确报错，不创建测试数据；可以显式指定应用：

```sh
bash ./start-first-run-test.command \
  --app "$PWD/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/CrewVela.app" \
  --data-dir "$PWD/.test-data/mac-first-run-1"
```

相对参数按调用终端的当前目录解析；默认数据目录固定为启动器所在目录的 `.test-data`，不受 Finder 的当前目录影响。启动器直接运行 `Contents/MacOS/CrewVela --test-data-dir <绝对路径>`，终端显示原始启动错误并保留退出码。

## 数据隔离边界

- 应用配置、会话、项目与数据库保存在所选测试根的 `config/`；该目录不接管已有正式数据，不自动清空文件。
- WKWebView 不支持 `dataDirectory`。macOS 14+ 使用测试根的规范路径派生稳定的 16 字节 `dataStoreIdentifier`，覆盖默认存储与无痕配置。网页缓存/localStorage 的实际路径由 WebKit 管理，可能在用户 Library 下；不宣称所有 macOS 文件都位于 `.test-data`。
- 同一个测试目录重启使用同一网页存储；另一个全新目录使用不同身份。移动或删除应用配置目录不等于清除对应 WebKit 存储。不要通过批量删除 Library/WebKit 等目录重置测试；重验首次启动请新建测试目录。
- macOS 14 以下、系统版本探测失败或格式未知时，在创建配置及 WebView 之前拒绝隔离启动，退出码为 2；不回退到正式存储。普通模式系统要求不被本次门槛改写。
- 禁止旧目录迁移、系统凭据读取/写入/删除、全局 Skill 同步、原 Agent 历史导入、真实账号和共享记忆等外部集成。测试输入的 Key 只保存在进程内存。相应入口出现明确的隔离限制属于预期行为。
- `.test-data` 已加入 Git 忽略。自定义测试目录建议放在其下面；放在其他位置时自行确认忽略规则，避免发布测试设置。
- 直接双击 `.app` 属于普通模式，会按正常流程加载本机账号与历史；需要空数据效果时必须使用测试启动器。隔离模式是首次启动与本地配置验收环境，不是任意 Agent 执行的操作系统沙箱。

## Mac 原生复验清单

以下步骤在一次性 macOS 测试账号中运行，不读取实际用户的正式数据。需要比较正式/测试配置时，只创建标为 `synthetic` 的假项目、假会话或假偏好，不使用真实凭据。

回传报告已记录首次启动、持久化、第二目录、Finder 与路径保护通过；普通模式并存、旧系统和部分凭据 GUI 项仍未验收。不要将这份复验清单当作每一项均已通过的结果表。

1. **构建与权限**：确认 `uname -m` 为 `arm64`，`sw_vers -productVersion` 至少 14；执行上方构建与测试，记录真实退出码。确认 `git ls-files --stage -- start-first-run-test.command` 为 `100755`，`bash -n start-first-run-test.command` 通过。
2. **Finder 首开**：从 Finder 双击启动器；用一个从未用过的测试根。应显示“新用户隔离测试”、0 会话、无旧项目、首次通信引导。窗口不能白屏；留存截图。找不到 `.app` 时应显示可操作的错误，不能转为普通启动。
3. **网页存储持久化**：关闭通信引导，新增一个测试工作区或调整一项本地外观设置。关闭测试窗口后确认该测试进程退出；用相同目录重开，引导不重复、本地偏好保留。
4. **第二目录**：用 `--data-dir "$PWD/.test-data/mac-first-run-2"` 重开。应再次显示首次引导和默认偏好，不出现第一个测试根中的内容。然后重开第一个目录，原测试偏好仍在。
5. **普通模式并存**：只在一次性账号中创建有明显 `synthetic` 标记的普通配置，再保持普通实例打开并启动测试器。应出现独立的测试窗口；两个模式的会话/引导/偏好不相互串用。不得操作真实用户现有实例。
6. **凭据与账号**：测试模式打开账户面板，应显示隔离限制且不列出系统账号，不弹出 Keychain 授权。若输入合成的测试 Key，重启后应消失；不得用真实 Key 检查。账号、Skill、共享记忆和 Agent 启动操作应明确被拒绝。
7. **路径保护**：在专用临时目录构造非空普通目录、坏标记和指向合成目录的符号链接；通过 `--data-dir` 分别启动，应退出 2，不打开 WebView、不覆盖夹具。通过含中文和空格的正常新目录启动应成功。不要对真实配置目录运行此检查。
8. **旧系统保护**：如有 macOS 13 测试环境，指定一个尚不存在的测试目录启动，应退出 2，提示需要 macOS 14，且目录不被创建。没有该环境时保留“未验证”，不能拿版本解析单元测试冒充原生验收。
9. **结果记录**：逐项记录系统版本、硬件、构建版本、通过/失败与截图。任何 WebKit 数据串用、Keychain 提示、普通数据导入或无法退出均判失败；保留日志，不通过隐藏提示制造通过。

## 当前可用验证与限制

Windows 可以实际执行 Git Bash 启动器语法和参数测试，包括假 `.app`、中文/空格、字面量参数、缺少应用和退出码；Rust 测试可验证 Mac 配置字段、稳定/不同存储身份和版本拒绝策略。这些检查不执行 WKWebView 或 macOS 系统命令。

`.github/workflows/desktop-core.yml` 已覆盖 macOS ARM64 Node/Rust 测试、启动器可执行位检查与测试打包。当前没有推送或触发该工作流；配置存在不代表运行通过，CI 构建也不替代 Finder/Keychain/原生窗口验收。

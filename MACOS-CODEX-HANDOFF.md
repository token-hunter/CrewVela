# 交给 Mac 上 Codex 的构建与隔离验收说明

这是一份源码包，包含 Windows 工作区尚未提交的功能及 macOS 隔离支持，不是已经验证的 Mac 安装包。ZIP 不包含 `.git`、模型凭据、真实会话、共享记忆、测试状态、依赖或编译产物。请先解压到一个新的目录，不覆盖现有项目。

后续合并说明：当前仓库已接收 Mac ARM64 原生隔离修复，以及 v2 普通启动缓存冲突修复，摘要见 `docs/macos-merge-validation.md`。最新报告记录普通模式构建副本进入主页并正常退出，安装副本正常退出仍未完成验收；不能把前轮隔离通过当作普通模式稳定性证明。原先的源码 ZIP 保留为交付基线，不会因仓库修改自动更新；若当前是从 Git 获取的源码，不执行下面针对无 `.git` ZIP 的初始化步骤。每台新 Mac 仍需执行本说明并记录实际结果。

## 可直接发给 Codex 的任务

> 请先阅读当前目录的 `AGENTS.md` 和本说明，按下面步骤在本 Mac 完成 CrewVela 的测试、打包与隔离模式验收。先核对系统版本和原生 CPU 架构，再准备环境和本地 Git 元数据。保留有效检查，遇到失败定位并做完成 Mac 构建必需的最小修复；不要升级全部依赖或顺手重构。只使用本源码和独立测试目录，不扫描、读取、迁移或清理真实 Agent 历史、账号配置和 Keychain，不导入真实密钥，不自动启动普通模式。不要提交、推送、创建远程仓库或触发 CI。不要关闭系统安全保护。GUI 验收优先使用可用的原生界面工具；无法操作的步骤交给用户，并明确记为未完成。最后输出实际构建产物路径、SHA-256、修改清单、测试结果和未完成项，并将不含秘密的报告保存在 `.scratch/macos-build/验收报告.md`。

## 1. 解压与完整性

进入解压后的 `CrewVela-public` 根目录，不假设它位于固定用户名或固定磁盘。目录应包含 `package.json`、`src-tauri/Cargo.toml`、本说明和 `start-first-run-test.command`。

先核对随包的逐文件 SHA-256 清单：

```sh
shasum -a 256 -c SOURCE-MANIFEST.sha256
```

## 2. 系统与工具链

```sh
sw_vers -productVersion
uname -m
xcode-select -p
node -v
npm -v
rustup -V
rustc -vV
cargo -V
```

- **隔离测试要求 macOS 14+。** 低于 14 时停止隔离验收并记录，不能改成正式存储或普通模式。
- Apple Silicon 推荐使用原生终端，`uname -m` 为 `arm64`，Rust host 应为 `aarch64-apple-darwin`。若显示 `x86_64`，先确认机器是否为 Intel，或终端/Rust 是否运行在 Rosetta；不要把 Rosetta 构建报告为原生 ARM64 通过。
- 使用 Node.js **24.x** 和 Rust stable；Cargo 的最低工具链约束见 `src-tauri/Cargo.toml`。已有其他项目时不随意更改系统默认版本或全局配置，可使用既有版本管理工具。
- 缺少 Xcode Command Line Tools 时执行 `xcode-select --install`，等待用户完成系统安装界面；安装请求不等于已经安装。需要系统授权时由用户处理。
- Node 官方下载：<https://nodejs.org/en/download>；Rust 官方安装：<https://rust-lang.org/tools/install/>；Tauri 环境：<https://v2.tauri.app/start/prerequisites/#macos>。

只安装缺少的工具，安装后重新核对版本。不要复制 Windows 的 `node_modules` 或 `target`，也不需要为了空数据验收安装/登录任何 Agent CLI。

**工具链就绪后再初始化本地 Git。** ZIP 没有 `.git`，但 `npm test` 中的启动器检查需要 Git 记录的可执行位。这是正常准备，不要跳过检查或改测试来绕过它。在这个新解压目录执行：

```sh
git init -b macos/source-verification
chmod u+x ./start-first-run-test.command
git add --intent-to-add --chmod=+x -- start-first-run-test.command
git ls-files --stage -- start-first-run-test.command
bash -n ./start-first-run-test.command
```

预期 Git 模式是 `100755`。这里只创建本地仓库并记录 intent-to-add，不创建提交，不配置 remote。若该目录意外已有 Git 仓库，先确认解压位置，不覆盖其索引或历史。ZIP 已写入 Unix 执行位；`chmod` 是针对解压工具丢失文件模式的单文件修复。

## 3. 依赖与测试

每条命令单独执行并检查真实退出码。可在 `.scratch/macos-build/` 保存日志；不要用忽略退出码、跳过测试、修改锁文件或隐藏异常制造成功。若使用管道写日志，应启用 `pipefail`。

设置本次终端使用的目标，二选一，不写入全局环境：

```sh
# Apple Silicon 原生构建：
MAC_TARGET=aarch64-apple-darwin

# 只有确认是 Intel Mac 时，改用这一行。Intel 仍属未验收架构：
# MAC_TARGET=x86_64-apple-darwin
```

然后执行：

```sh
rustup target add "$MAC_TARGET"
npm ci
npm run build
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml --target "$MAC_TARGET" --features custom-protocol --lib test_profile::tests -- --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml --target "$MAC_TARGET" --features custom-protocol --lib secrets::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --target "$MAC_TARGET" --features custom-protocol --lib brand::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --target "$MAC_TARGET" --features custom-protocol --bins only_desktop_empty_invocation_can_start_gui
cargo test --locked --manifest-path src-tauri/Cargo.toml --target "$MAC_TARGET" --features custom-protocol --test packaged_assets -- --ignored
```

先运行资源集成测试再启动 GUI，避免可执行文件占用干扰构建。Windows Node 基线为 425 通过、1 项既有跳过；最新 Mac 回传 Rust 筛选为隔离 8 项、凭据 5 项、品牌目录兼容 5 项、实例锁 3 项、CLI 1 项及资源集成 1 项。最新合并回归见 `docs/macos-merge-validation.md`。Mac 必须自行执行并记录，不照抄基线。

当前隐私加固工作树已更新 npm 依赖，Windows 复核全量及生产审计均为零告警；私有备份中的旧标签与旧附件不包含这些更新。Mac 仍需自行执行 `npm audit` 并保留实际结果，不直接执行 `npm audit fix --force` 或批量升级。

本轮还需验证发行路径映射与依赖发现。构建使用下面的 npm 命令，完成后按 [源码公开与本地数据边界](docs/publication-privacy.md) 扫描最终应用；记录安装副本启动及退出情况。不要将旧 DMG 改名后当作本轮构建结果。

## 4. 生成 Mac 应用和 DMG

Apple Silicon 使用项目已有命令：

```sh
APPLE_SIGNING_IDENTITY=- npm run build:macos
```

ARM64 预期产物：

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/CrewVela.app
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/*.dmg
```

只有确认是 Intel Mac 时，使用显式 Intel 目标（该路径仍需验收）：

```sh
APPLE_SIGNING_IDENTITY=- npm run tauri -- build --target x86_64-apple-darwin --features custom-protocol --bundles app,dmg
```

Intel 产物在 `src-tauri/target/x86_64-apple-darwin/release/bundle/`。`build:macos` 固定为 ARM64，不要在 Intel 上把它当成本机架构命令。

`APPLE_SIGNING_IDENTITY=-` 是本地 ad-hoc 测试签名，不使用开发者证书，不代表 Developer ID 签名或公证通过。遇到系统安全提示按正常签名/授权流程处理，不关闭 Gatekeeper，不批量删除 quarantine 属性。DMG 打包失败也算未完成，不能只因 `.app` 存在就报告全部成功。

## 5. 用独立数据启动，验证首次打开

Apple Silicon：

```sh
bash ./start-first-run-test.command --data-dir "$PWD/.test-data/mac-first-run-1"
```

Intel 需要显式指定刚构建的应用：

```sh
bash ./start-first-run-test.command \
  --app "$PWD/src-tauri/target/x86_64-apple-darwin/release/bundle/macos/CrewVela.app" \
  --data-dir "$PWD/.test-data/mac-first-run-1"
```

**不要直接双击 `.app` 或从 DMG 普通启动来做首次隔离验收。** 普通模式会正常读取本机配置；本任务没有授权读取真实历史。启动器会直接执行 `.app/Contents/MacOS/CrewVela --test-data-dir ...`。

Mac GUI 验收优先在独立的 macOS 测试账户中进行；当前没有完成原生隔离验收，不能先承诺绝对不接触原数据。只用新建测试目录和合成数据，不通过读取正式用户目录来证明隔离。

最少检查：

1. 窗口顶部为“新用户隔离测试”，会话 0、无旧项目、出现首次通信引导，没有白屏。截图仅包含测试窗口。
2. 关闭引导，关闭测试窗口后确认该测试进程退出；使用相同目录重开，引导不重复。
3. 将测试目录改为全新的 `mac-first-run-2` 再启动，应重新出现首次引导，不串用前一个目录的偏好；再回到第一个目录，原测试状态仍保留。
4. 账户入口显示隔离限制、不列出真实账号、不弹出 Keychain 授权。真实 Agent、Skill、共享记忆等操作被拒绝属于预期；不要改白名单来“修复”这些限制。
5. 对专用临时目录里的非空/坏标记/符号链接合成夹具，应明确拒绝且不修改夹具；不要对真实历史路径执行检查。
6. 若需验证与普通模式并存，只在一次性测试账户中创建假配置后执行；没有该条件就明确记为未验证，不接触当前真实账号。

Mac 使用测试目录规范路径派生的专属 WebKit 存储身份；网页存储实际位置由系统管理，可能位于 Library 下，不保证全部在 `.test-data`。配置和会话在测试根的 `config/`。不要删除 Library/WebKit 或 Keychain 来重置；需要全新状态时换一个全新的测试目录。

更完整步骤见 `docs/macos-isolated-test.md`；整体平台限制见 `docs/macos-core.md`。电脑控制、共享记忆托管安装包等尚未支持的能力，不属于本次自动补全范围。

## 6. 交付结果

在 `.scratch/macos-build/验收报告.md` 记录：

- macOS 版本、CPU 架构、Rust host、Node/Cargo 版本，不收集序列号、真实账号或密钥。
- 每项命令的实际退出码、通过/失败/跳过情况，以及最小必要修复和受影响回归。
- `.app`、`.dmg` 的实际绝对路径；用 `shasum -a 256` 分别计算应用内部 `Contents/MacOS/CrewVela` 和实际 DMG 的 SHA-256。
- Finder 首开、持久化、第二目录隔离、Keychain 保护等每项的实际验收状态；可用时附测试窗口截图。
- 仍存在的编译警告、依赖漏洞、签名/公证限制和未完成项。禁止把浏览器预览或配置单元测试当成真实 Mac GUI 验收。

本源码包不要求提交或推送。若 Mac 上有必要修复，保留工作区改动和差异，交给用户审阅后再决定如何回传。

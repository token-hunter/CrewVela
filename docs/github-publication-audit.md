# GitHub 发布文件与敏感数据审计

日期：2026-11-07。对象：当前项目根仓库，基线 `74ad83a`，实施分支 `security/github-publication-audit`。本报告记录清理提交前的审计结果；后续清理提交不消除历史残留。本轮未推送或改写历史。

## 结论

当前公开候选源码未发现真实 API key、密码、私钥或可用登录 token；3 个疑似私人邮箱已从账户解析测试中替换为 `example.com` 示例。模型密钥原有系统凭据库管理方式保留，没有将其导出到明文文件。

**现有 Git 历史仍不能直接当作已清理的公开版本推送。** 祖先提交含个人邮箱、本地截图和已删除的模型文件；当前文件修改、取消跟踪及 `.gitignore` 都不会清除历史。建议从清理后的源码建立新的独立公开仓库，保留原本地历史。不要复制原 `.git`、使用 mirror 或推送全部 refs。

## 扫描范围与方法

- 基线跟踪 740 个文件。根工作树清单包含未跟踪的本地 QA 文件和隐藏配置；源码、测试、文档、脚本、工作流和配置进行常见密钥格式、敏感赋值、URL 凭据、JWT、私钥与邮箱模式检查，并人工核实命中上下文。
- HEAD 可达 351 个提交，全部 refs 可达 360 个提交，3332 个不同 blob、约 241 MB。对所有 blob（包括二进制与大型 ONNX）检查标准服务商密钥、GitHub/AWS/Google/Slack token、私钥和 JWT；对文本补查字面量凭据、邮箱，另查提交消息和作者元数据。
- 使用 Gitleaks v8.30.1，下载自官方发布并核验官方 SHA256；默认规则扫描全 refs 的提交差异，再扫描按当前 Git 跟踪范围与非忽略新增文件导出的独立源码快照。扫描日志和 JSON 报告均使用完整脱敏，只保留在被忽略的 `.scratch/github-publication-audit/`。
- 默认规则历史扫描发现 1 个误报：`tests/browserControlLazyBridge.test.mjs` 中用于临时 loopback 测试服务的固定合成 UUID。`.gitleaks.toml` 保留所有默认规则，仅在该路径与完整合成值同时匹配时放行。已验证相同值换路径、相同路径换值仍会报告。
- 应发布的源码快照没有 `.git`、`.env` 或本地备份。最终候选文件数、扫描及回归结果见后文。

## 不需上传的内容

以下类别已由 `.gitignore` 保护；规则中的源码例外经正负例测试验证。

| 类别 | 主要文件或目录 | 原因 |
| --- | --- | --- |
| 依赖和构建 | `node_modules/`、虚拟环境、`dist/`、`src-tauri/target/`、`src-tauri/gen/`、`*.tsbuildinfo` | 可从源码和锁文件重新生成 |
| 本地输出与备份 | `output/`、`outputs/`、`.scratch/`、`artifacts/`、`demo/`、`tmp/`、`temp/`、`$dest/` | 包含个人实验、备份、截图和运行输出 |
| 工具缓存 | `.codegraph/`、Playwright 缓存、测试报告、覆盖率、Python 缓存、日志 | 本机状态，发布无必要 |
| 环境和凭据 | `.env`、`.env.*`（保留 `.env.example`）、`.envrc`、`secrets/`、`credentials.json`、`auth.json`、包管理认证配置 | 可能包含访问凭据 |
| 私钥和登录状态 | 私钥/签名容器、cookies、浏览器 storage state、SQLite/DB 及伴随日志 | 可能含私钥、账号、会话或聊天数据 |
| Agent 本机状态 | `.claude/settings.local.json`、任务/邮箱/工作树状态、`.codex` 会话与登录文件、Impeccable 本地缓存 | 含本机权限、路径或执行记录 |
| 下载及安装文件 | `*.exe`、`*.msi`、`*.dmg`、`*.onnx`、`*.safetensors`、`*.gguf` 等 | 构建或下载产物；发布安装包应走 Releases |
| 根目录个人过程记录 | `task_plan.md`、`findings.md`、`progress.md`、`design-qa.md`、指定 DeepSeek 交接记录 | 非产品文档，含本机工作过程 |
| 一次性 QA 文件 | 根目录 `qa-*.png`、`baidu-*.png`、`qa-*.mjs` | 本地探针和捕获；已有两份可复用回归脚本明确放行 |

5 个已跟踪的根目录过程记录已用 `git rm --cached` 移出索引，文件仍在本机。这些取消跟踪操作在清理提交的差异中表现为删除；未删除本地文件。原有未跟踪 QA 脚本和截图同样保留。

继续保留：`src/`、后端源码、`tests/`、`scripts/`、`tools/`、`skills/`、CI、`package-lock.json`、`Cargo.lock`、打过补丁的 `src-tauri/vendor/portable-pty/`、LICENSE/第三方许可、应用图标和实际使用的图片素材、架构/领域文档，以及 `pet-preview.html`。`qa-general-settings.mjs`、`qa-kimi-computer-use.mjs` 是现有可复用回归脚本；`.impeccable/config.json` 是有效检查配置，均明确保留。没有笼统忽略 PNG、JSON、Markdown 或整个测试目录。

## 敏感信息及处理

| 发现 | 处理和状态 |
| --- | --- |
| `src-tauri/src/codex_accounts.rs` 账户表测试含 3 个疑似私人邮箱 | 替换为 first/second/third 的 `example.com` 地址，保留对应关系并增加账户解析、错误归属断言 |
| 两个 Rust 诊断例子含开发者机器的 Claude 路径，图片诊断固定写项目绝对路径 | 改 PATH 默认与 `CREWVELA_CLAUDE_EXE` 覆盖；输出改系统临时目录，编译验证通过 |
| Provider API key 管理代码 | `src-tauri/src/secrets.rs` 使用系统 keyring，配置里的 `auth_token_ref` 是引用，不是作者的密钥；保持原实现 |
| 共享记忆和 ZCode 可选本机秘密 | 已从进程环境读取 `AI_MEMORY_AUTH_TOKEN` / `ZCODE_CREDENTIAL_SECRET`；无作者凭据常量，不需要搬迁真实密钥 |
| 测试中的 password/token、带用户密码的拒绝样例 URL | 确认为合成夹具，保留安全回归覆盖；没有将字段名称或占位符误认作真实泄漏 |
| Kimi OAuth client ID | 公共客户端标识，不是客户端秘密，保留 |
| 文档中的公开产品联系地址、依赖作者和 Git SSH 地址 | 不属于本机账号凭据，保留来源信息；`@2x` 图标路径不作为邮箱 |

新增 `.env.example` 仅含注释和空变量说明；本地 `.env` 仅在不存在时由模板创建，已验证被忽略，没有将现有系统凭据导出。普通 Tauri/Rust 启动不会自动读取 dotenv 文件；README 提供了已验证的 Node 24 启动入口：

```powershell
node --env-file=.env --run tauri -- dev
```

模板只用于开发时对应的可选服务配置，模型 key 仍由应用设置和系统凭据库管理。不要把秘密设为 `VITE_*`，也不要把 `.env` 放进安装包或构建资源；前端变量会进入用户可读取的构建结果。

## 历史残留与发布方式

- 私人邮箱从 `63157ab` 引入；`CodexAccountsPanel.tsx` 的祖先版本也包含额外私人邮箱。当前树已清理，旧提交仍可读。
- `.scratch/ui-audit/01-main.png` 等本地截图曾于 `3310fc7` 加入、于 `0b8bf5a` 删除。未对历史截图进行视觉/OCR 隐私验收，不应直接发布这部分历史。
- 约 43 MB 的 ONNX 模型曾于 `2029a1d` 加入、于 `0b8bf5a` 删除，仍在祖先对象中。
- 部分根目录 QA 文件仅在本地 checkpoint refs 中出现，HEAD 不可达；推送全部 refs 或镜像仓库会扩大公开范围。
- 提交身份元数据仅见 `CrewVela.local` / `local` 本地标识域，本次未发现个人邮件服务地址。私人邮箱残留位于历史文件内容。
- 忽略目录 `output/` 下已有其他公开版副本；本轮修改适用于当前根仓库，没有覆盖这些副本，也没有修改其各自的 `.git`。若从副本内部推送，应针对该仓库另做检查。

发布准备应导出当前审定的跟踪文件及本任务新增源码，排除所有忽略项，在新的空目录建立 Git 历史，再检查文件清单和扫描结果。不要直接复制整个项目文件夹或原 `.git`。若以后选择保留旧历史，需要另行决定历史清理和协作者同步方案；本轮没有擅自重写原仓库。

## 验证结果

- 新增忽略规则正负例、空环境模板检查：2/2 通过。`git ls-files -ci --exclude-standard` 为空，当前索引没有被忽略规则遗漏保护的已跟踪文件；5 个本地笔记仍存在。
- Node 全量回归：387 项，386 通过、1 项既有跳过、0 失败。
- 账户后端定向回归：12/12 通过，新增断言覆盖邮箱解析及刷新错误归属。
- `cargo check --examples` 通过；保留现有 Rust 编译警告。
- Windows 诊断默认 PATH 解析经 ConPTY 的无费用 `--version` 启动验证通过（按 vendored portable-pty 参数复现，npm CMD shim 和原生 EXE 对照均退出 0）；未运行真实诊断 TUI 交互。
- `npm run build` 通过；保留 Vite 大于 500 kB 的 chunk 提示，没有修改阈值。
- `node --env-file=.env --run tauri -- --help` 通过，环境文件入口能够启动本机 Tauri CLI；没有启动模型请求。完整 GUI 开发会话未单独运行，本轮未修改产品界面。
- Gitleaks 默认历史扫描：1 个已核实的合成 UUID 误报；精确例外配置后：0 命中。例外正负控制：3/3 通过。
- 扫描器控制实验曾使用随机 UUID，末次随机值未被默认通用规则识别，导致控制断言失败；报告保留为 `scanner-random-control-miss.json`。已改为与原合成值字符频率和熵相同、顺序不同的固定异值来验证例外，未降低扫描器阈值或断言。该失败也说明不能依赖单一通用密钥规则识别任意随机串。
- 完整 Rust 库回归：982 通过、32 项既有忽略、0 失败；运行时长 174.90 秒。真实模型/登录等 opt-in 测试仍未运行。
- 最终公开候选源码快照：740 个文件，Gitleaks 0 命中；不包含 `.env`、5 个本地笔记、QA 捕获或 Git 元数据。
- 工作树及暂存差异 `git diff --check` / `git diff --cached --check` 通过。Git 提示部分 Rust 文件将在下次操作时转换 CRLF，本轮没有批量更换行尾。

安装 Gitleaks 后可使用项目配置复查已提交历史；此命令不替代对未提交候选文件的独立快照扫描，也不检查个人邮箱：

```powershell
gitleaks git . --log-opts="--all" --config .gitleaks.toml --redact=100
```

原始日志：`.scratch/github-publication-audit/`；无需提交。

## 识别边界

扫描没有发现真实密钥，不等于证明不存在任意自定义格式的秘密。当前素材和历史图片没有逐张 OCR，第三方依赖内部、已忽略的安装包/缓存/备份没有做内容级全面审计或清除；它们不属于根仓库公开候选文件。历史二进制只进行了字节模式检查。未访问真实模型、账号或系统凭据库，也未执行 GitHub 推送。本轮不构成第三方素材授权或跨平台运行验收。

Gitleaks 的规则扩展和精确例外语法依据 [官方 v8.30.1 配置说明](https://github.com/gitleaks/gitleaks/blob/v8.30.1/README.md#configuration)。

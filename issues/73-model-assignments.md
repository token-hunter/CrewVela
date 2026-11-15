# 模型设置：统一分配表

Status: ready-for-human

## 已选方案

用户选择优化后的第 2 版「精简分配表」。视觉参考：`C:/Users/PC/.codex/generated_images/01a1daa0-c36a-7e00-accd-28836a36169f/exec-30363e89-dc5f-4c29-ac0e-7caf7fb75ced.png`。

## 范围

- 默认、项目成员与会话配置在一个表格中查看、筛选、编辑和恢复继承。
- 连接和模型分别保存，复用现有 Provider 管理、成员模型字段、会话模型覆盖和 CLI 启动实现；保留旧配置兼容。
- 保存默认不修改运行中的会话；可靠支持的会话状态才允许修改启动覆盖，明确展示已保存配置与启动记录的区别。
- 保留真实能力边界，不将 Claude Provider 假装成任意 Runtime 的通用连接。原生聊天/专用适配器不支持的设置只读并说明。
- 验证优先级、持久化失败、旧数据、并发更新、CLI 参数、界面筛选/编辑/失败重试与共享设置布局。

## Comments

- 任务开始前已有修改保存在 `.scratch/model-assignments/baseline.patch`，不回滚或提交其他工作。

## 实际结果

- 已按第 2 版实现统一分配表，保留现有设置导航和连接管理。支持 5 种 CLI 默认、项目成员覆盖及安全状态下的会话启动覆盖。
- 连接与模型分离存储；默认使用原子文件写入，成员使用同一 SQLite 事务更新，旧 Claude 服务名配置兼容；保留名称、职责、任务及会话关系。
- 已接入项目任务执行、团队消息唤醒、普通 CLI 启动/恢复及 Claude 自然语言解析默认。会话记录启动参数，不冒充 CLI 内实时切换后的实际模型。
- 配置含义、继承规则和未适配范围见 `docs/model-assignments.md`。设计对比见根目录 `design-qa.md` 当前任务段。

## 验证

- `npm run build` 通过；保留现有大 chunk 警告。
- `npm test`：494 通过，1 跳过，0 失败。
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --no-default-features -- --test-threads=1`：1130 通过，48 原有忽略项，0 失败；包含 11 项模型分配回归。
- 完整并行 Rust 测试曾在共享记忆 HTTP 下载测试中出现超时、连接重置或重试次数差异；未改该模块或放宽断言，串行全量复核通过。日志：`.scratch/model-assignments/rust-all-final.log`、`rust-all-serial.log`。
- `modelAssignments.ui.mjs`、`settingsNavigationLayout.ui.mjs`（6 组）、`providerRowLayout.ui.mjs`、`uiThemeContrast.ui.mjs`（18 组）、`appSessionAsync.ui.mjs` 通过；保留删除失败可见和服务行保留断言。
- IAB 已实际检查桌面编辑与窄窗口，截图和对比保存在 `.scratch/model-assignments/`；只使用隔离数据，不改真实账号、服务配置或业务项目成员。
- 已生成 Windows 单文件 EXE；正常 Windows 模式已验证 Claude GLM 与 Codex Terra 的保存、启动和真实响应，详见下节。未打包 DMG，未验证 macOS 原生运行。正在运行的旧版本不会因源码变更自动更新。

## Windows EXE（2026-11-15）

- 用户要求打包测试，按 `npm run tauri -- build --no-bundle` 构建。原 release EXE 被正在运行的旧版锁定，首次构建失败；复制构建缓存至独立 `CARGO_TARGET_DIR` 后，同一命令构建成功，保留旧进程。
- 交付：`output/CrewVela-model-settings-20261115/CrewVela.exe`，46,350,336 字节；SHA256：`BD04B71D40A5C04F67641DE99C70B0470EA84877185F4ACDCF748F72EF4474E5`。
- `npm run test:desktop-assets` 1 项通过，内嵌入口和 JS/CSS 存在；EXE `--version` 返回 `CrewVela 0.1.0`，发布路径隐私检查通过。构建保留 50 项编译警告，未为打包修改产品代码。
- 用 `--test-data-dir` 启动独立原生实例，确认 `http://tauri.localhost/` 页面和新模型设置布局加载。现有隔离模式 allowlist 拦截新模型配置 IPC，显示明确保护提示，因此仅算原生启动与页面加载检查，未宣称模型配置原生端到端通过；没有放开保护规则或改真实数据。
- 原生证据：`.scratch/model-assignments/windows-native-smoke.png`；构建/资源/隐私/产物日志位于同目录。核验后已终止自己创建的隔离进程，原用户实例保留。

## 正常模式真实模型验收（2026-11-15）

- 正常 EXE 界面保存默认后，新建成员继承并执行真实请求：Claude 响应模型 `glm-5.3-flash`，Codex 原生 `turn_context.model` 为 `gpt-6.1-terra`（本机默认仍为 Sol），两者返回唯一标记。
- Codex 协作自动投递未通过，人工在同一个受管 CLI 中提交请求后模型响应成功；随后出现完成 Hook 退出码 1。详见 `issues/74-codex-live-delivery-hook.md`，未把人工提交计作自动投递成功。
- 默认覆盖已从界面恢复，两个测试会话已停止、待发消息为零，独立项目暂停；原有 79 名成员的配置未变，业务目录为空。
- 详细过程、准确会话 ID 和证据清单：`docs/model-assignments-live-verification-20261115.md`。此次验证未修改产品代码。

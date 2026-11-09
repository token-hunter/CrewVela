# CrewVela 设计文档

> Claude Code 多模型启动器 — Tauri 2 桌面应用
> 日期：2026-06-13

## 1. 目标

让用户在一个桌面 GUI 里管理多个 Anthropic 兼容的 AI provider，点一下就在内嵌终端里用对应环境变量拉起 `claude` CLI，免去手改 shell 配置、明文存 key 的麻烦。

参考来源：Reddit r/ClaudeCode《How to Set Up Claude Code with Multiple AI Models》（作者 ThreeKiloZero）的 shell 函数方案，将其 GUI 化、安全化、跨平台化。

## 2. 非目标（YAGNI）

- 不做 AI 对话界面——终端即界面，`claude` 本身负责交互。
- 不做代理/路由层（不走 Claude Code Router）——直连 provider 原生 Anthropic 端点，保持最快。
- 不替代 `claude` CLI——它是依赖，不是被替代物。

## 3. 架构

三层：

```
┌─────────────────────────────────────────────┐
│ 前端 React+TS+Vite                           │
│  ProviderList │ Terminal(xterm.js) │ Editor  │
└───────────────┬─────────────────────────────┘
                │ Tauri invoke / event
┌───────────────┴─────────────────────────────┐
│ Rust 后端 (Tauri 2)                           │
│  providers.rs │ secrets.rs │ launcher.rs     │
│                          │ pty.rs            │
└───────────────┬─────────────────────────────┘
                │ spawn (PTY + env 注入)
                ▼
          claude CLI 进程
```

### 3.1 组件职责

| 模块 | 职责 | 依赖 |
|---|---|---|
| `providers.rs` | 读写 provider 配置（JSON），含 3 个预置模板 | serde, dirs |
| `secrets.rs` | API key 存取，走系统 keychain | keyring |
| `launcher.rs` | 组装 env、unset `ANTHROPIC_API_KEY`、探测 claude 路径、spawn | std::process, portable-pty |
| `pty.rs` | PTY 会话生命周期、字节流与前端双向桥接 | portable-pty, tauri event |
| 前端 `Terminal.tsx` | xterm.js 渲染 + 输入回传 | @xterm/xterm |
| 前端 `ProviderList.tsx` | provider 列表与选中态 | — |
| 前端 `ProviderEditor.tsx` | 编辑 provider 详情（含 model 映射）| — |

### 3.2 数据流（启动一个 provider）

1. 用户点 Provider → 前端 `invoke('launch_provider', { id })`
2. Rust：`providers::get(id)` 取配置 → `secrets::get(id)` 取 key（缺失则报错）
3. `launcher::build_env(provider, key)` 组装环境变量 map
4. `launcher::resolve_claude()` 探测 claude 可执行路径（PATH / `which`）
5. `pty::open(size)` 建 PTY → spawn claude（注入 env、设 cwd）
6. PTY stdout → `app.emit('pty:data', bytes)` → xterm.js 写
7. xterm.js 输入 → `invoke('pty:input', { bytes })` → PTY stdin
8. 退出/关闭 → 清理会话

## 4. 数据模型

### Provider 配置（JSON，存 `~/.CrewVela/providers.json`）

```jsonc
{
  "version": 1,
  "providers": [
    {
      "id": "glm",
      "name": "Z.AI GLM",
      "baseUrl": "https://api.z.ai/api/anthropic",
      "authTokenRef": "glm",          // keyring 里的 service 名
      "env": {
        "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-4.6",
        "ANTHROPIC_DEFAULT_SONNET_MODEL": "glm-4.6",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.5-air"
      },
      "timeoutMs": null,
      "disableNonessentialTraffic": false,
      "builtin": true
    }
  ]
}
```

### keychain 条目

- service: `CrewVela`，account: provider id（如 `glm`），secret: API key。
- 使用 `keyring` crate，Windows → Credential Manager，macOS → Keychain，Linux → Secret Service。

## 5. 关键实现细节

1. **避免 key+订阅冲突**：第三方 provider 启动前显式从 env 移除 `ANTHROPIC_API_KEY`（对应文章里 `unset`）。
2. **Windows PTY**：portable-pty 自动选用 ConPty（Win10+）。前端调整大小时发 resize 命令。
3. **claude 路径探测**：`which claude`；Windows 上解析 `.cmd`/`.ps1` 包装。
4. **预置模板**（来自原文）：
   - GLM: `api.z.ai/api/anthropic`，model `glm-4.6` / `glm-4.5-air`
   - Kimi: `api.kimi.com/coding/`，model `kimi-for-coding`
   - MiniMax: `api.minimax.io/anthropic`，model `MiniMax-M2`，timeout 3000000，关闭非必要流量
5. **自定义 provider**：用户填 baseUrl + model 映射 + key，存为普通条目。

## 6. 错误处理

- key 缺失 → 返回结构化错误，前端弹"未设置 API key，是否现在录入？"
- claude 未找到 → 启动前检测，提示安装路径
- PTY spawn 失败 → 显示 stderr
- 终端重连：PTY 进程退出 → 前端显示退出码，保留输出历史

## 7. 测试策略

- **Rust 单测**：`launcher::build_env` 的 env 组装逻辑（含 unset、合并）、预置模板序列化。
- **前端**：组件渲染（如时间允许补 vitest）。
- **手动验收**：Windows 11 实跑 GLM/Kimi/MiniMax。

## 8. 环境前置（运行/编译所需）

- Rust 工具链（`rustup`）+ MSVC C++ 构建工具（Windows）
- Node 18+（开发前端）
- WebView2（Windows 11 自带）
- 本机已安装 `claude` CLI

> 当前开发机缺 Rust 工具链，代码可完整写出，编译运行需先补 Rust。

## 9. 分阶段计划

1. **P0 骨架**：Tauri 工程 + 前后端 hello，前端能 invoke 到 Rust。
2. **P1 配置与密钥**：providers.json 读写 + keyring CRUD + 3 模板。
3. **P2 终端**：portable-pty 桥接 + xterm.js 双向流。
4. **P3 启动**：env 组装 + spawn claude + unset 旧 key。
5. **P4 打磨**：错误处理、resize、退出码、README 与安装指引。

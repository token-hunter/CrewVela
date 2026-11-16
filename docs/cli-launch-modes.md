# CLI 启动模式

在“管理 Agent”的 CLI 列表内设置各 Agent 的默认启动模式。新建 CLI 会话时可以临时覆盖；只有勾选“设为默认”才会保存该选择。所有新用户默认使用“CLI 默认”，沿用原生配置。

模式目录共用 `src/runtimeLaunchModes.json`，实际参数在 `src-tauri/src/runtime_launch.rs` 中接通；目录中的每个选项均经过后端映射测试。仅影响六类独立 CLI，不代替原生 Chat 和项目协作权限配置。

| CLI | 提供的选择 |
| --- | --- |
| Claude Code | CLI 默认、Manual、Accept Edits、Plan、Auto、YOLO |
| Codex | CLI 默认、工作区写入、只读沙箱、Approve for me、YOLO |
| Kimi Code | CLI 默认、Ask When Needed（--yolo）、Never Ask（--auto）、Plan |
| ZCode CLI | CLI 默认、Guarded、Build、Edit、Plan、YOLO |
| OpenCode | CLI 默认、Build、Plan、Auto |
| Pi | CLI 默认、仅文件读取工具 |

这些名称按本机 CLI 的真实参数定义，不能按同一英文名字推断跨 CLI 权限含义。Pi 的文件读取选项限制 read/grep/find/ls 工具，并关闭自动扩展和 MCP，不是系统沙箱。OpenCode 的 Build/Plan 选择原生 Agent，保留用户对这些 Agent 的配置。CLI 版本或账号不支持某模式时，保留其真实启动错误，不自动切换到权限更大的模式。

默认值保存在应用 `runtime-preferences.json` 的 `defaultLaunchModes` 中，和启用状态一同原子保存。它不修改原生 CLI 的账号、模型或全局配置。

新建时将确定的选择随会话一次性持久化为 `cliLaunchMode`，之后恢复使用会话自己的选择。修改默认值不会改写已有会话，旧记录缺少此字段时保持原来的启动行为。会话交接由既有交接权限约束接管，不以此默认值放宽。

终端标题中的“启动：…”表示创建时的启动选择。之后在原生 CLI 内切换模式，以 CLI 自身显示为准；选择“CLI 默认”时实际权限由原生配置决定。

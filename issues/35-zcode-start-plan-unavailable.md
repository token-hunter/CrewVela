Status: ready-for-human

# 暂停提供当前无法调用的 ZCode Start Plan 模型

- 用户反馈 Start Plan 所有选项均无法聊天，希望无法解决时移除。
- 当前真实最小请求再次失败；既有原生诊断明确返回 ProviderBusinessError / 3012 / unusual activity。权益目录可用并不等同于模型可调用。
- 当前接入暂不提供 Start Plan，返回明确的集成不可用状态；保留个人 Coding Plan、自定义 API、原生账号配置和历史。
- 已保存 Start Plan 选择不能静默改为其他收费模型；刷新后清除无效选择，要求用户明确选择。
- 回归账号 Host、旧授权拒绝、个人模型目录/真实回复、模型选择与界面；按既定命令构建，不提交、不推送。

## 实际结果

- 当前真实最小 Start Plan 请求失败，未将服务端限制当作已修复。此次交付是暂停暴露不可用集成，不改写原生账号数据。
- Host 不再查询 Start Plan 权益、不发布对应模型与授权，并拒绝旧缓存授权。原生协议沿用其支持的 unavailable/not-entitled 状态，私有提示标识留在 Host 内部。
- 首轮安装版验证发现新增 unavailableReason 枚举被原生拒绝（-32602），已改正；复测真实目录为 6 个个人套餐选项，旧 Start Plan 模型 ID 被拒绝。
- 个人套餐 GLM-5.3 的真实模型调用成功，有助手回复及 completedSuccess 终态。没有重试或绕过 Start Plan 风控。
- Node 全量 420 通过、1 原有跳过；Rust ZCode 相关 67 通过、13 默认忽略。两个需要安装/账号的检查另行运行通过：真实目录排除及 GLM-5.3 回复。
- 真实 ZCodeChat 组件的隔离界面检查通过：刷新移除旧 Start Plan，历史保留，未选模型禁止发送，明确选择个人模型后发送正常。截图使用模拟数据，位于 `.scratch/zcode-start-plan-unavailable/model-refresh.png`；不充当真实模型验收。
- `npm run tauri -- build --no-bundle` 成功，输出 `src-tauri/target/release/CrewVela.exe`，保留已有构建警告。未在打包窗口重验交互，也未验证 macOS/Linux。
- 本次提交只包含 Start Plan 暂停集成及相关测试，其他未提交的账号修复、会话恢复与界面改动保留在工作区。上述全量回归及真实模型验收针对当前工作区；独立暂存版本的账号 Host 专项另行验证 8/8 通过。不推送。

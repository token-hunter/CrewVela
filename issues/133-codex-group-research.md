Status: ready-for-human

# 群聊恢复 Agent 原生能力与成员选择

用户最初要求修复上一轮拒绝 Codex CLI 的问题，随后明确纠正需求：保留 Agent 原本所有能力，仅增加角色设定；不需要群聊统一禁工具、禁联网或另造受限工具通道。当前范围以这条新指示为准，撤回上一轮的默认严格群聊设计。

角色只定义专长、分工和沟通风格；普通群聊沿用 CLI 自带工具、联网、Skill、MCP、模型/供应商及权限配置。当前用户明确的只分析、不修改、不使用工具等要求仍随本次消息传递；不擅自将默认权限改为 YOLO。旧的已排队/运行请求保留原执行约定，新的界面请求使用原生能力。

当前工作区有既有改动，保留；本任务不提交 Git，不直接迁移用户生产配置。

## 实施

- 恢复全部八类当前协作 Runtime 的选择，保留用户明确停用 Runtime / 人工接管的限制；Codex 和 ZCode CLI 不再因只读适配白名单被置灰。
- 新请求保留原生能力，群聊项目 CWD 使用当前项目目录，角色和项目目标作为分工背景；历史验收约定不当作日常能力总开关。
- Claude/Codex/Kimi/OpenCode/DeepSeek 复用既有 ACP 通道；默认权限不强制切模式，需要许可的原生操作沿用现有待处理事项/审批流程。Pi 保留 CLI 原有扩展、Skill、MCP，不再加空工具名单。
- ZCode CLI 群聊直接使用其原生 prompt/JSON 通道，复用原生 launcher，避开 Desktop Account Host 对 CLI 登录凭据的错误转换；Default 仍使用已有 build 权限，只有原有 Yolo 配置才传 yolo。CLI 需要的交互权限没有被改成自动批准。
- 移除新增的群聊限制设置入口及旧规则迁移提示，保留正常角色编辑、草稿、提及、群聊与任务执行视图。历史数据及旧请求恢复逻辑保留。

## 验证

- 实际生产执行链：Codex、Claude、Pi、ZCode CLI 均用各自原生工具读出未写进提示词的随机文件内容；成员保留模型验收角色文字，权限 Profile 为 default，未修改测试文件。失败尝试证据保留，最终成功索引在 `.scratch/codex-group-research-20261117/native-implementation.md`。
- ZCode CLI 曾在 Desktop Account Host 路径报安全存储错误；直接调用原生 CLI 能回复，因此改用原生 CLI 认证流程，最终随机读取通过。
- Node：497 通过、1 跳过；conversation：80 通过、3 忽略；ACP：84 通过、13 忽略；bridge：7 通过；read_only：6 通过、1 忽略；ZCode CLI JSON 正文校验：1 通过。过滤集合有重叠，不合计为独立总数。
- 浏览器正式组件隔离 IPC：截图所示四成员全部可选/全选并发送，显式停用仍有效；角色编辑、原群聊回归、聊天/执行视图、输入区域拖拽回归通过。
- Windows Release 构建通过。旧 EXE 正在运行，使用独立 target 复用编译缓存，新包复制到 `output/CrewVela-native-group-20261117-151227/CrewVela.exe`，未停止或覆盖旧程序。
- 其它 Runtime 的真实模型调用未逐一验证；未穷举每个 CLI 的所有工具/写入操作/套餐能力；未做 macOS/Linux 或新 EXE 原生窗口交互验收。原生能力可用性仍由 CLI 安装、账号、服务和自身配置决定。

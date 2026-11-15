# 具体 Agent 多选与全选验证（2026-11-15）

## 实现

协作目标弹层和输入框 `@` 候选都只展示当前项目可用的具体成员，移除 Runtime 类型群组、`@All` 及单独终端会话候选入口。保留原有快捷指令操作。旧句柄的预留规则和底层通信兼容逻辑保留，避免历史草稿的成员句柄变化。

- 点击成员连续勾选/取消，列表保持打开；点击“完成”回到输入框。
- “全选”展开为每位成员的明确 token 和 ID，已有选择不重复；“取消全选”移除对应 token，保留指令正文。
- `@` 候选保留键盘上下选择和 Enter 插入；候选中的全选清除当前未完成的 `@` 查询再插入所有成员。
- 已禁用成员及已禁用 Runtime 的成员不进入选择范围。全选在点击时固定为具体成员，不是动态广播群组。
- 实际点击发现候选顶部被上下文栏拦截，已局部提高候选菜单层级，并以正常点击验证，无强制点击。

## 验证结果

以下验证使用实际 React 组件在 Chrome 中运行、模拟 Tauri IPC，不代表本轮再次完成了模型真实互聊：

1. `tests/explicitAgentSelection.ui.mjs`：连续多选、取消单项、全选去重、取消全选保留正文、完成后焦点、`@` 全选、键盘选择、禁用过滤、跨项目全选均通过。两个同为 Codex 的成员中只选审查员时，发送参数只有审查员 ID；全选时恰好三个可用成员 ID，`targetRuntimeKinds` 和 `targetSessionIds` 均为空，不自动按类型选人。切到项目 B 后的发送只含 B 的成员 ID。
2. `tests/retainedControlAsync.ui.mjs`：快捷指令、防重复提交、提示词优化、计划确认与失败提示、项目隔离、审批和交接回归通过。
3. `tests/multiAgentInstances.ui.mjs`：多面板共享自动模式、仅活动面板审批回归通过。
4. `tests/collaborationTaskFirst.ui.mjs`：任务详情、筛选、响应式几何和升级失败展示回归通过。
5. 相关 Node 单元测试 **30 通过、0 失败**，覆盖 mention token、候选菜单、明确身份路由、草稿、协调消息、统一路由和协作自动化。
6. TypeScript/Vite 前端构建通过。未修改 Rust 业务代码，未重跑 Rust 全量测试；本轮不将上次的互聊及 Rust 验证冒充新的测试结果。

初轮界面脚本中，回执按完整文本定位无法匹配富文本节点，调整为消息容器正文匹配；测试夹具补齐发送后的持久化消息快照。另发现并实际修复上述上下文层级拦截。跨项目保留草稿时成员已为全选状态，脚本先取消再重新全选，明确核对新项目 ID。

日志：`.scratch/agent-selection/`。实际截图：

![具体成员多选](../output/playwright/explicit-agent-selection-all.png)

![具体成员候选](../output/playwright/explicit-agent-mention.png)

## 交付

按约定使用 `npm run tauri -- build --no-bundle` 生成 Windows EXE，构建成功（保留已有 50 条 Rust 编译警告）。生产入口引用资源存在，新成员选择及取消全选逻辑已进入生产资源，旧群组入口构造及文案不在产物脚本中。交付 EXE `--help` 启动检查通过，隐私扫描 findings 为空。本轮未启动新版原生 GUI 或重跑模型互聊，以上界面验证均为真实浏览器加模拟 IPC。

- 文件：`output/CrewVela-agent-selection-20261115/CrewVela.exe`。
- 大小：46,385,152 字节。
- SHA256：`532FBF85195E20723047CD920F7A498F11B09BB7F0FB2D6108E128C81C75E373`。

交付目录包含 LICENSE 和 THIRD_PARTY_NOTICES.md。保留当前用户正在使用的窗口，不关闭或替换其进程；当前已有窗口不会因打包自动升级，请从新路径启动测试。ZCodeCLI 额度相关验证继续按用户要求暂不处理。

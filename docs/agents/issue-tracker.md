# 本地 Markdown 任务管理

所有路径均相对于仓库根目录。

## 文件约定

- 每个功能使用 `.scratch/<feature-slug>/`。
- 需求规格存放在该目录的 `spec.md`。
- 实施任务分别存放在 `issues/<NN>-<slug>.md`，从 01 编号。
- 每个任务独立成文件，不合并成一个任务清单文件。
- 分诊状态写在任务顶部的 `Status:` 行，取值见 `triage-labels.md`。
- 评论和讨论追加到文件末尾的 `## Comments`。
- 技能要求“发布到任务管理器”时，创建对应本地文件。
- 技能要求“读取任务”时，读取用户指定的任务文件。
- 按需创建目录和文件。

## Wayfinder 决策探索

仅在使用 wayfinder 时应用：

- 决策地图：`.scratch/<effort>/map.md`，记录笔记、已有决策和待解问题。
- 子任务：`.scratch/<effort>/issues/<NN>-<slug>.md`。
- `Type:` 取值为 research、prototype、grilling 或 task。
- `Status:` 使用 claimed 或 resolved，未认领任务保持 open。
- `Blocked by:` 列出依赖的任务编号；全部依赖 resolved 后才能认领。
- 优先认领编号最小、未认领且依赖已解决的任务，执行前保存 claimed。
- 解决后追加 `## Answer`，更新为 resolved，
  并在 map.md 中记录结论摘要与任务链接。

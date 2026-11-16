Status: ready-for-human

# 去掉讨论输入时的内层焦点描边

## 验收

- 聚焦/输入时 textarea 无内层框，外层容器及文字光标清晰。
- 按钮键盘焦点、@ 选择、Shift+Enter 换行、Enter 发送正常。
- 深浅色及窄视口显示检查通过。

## Comments

- 修复前实际组件复现 outline-style=solid，回归断言按预期失败。
- 范围见 `.scratch/conversation-input-focus-20261116/spec.md`。
- 已限定到讨论输入编辑区取消内层 outline；外层边线轻微提亮表示焦点，按钮等其他控件的焦点描边保留。
- 实际组件深浅色 × 1015/718px 显示与键盘输入检查通过，既有讨论 UI 回归及前端构建通过；未打包 EXE。

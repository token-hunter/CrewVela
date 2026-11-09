---
name: computer-control
description: 通过 computer-control MCP 驱动本机 Windows 应用的图形界面：读取无障碍树、点击、输入、滚动、拖拽与窗口截图。需要操作没有 CLI/API 的桌面软件的可见窗口时使用；能用命令行、文件、HTTP 或现有 CLI 完成的，优先用那些方式。
---

# 电脑控制（computer-control）

你可以通过 MCP 服务器 `computer-control` 驱动用户本机 Windows 应用的图形界面。工具基于 Windows UI Automation（无障碍树）+ 合成输入，移植自 orca（MIT）。

## 优先级判断（每次都先想）

1. **能不用就不用**：命令行、文件读写、git、HTTP、现有 CLI 能完成的，绝不用电脑控制。
2. **必须用 GUI 时，元素优先于坐标**：所有工具都接受 `elementIndex`（快照树行首的索引），它按无障碍语义精确定位，不受窗口移动/缩放影响。
3. **纯坐标是最后手段**：点击/滚动/拖拽只有坐标时**必须携带最近一次 `computer_get_state` 返回的 `snapshotId`**，过期会被拒绝——这是防止拿旧图的坐标点错位置的保护。

## 标准工作流

```
computer_list_apps          找到目标应用（pid/process/title）
computer_get_state          观察窗口：无障碍树（行首=元素索引）+ 截图 + snapshotId
computer_click/…            按元素索引操作；响应自带最新快照，无需每次重新观察
```

- 树行格式：`索引 角色 标题, Value: 值, Secondary Actions: Invoke/Toggle/Select`。
- 文本框优先 `computer_set_value`（ValuePattern 直写，不经键盘）；其次 `computer_click` 元素后 `computer_type_text`。
- 大段文本用 `computer_paste_text`（剪贴板粘贴，会自动保存并恢复用户剪贴板）。
- `computer_perform_action` 执行树行标注的具名动作（invoke/select/toggle）。
- 键盘类动作（type/press/hotkey/paste）要求目标窗口持有焦点；默认会 `restoreWindow` 置前，最小化窗口也能恢复。

## 安全与礼貌

- **这是用户的真实电脑**：鼠标键盘会被真实移动，密码管理器等敏感应用被硬性禁止；涉及密码/验证码的输入，把焦点移到输入框后**请用户手动完成**。
- **截图含真实屏幕内容**：`computer_get_state` 默认带窗口截图（会作为图片发给模型提供方），确认界面布局时才需要；纯结构信息看树就够，动作响应默认不带截图以省 token。
- 合成输入的动作结果是"未验证"的（工具会标注）：关键操作后看一眼动作响应里的新树确认效果。
- 一次一个动作，观察后行动；连续失败两次就停下来向用户说明。

## 排错

- `appNotFound(...)`：用 `computer_list_apps` 重新找准确的进程名或 pid。
- `stale element frame` / 元素索引找不到：界面变了，重新 `computer_get_state`。
- `window_not_focused`：目标窗口没焦点，重试时带 `restoreWindow: true`。
- `snapshotId 已过期`：重新观察后再用坐标。
- 工具列表里没有 computer_* 工具：本会话未启用电脑控制，提示用户到 CrewVela 设置 → 电脑控制 打开。

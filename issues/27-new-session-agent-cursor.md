Status: ready-for-human

# 修正选择 Agent 卡片的鼠标指针

## 验收

- 卡片正常悬停使用常规点击手形；排序把手和拖拽中使用标准移动指针。
- 保留卡片单击、键盘选择、拖拽排序和排序持久化。
- 实际运行界面验证，并按指定命令重新打包；说明原生指针可见性验证的限制。

## Comments

- 现有 CSS 未隐藏指针，整张卡片原先使用 grab，拖拽中为 grabbing；不能仅凭截图确认 Windows 实际指针形状。
- 已仅修改 4 处 cursor：卡片 pointer，把手及拖拽中 move；原有点击、键盘和排序逻辑未改动。
- CUA 实际运行预览：13 种 Agent 卡片及图标计算样式为 pointer，把手为 move；Enter 和鼠标单击均进入 Provider 选择。
- 预览中首次通信提示遮住 Claude 卡片，关闭该预览提示后点击成功；此独立遮挡未纳入当前指针任务修改。
- 顺序持久化、拖放插入、顺序规范化、Provider 图标等 6 项现有回归通过。CSS 差异检查通过。
- 原生 Windows 指针形状和真实鼠标拖拽尚未重新验收；浏览器样式检查不等于系统鼠标图像验证。
- 指定命令 `npm run tauri -- build --no-bundle` 打包成功，默认产物 `src-tauri/target/release/CrewVela.exe`。保留已有 Rust warnings 和大 chunk 提示；未提交或推送。

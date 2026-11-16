Status: ready-for-human

# 按指定原型完善自动化界面

用户明确指定exec-1e536184-f55c-41fc-808c-50e6ffcc1ba5.png，直接依该图实施，无需重做原型。
规格：.scratch/automation-design-20261116/spec.md。

## 验收

- 主标题/日期/下一计划、双列任务卡、图标、计划开关、语义状态与执行记录贴近原型，并适配深浅色和窄窗口。
- 保留现有计划/执行状态分离、持久请求幂等、重试、精确停止与人工核对；不改后端，不伪造序号或成功状态。
- 回归自动化/创建表单、设置框架、已完成Skills卡片及新增菜单Escape行为。
- 仅在feature/major-redesign上工作，保留89未提交改动，不改main或备份，不自动提交。

## Comments

- 2026-11-16用户实机反馈：主按钮/开关仍为白色，长未知结果说明撑高卡片。重新打开任务，以用户提供的design-dark-dialog.png为本轮精确参考。定位正式应用data-appearance-accent=clay使accent-fill变为中性白；旧fixture仅设置data-theme，未覆盖真实主题路径。补齐真实主题、长记录与125%缩放回归后重新交付，保留已确认的未知状态和所有底层行为。
- 正式主题一致性修复完成：仅自动化列表局部固定图2陶土色，长说明单行省略并保留完整历史详情和title，计划状态补完整title。三条未知记录卡片由236.75px恢复221.46875px；不伪造成功、运行或会话入口。
- automationAppearance新增14组深浅色/显式clay-none-purple/长记录/125%缩放/窄窗口回归，先失败后通过；全局clay中性填充、创建表单配色及业务状态不变。原自动化、协作异步、Skills卡片、28组设置框架/336次菜单访问通过；Node471通过、1既有跳过，前端构建通过。新design-qa.md为passed，参考与同状态截图并排核对。
- 修正版EXE：output/CrewVela-automation-parity-20261116-092839/CrewVela.exe，构建2026-11-16 09:28:01+08:00，SHA-256 6666E5EE035B600C4ECF26FB9A89E048F3F08516AE4905AEAE45B674627D2D4E。Windows发布构建通过，66条原有Rust警告未屏蔽。原生隔离启动通过，测试进程关闭。
- 原生WebView颜色读取尝试未完成：test_profile.rs有意移除WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS，隔离模式无CDP端点；没有修改隔离机制。浏览器正式主题属性下颜色验证通过，但原生配色仍待用户实机验收。无真实Agent任务调用，main/备份/HEAD保持原值，未提交。

- 2026-11-16：用户路径多余的前导斜杠已按Windows路径规范修正，仅用于读取同一张已指定图片。图片已实际打开，1487×1058。
- 2026-11-16：已实现指定图的标题/日期/下一计划、双列卡片、状态高亮、计划开关及下方执行记录。保留原有动作与确认，使用真实执行ID而非虚构序号；更多菜单保留删除、核对与记录入口。
- 修复390px主按钮裁剪、选中开关圆点、已显示历史时更多入口误收起等问题。源码与实际截图共同比较通过，design-qa.md为passed；上一份Skills核验已另存。
- automationDesign/automationExecution/collaborationAsync/skillsCardLibrary/modalOverResizers浏览器检查通过；设置框架28组组合、336次菜单访问通过。Node471通过、1项既有跳过；前端构建通过。
- Windows命令npm run tauri -- build --no-bundle，发布构建通过（保留66条既有Rust警告和Vite大分块提示）。EXE构建时间2026-11-16 08:55:31+08:00，output/CrewVela-automation-ui-20261116-085555/CrewVela.exe，SHA-256 F7BB7B28732C7FE85E8B2744ACE5FADCF8E687C6807A8AD34502E3AF0A69B171。
- 新EXE独立test-data-dir原生启动通过，窗口响应正常，测试进程已关闭。仅验证原生启动，不等于真实自动化或模型调用验收；本轮未改Rust后端、未重跑Rust测试、未触发真实计划或Agent。
- 保留89的Skills未提交改动。main/备份仍为239febf0e55e10bb8733663d4ac840e2f4eeea25；HEAD未变，未提交/推送。

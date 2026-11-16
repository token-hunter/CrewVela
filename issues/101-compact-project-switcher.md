Status: ready-for-human

# 实施项目选择器第1版轻量文字入口

用户通过40466b0e-a4c4-4fcc-8de0-9efaa2b55f2a明确选择项目选择器第1版，不是关于Logo或黑色主题原型。
视觉目标：exec-40739a3e-3d9b-485b-99ce-ffac30e8c8c8.png。

## 验收

- 项目名使用15px/500文字、28px控件高度，无边框/底色胶囊，小箭头12px；所有宽度均不恢复原20–22px粗标题。
- 内容自适应、上限320px，窄窗为右侧创建/管理按钮保留空间；长项目名省略显示，悬停保留完整名称和根目录。
- 保留原生选择框、实际项目切换、任务直达协作中心、成员选择及管理/创建入口、强调色焦点；不改任务与讨论路由。
- 真实组件深浅色、宽窄窗口、长名称与交互验证，依据选定原型和实际截图做视觉比对。
- 保留未提交的任务100；关于Logo和黑色主题方案仍待独立选择，不提交、不打包EXE。

## Comments

- 已实现指定原型：15px/500、28px高度、透明无边框、12px小箭头，上限320px；移除宽/窄断点上的18–22px覆盖。长名称悬停同时显示完整项目名与目录。
- 八组深浅/宽窄实际组件测试验证字号、字重、尺寸、透明/边框、箭头、title及原生Home/End项目切换；原成员菜单/创建/管理/工作台和任务跳转回归保留。
- collaborationHeader.ui、collaborationTaskNavigation.ui、collaborationConversation.ui、前端构建通过；完整测试476通过、1项既有POSIX用例跳过。合并视觉对比已查看，design-qa为passed。
- 应用内浏览器的browser-client无法加载，按不可用回退规则用隔离Chrome验证。详情及125%缩放证据见.scratch/project-switcher-implementation-20261116/verification.md。
- 保留任务100及未选定的Logo/黑色主题原型。未真实调用Agent、未提交、未打包EXE，main和备份未动。

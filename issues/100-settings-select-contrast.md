Status: ready-for-human

# 修复归档项目筛选及设置原生下拉的白底浅字

用户截图codex-clipboard-827fe4b2-03a5-484b-b971-3212855881b1.png：深色设置的归档项目下拉展开后，未分组选项白底浅字。

## 验收

- 设置页原生select的option/optgroup具有显式主题背景/文字色，不依赖系统白底；深浅色及现有主题可读。
- 复用设置共用样式，不改变原生下拉、页面布局、归档筛选/搜索或恢复/删除逻辑；已修复的Skills仍正常。
- 实际组件/设置界面验证颜色对比度、所有项目/具体项目/未分组及搜索组合，回归设置与Skills控件。
- 保留未选定的关于弹窗原型；不自动提交、不打包EXE，不改main或备份。

## Comments

- 当前工作区干净，分支feature/major-redesign，HEAD01becd1。Skills已具有局部option主题背景，但归档页及设置共用样式仍缺少显式选项背景；根color-scheme已有深浅主题支持。
- 修复完成：settings-native共用样式为select中的option/optgroup提供主题背景与文字色，未改ArchivedSessionsPanel或归档处理逻辑。
- 已先复现dark归档“所有项目”option背景为rgba(0,0,0,0)导致回归失败（before-ui.log）；修复后六种主题及深浅色窄屏共八组通过，每个选项文字对比度≥4.5，所有项目/具体项目/未分组及搜索组合正常，常规和浏览器设置下拉同步验证。
- Skills作用域回归、实际App六主题切换与持久化、前端构建通过；完整测试476通过、1项既有POSIX用例跳过，0失败。详见.scratch/settings-select-contrast-20261116/verification.md。
- 截图为真实组件的内联原生列表，已打开检查深浅色显示；Windows WebView2系统弹出菜单仍需新EXE实机确认。未重新打包EXE、未提交，main及备份未动，关于弹窗原型仍待选择。

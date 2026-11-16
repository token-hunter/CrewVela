Status: ready-for-human

# 修复Skills下拉选项白底白字

用户截图：作用域下拉的“全局”和“任务 / Agent”在Windows深色界面难以阅读。

## 验收

- Skills原生下拉选项明确使用主题背景和文字色，深浅色可读，普通文字对比度至少4.5。
- 保留全局/项目/任务作用域切换及现有分发逻辑，不改变页面布局或其他设置页。
- 保留已有未提交的89/90改动，仅在feature/major-redesign操作，不改main或备份，不提交。
- 回归实际React界面与Skills行为，按默认Windows命令重新打包。

## Comments

- 2026-11-16：skillsSelectContrast.ui.mjs已实际失败：dark 全局 option背景rgba(0,0,0,0)，文字rgb(242,241,238)。控件和根color-scheme均为dark，浅色同样没有option显式背景。没有发现色彩模式被强制浅色；原生弹出菜单在Windows可回退白底，Chrome内联列表未复现系统菜单白底，因此两类证据明确区分。
- 已在Skills页面为select option补主题背景与文字色，深色背景rgb(31,31,30)，消除透明选项表面对系统白底的依赖；浅色沿用主题深字白底。未改变布局、作用域或分发逻辑。
- 新回归深浅色选项对比度≥4.5、完整三种作用域切换、无页面异常通过。skillsRuntimeFirst、skillsAsync、skillsCardLibrary回归及前端构建通过；本轮没有重跑Node或Rust全量测试，也没有触发真实部署或Agent任务。
- EXE构建2026-11-16 09:41:38+08:00：output/CrewVela-skills-select-fix-20261116-094231/CrewVela.exe，SHA-256 499411DA144FADFAFB3A55A3C9AD73D22D0B874EF5BC508F6EBEA79655E0EF84。保留66条既有Rust警告和Vite大分块提示。全新隔离数据目录原生响应窗口启动通过，测试进程已关闭。Windows原生弹出菜单外观仍待用户实机确认。
- 之前89/90改动保留，main/备份仍为239febf0e55e10bb8733663d4ac840e2f4eeea25，HEAD仍为a529e273a7db492c18bfbf016c127fc5f1979734，未提交或推送。

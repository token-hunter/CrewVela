# 石墨黑替换暮紫
Status: ready-for-agent

## 需求
按用户选定的第 2 款原型替换暮紫。保留其他五个主题和三深三浅排列。

## 验收
- 主页面、设置和预览卡片使用石墨黑，旧主题配置可直接加载。
- 主题切换、强调色（含紫色）、背景、透明度和持久化正常。
- 实际页面显示和操作通过，Windows EXE 包含工作区补号修复。

## Comments
- 实现与交互核验完成，复用现有主题令牌与内部标识；未改动紫色强调色。
- TypeScript、8 项相关检查与 IAB 实际页面验证通过，见 docs/graphite-theme-verification-20261115.md。Windows 构建及交付校验完成，新包 output/CrewVela-graphite-20261115/CrewVela.exe。

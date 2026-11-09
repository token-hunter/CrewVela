Status: ready-for-human

# 检查源码及打包产物中的旧品牌

用户要求后续使用 `npm run tauri -- build --no-bundle`，并搜索项目中 cc 相关旧名称，包括 EXE 等产物。

## 结果

- 已将指定命令记入 AGENTS.md，未进行新构建。
- 当前源码基线87个文件332处旧名；文件/目录176个旧名路径；公开源码副本201个文本文件命中。
- 清理前曾发现旧版 EXE/MSI 和发布者元数据残留；此处记录扫描基线，清理后的当前状态见品牌迁移任务及报告。
- 完整分类、位置与扫描边界见 docs/legacy-brand-audit.md；1个历史临时目录读取失败已披露。
- 本次不批量替换运行时标识、不删除旧包；保留之前未提交改动。后续清理须覆盖配置/凭据/Skill/应用身份的兼容迁移。

- 后续品牌迁移已完成并重新构建；以上“未构建/不替换”是本任务早期扫描记录，当前结果以 issues/23-crewvela-brand-migration.md 和 docs/legacy-brand-audit.md 为准。

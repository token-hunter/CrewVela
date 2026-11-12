Status: ready-for-human

# 为 GitHub 发布与文档添加 Beta 标识

## Scope

- Release 标题显示 CrewVela v0.1.0 Beta，并标记 Pre-release。
- 中英文 README 徽章、当前版本提示与下载说明同步。
- 本地发布说明同步 GitHub 已有构建信息，保留验收限制。
- 保留 v0.1.0 标签、安装包内部版本、附件与链接，仓库保持私有。

## Validation

- GitHub Markdown API 渲染中英文 README、下载说明、发布说明、更新记录通过。
- 徽章 SVG 显示 0.1.0-beta，本地文档链接检查通过，文档密钥扫描零命中。
- GitHub Release 已显示 CrewVela v0.1.0 Beta，Pre-release 为 true，非草稿。
- 发布说明核对一致；所有附件 ID、哈希、大小与文件名保持不变。
- 应用内部版本、Git 标签与下载链接保留；本任务不重新编译安装包。
- 发布后核对远端 README、main 提交与仓库私有状态。

# 双平台发行包与命名记录

> 以下为旧仓库的历史构建记录，旧附件只保留在独立私有备份中。当前新仓库附件状态以 [v0.1.0 说明](releases/v0.1.0.md)为准，不复用下方旧安装包的哈希。

日期：2026-11-09。只操作开源副本；保留此前开源准备文档和用户原 DMG。构建验证阶段未提交或发布；用户后续授权提交文档和安装包配置，不包含 EXE/DMG，不推送或发布。

## Mac DMG

用户提供的旧显示名称 ARM64 DMG 复制为 `output/releases/v0.1.0/CrewVela-0.1.0-macos-arm64.dmg`。只改变外部文件名，未改镜像或 app 内容。

原包与复制件 SHA-256 均为 `63c15b24f6e9c4964d67f836583a9d190d236d3e63f76d9a1e2700f740c931da`，与此前 v2 Mac 报告一致。Windows 已检查 UDIF 的 koly 尾部标记及头长度；这不等于 hdiutil 完整映像检查或签名、公证验收。此前报告为 ad-hoc 签名、未公证；安装副本正常退出与真实 Agent 等边界继续保留。

## Windows 打包方式

保留日常构建命令，分两步执行：

```sh
npm run tauri -- build --no-bundle
npm run bundle:windows
```

第二步调用 `tauri bundle --bundles nsis`，使用已经构建的应用生成真正的安装向导，不是把应用 EXE 改为 setup 名称。Windows 专属 `tauri.windows.conf.json` 只在 Windows 合并，保留主配置里的应用身份和标题；新增当前用户安装、中英文语言选择、CrewVela 开始菜单目录、MIT 展示及 5 项明确列出的许可资源。

缺少 WebView2 时使用下载引导，需联网。该安装包不会替用户安装全部 Agent CLI、Node 和 ACP 适配器，不等于此前“干净机器完整工作流”待办已完成。许可材料也不替代未完成的素材来源核对。

## 实际验证

- Node 全量：425 通过、1 项既有跳过，0 失败。
- 发布资源检查：1/1，通过，保留生产协议、内嵌入口和 JS/CSS 的有效断言。
- `npm run tauri -- build --no-bundle` 成功，包含 TypeScript/Vite 构建；保留 48 条既有普通库 Rust 警告和前端大 chunk 提示。
- `npm run bundle:windows` 成功，调用 makensis 生成 NSIS 安装向导。当时输出为旧显示名称的 NSIS 安装包，复制为 `output/releases/v0.1.0/CrewVela-0.1.0-windows-x64-setup.exe`；复制前后散列相同。
- 安装包为有效 PE，含 NullsoftInst 标记；生成的 NSIS 脚本为 x64 目标、当前用户模式，明确只打包主 EXE 和 5 个许可文件，并生成标准卸载程序。没有把本地历史、账号、node_modules 或测试数据目录作为资源。
- 实际运行：语言选择默认简体中文，进入中文欢迎页和 MIT 许可页，未点击接受条款或安装，按取消退出；没有执行安装/卸载或启动真实 Agent。坐标交互有一次未推进，改用向导原生快捷键后成功核对许可内容；未把工具交互问题记为功能通过。
- Windows Authenticode 状态为 `NotSigned`；没有跳过或关闭系统安全保护。未验证从网络下载后的 SmartScreen 行为。
- 原始日志和向导截图保留在忽略目录 `.scratch/release-installers/`。

安装包 SHA-256：`1e4c8c6562082b1e9bdbaf5be17beec4c14e2396851029f27c30f85478753872`，大小 19,023,514 字节。Mac DMG 为 23,837,733 字节。发行目录的 `SHA256SUMS.txt` 覆盖两个最终文件；散列核对不能替代签名。

Tauri bundle 会向主 EXE 写入 bundle 类型信息，因此 bundle 后的主 EXE 散列可能变化；不能混用此前便携候选目录的 EXE 校验值。本轮交付对象是新的 setup EXE 和原字节不变的 DMG，先前便携候选目录保留，不覆盖其旧校验清单。

安装与卸载不在当前用户的正式环境执行；不会关闭真实应用实例或删除历史。构建工具、向导观察和散列检查不能代替干净机器安装、运行、升级和卸载验收。

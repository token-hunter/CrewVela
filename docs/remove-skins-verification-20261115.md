# 皮肤功能移除与验证（2026-11-15）

按用户明确要求直接移除皮肤，保留普通外观与桌宠。删除皮肤入口、库管理、JSON/CCSKIN/MPKG 导入解析、动态媒体层、场景动画/环境音及 Rust 专属命令和注册。删除专属配置模块、样式及仅针对已移除功能的测试；原皮肤/桌宠混合回归中的桌宠部分迁为独立 `petAsync.ui.mjs`，保留全部有效桌宠断言。

皮肤样式表混放的外观透明度与基础界面样式原样迁到 `appearance-transparency.css`，保持加载位置。普通主题、背景、强调色、跟随系统、透明度和独立桌宠保留。文件预览仍使用本地 asset protocol，所以只去掉 skins 目录静态授权，保留共享协议及依赖。未发现可仅随皮肤删除的独占第三方依赖。

停止读取、迁移和应用旧皮肤配置。旧 localStorage 值及用户导入的媒体文件不自动清理，正式用户目录未被删除。历史研究文档已标明功能退役，不当作当前功能说明。

## 验证

- Node 全量：493 项中 492 通过、1 跳过、0 失败。
- Rust 库全量：1130 通过、49 忽略、0 失败。与此前相比移除了已删除动态皮肤模块的 8 项专属测试，保留功能的断言未放宽。
- 新增移除契约检查：专属模块/命令缺失、渲染入口缺失、共享透明度和桌宠/文件预览保留。
- `skinRemoval.ui.mjs`：预置历史皮肤与启用音频配置，不产生媒体请求、视频/音频元素或皮肤标记；真实外观选项可修改并持久化；透明度开关和原有 0%–90% 滑块范围通过；陪伴桌宠入口可用。
- `petAsync.ui.mjs`：导入去重、并发设置/删除保留、正常选择新宠物、卸载后忽略完成事件通过。
- `appearanceAccent.ui.mjs`：6 主题、8 强调色、旧色迁移、鼠标/键盘选择、重载和恢复默认通过。
- `fileExplorerEditing.ui.mjs`、`modelAssignments.ui.mjs` 通过；`settingsNavigationLayout.ui.mjs` 在 3 种尺寸和深浅色的 6 组设置布局中通过。
- TypeScript/Vite 构建通过，生产代码扫描未发现皮肤模块、渲染器或 IPC 命令引用。界面截图已实际查看。

首轮新界面脚本尝试点击不可见的开关 input，被可见标签拦截；改为按真实用户点击可见开关旋钮。随后 End 键的期望值按原有滑块 max=90 修正，未改产品范围或跳过检查。保留初轮日志及最终成功日志。

本轮界面验证采用实际 React 组件与隔离 IPC，未改正式配置、未运行真实 Agent、未声称 macOS 原生通过。日志与改动前备份在 `.scratch/remove-skins/`。

![移除皮肤后的外观设置（验证数据）](../output/playwright/appearance-without-skins.png)

Windows 构建使用 `npm run tauri -- build --no-bundle`；最终 EXE 信息随交付目录 README 提供。

- 文件：`output/CrewVela-no-skins-20261115/CrewVela.exe`。
- 大小：46,261,760 字节。
- SHA256：`D5F1C8E1CBB9DE300A18D24C8F80E2BCF3EB0FD682CDAD88CBBC0B80E0429EFB`。
- 构建成功，既有 50 条 Rust 警告保留。生产 JS/CSS 不含已移除皮肤运行逻辑，透明度样式仍在；EXE 复制哈希、CLI 启动及隐私扫描通过。CLI 检查不代表原生 GUI 验收。

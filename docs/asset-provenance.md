# 发行素材来源记录

本文件记录素材来源与核对范围。项目根目录的 MIT 许可证不能自动授予第三方商标或图像的使用权。

## 2026-11-09 中性图形替换

按维护者选择，菜单、标签、Provider、CLI 状态区及原生聊天欢迎页改用通用图形。Agent 名称、运行时标识、接入方式与通信逻辑不变；图形不代表厂商授权、合作或背书。

| 当前素材 | 来源与处理 |
| --- | --- |
| `src/assets/neutral-agents/*.svg` | 从锁文件中的 `@phosphor-icons/react` 2.1.10 直接渲染，保留上游轮廓；仅配置颜色和用途。MIT 完整原文位于 `THIRD_PARTY_NOTICES/phosphor-icons-LICENSE` |
| 中性图形生成器 | `scripts/generate-neutral-agent-icons.mjs`；在 `npm ci` 后执行 `node scripts/generate-neutral-agent-icons.mjs` 可复现 |
| `docs/media/crewvela-collaboration-banner.png` | Image Gen 新生成的宣传插画；沿用 CrewVela 橙色球形形象与舞台布局，其余为抽象通用角色，不按厂商角色或 Logo 设计。生成图不是产品截图，也不能作为第三方授权证明 |
| 新版原生桌面截图 | 来自新编译的 Windows 应用与专用隔离目录；具体拍摄范围见 `docs/media/README.md` |
| README 多 CLI 分屏及原生聊天截图 | 用户再次提供并明确要求用于界面预览，原样恢复；为含旧图标的历史版本实拍，不代表当前中性图形或第三方品牌授权 |
| `crewvela-overview.svg` / `social-preview.jpg` | 项目文字和几何示意图及其转出图片，不是产品截图 |

已从当前应用源码资源移出旧 Agent 品牌图形、欢迎页图片与 Claude 状态帧；旧文档图片中，多 CLI 分屏与原生聊天图按用户后续指定恢复展示，其余仍保留在本机已忽略的 `.scratch/asset-rights/retired/`。此操作不改写 Git 历史，也没有更新 GitHub 既有 Release 的安装包；这些旧版本仍可能包含旧图形。

## 其他素材与依赖

- CrewVela 代码许可见根目录 `LICENSE`。
- 配额适配器的实际来源说明及 CC Switch 许可证保留；portable-pty 的许可证保留。详见根目录 `THIRD_PARTY_NOTICES.md`。
- CrewVela 橙色品牌形象、通知形象、其他可选宠物和旧版素材的逐文件创作记录尚不完整。本次替换针对 Agent 品牌图形，不将这些其他素材标为已经完成权利核验。
- npm / Cargo 依赖还适用各自的许可证；安全扫描与测试通过不等于全部许可已核验。

本记录不作“全部素材没有版权问题”的承诺。以后公开新版二进制时，需使用本次改动后的构建产物，并随分发保留适用许可证。

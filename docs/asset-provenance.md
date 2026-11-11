# 发行素材来源记录

本文件记录当前素材来源与处理范围，项目根目录许可不替代第三方素材适用的许可。

## 当前项目视觉

按维护者最新要求，恢复项目原版的会话图形、聊天欢迎页角色与 CLI 状态动画。维护者指出这些资源中包含其自行设计的图形；中性替换不再整类覆盖项目既有设计。

| 当前素材 | 来源与处理 |
| --- | --- |
| `app-icon.png` / `src-tauri/icons/` | 对照中性替换前版本逐文件核验，保持原文件一致，没有重新生成软件主图标 |
| `src/assets/agent-*`、`claude-color.png` | 项目原有会话图形，按维护者要求恢复到中性替换前版本 |
| 欢迎页及 CLI 角色 | 恢复原版角色组件、图片与状态帧，保留暂停、减少动态效果和交互行为 |
| `src/assets/model-zhipu.svg`、`model-deepseek.svg`、`model-minimax.svg` | 来自 Lobe Icons；保留 LobeHub 署名及 MIT 完整原文，见 `THIRD_PARTY_NOTICES/lobe-icons-LICENSE` |
| `src/assets/neutral-agents/*.svg` | 保留的 Phosphor 通用辅助图形，由 `scripts/generate-neutral-agent-icons.mjs` 生成；许可见 `THIRD_PARTY_NOTICES/phosphor-icons-LICENSE` |
| `docs/media/crewvela-collaboration-banner.png` | 中文 README 使用的原宣传插画，保留 CrewVela 橙色形象与舞台布局 |
| `docs/media/crewvela-collaboration-banner.en.png` | 以上原图的英文文案本地化版本，用内置 Image Gen 编辑；用于英文 README，具体文案见 `docs/media/README.md` |
| `crewvela-overview.svg` / `social-preview.jpg` | 工作方式示意及其分享图 |

此前中性替换涉及的视觉部分，已按本次要求恢复。素材恢复不修改 Git 历史，也不会自动更新已发布安装包。

## 其他随附许可

- CrewVela 代码许可见根目录 `LICENSE`。
- 配额适配器、CC Switch 与 portable-pty 的已有来源说明及许可继续保留。
- 其他 npm / Cargo 依赖适用各自许可。相关入口见 `THIRD_PARTY_NOTICES.md`。

# 第三方声明

## Phosphor Icons

界面中的通用图标及 `src/assets/neutral-agents/*.svg` 来自 [Phosphor Icons](https://github.com/phosphor-icons/react)，当前锁定版本为 `@phosphor-icons/react` 2.1.10。中性会话图标通过 `node scripts/generate-neutral-agent-icons.mjs` 生成；图标轮廓来自该库，颜色与运行时的对应关系由 CrewVela 设置，不是厂商品牌标识。

Copyright (c) 2020 Phosphor Icons

采用 MIT 许可证，完整原文见 [phosphor-icons-LICENSE](THIRD_PARTY_NOTICES/phosphor-icons-LICENSE)。

## 其他随附许可

- 群聊 Markdown 显示使用 [react-markdown](https://github.com/remarkjs/react-markdown) 与 [remark-gfm](https://github.com/remarkjs/remark-gfm)，及其解析依赖；原始许可与声明见 [markdown-LICENSES.txt](THIRD_PARTY_NOTICES/markdown-LICENSES.txt)。

- 模型 SVG 图标（智谱、DeepSeek、MiniMax）来自 [Lobe Icons](https://github.com/lobehub/lobe-icons)，Copyright (c) 2023 LobeHub，MIT 许可原文见 [lobe-icons-LICENSE](THIRD_PARTY_NOTICES/lobe-icons-LICENSE)。

- 配额适配器：[来源说明](THIRD_PARTY_NOTICES/provider-quota.md)及 [CC Switch 许可证](THIRD_PARTY_NOTICES/cc-switch-LICENSE)。
- portable-pty：[随源码保留的许可证](src-tauri/vendor/portable-pty/LICENSE.md)。
- 内置共享记忆使用 [ai-memory 3.5.1](https://github.com/akitaonrails/ai-memory/releases/tag/v3.5.1)，Copyright (c) 2026 Fabio Akita，MIT 许可证。官方安装包和其中的 LICENSE 随支持平台的发布构建内置。

依赖各自附带的许可证继续适用。上述文件也通过 Tauri 资源配置随新构建的安装包分发；直接分发未捆绑 EXE 时，应同时提供这些许可文件。

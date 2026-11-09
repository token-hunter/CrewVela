Status: ready-for-human

# 接入 OpenAI 官方候选与 Codex 原生安装

按 `.scratch/openai-plugin-market/spec.md` 实施，保留人工精选和既有安装数据。唯一代码写者负责插件前后端与回归，不提交或打包。

## Comments

2026-10-15：用户已授权执行分析建议；先核验官方固定提交及 Codex CLI 回执，按真实适配边界实施。

2026-10-15 实施结果：显式接入固定提交 `0722921d5542fc593105c27bd52630babd8b8c2a` 的 OpenAI 候选，不默认精选；支持 local/path 与 Codex/portable 元数据，连接器待授权仅展示。首批范围按用户最新要求限定 Remotion / Product Design。Codex 原生安装通过明确的 CrewVela 个人市场包装整包，保留根引用，按回执、来源、版本、缓存路径与指纹验证；未核验状态保留且不认领外部安装。其它 Agent 不拆分这些 Codex 专属包。

验证：前端构建通过；Rust plugin_market 回归 31 项通过；Node helper 8 项通过；原插件 UI、人工精选 UI 与官方候选 UI 契约均通过，包括 600px 窄屏。Rust cargo check 通过。首次执行候选 UI 检查时命令文件名写错，已用实际 `tests/pluginCuration.ui.mjs` 修正并通过；未跳过有效检查。

边界：真实 CLI 安装/卸载与 Remotion 1 秒 MP4 渲染由主智能体在隔离目录验证；Product Design user-context preflight 返回 missing，Browser/ImageGen/Sites 宿主工具集成尚未验证。安装记录核验不代表 Agent 自动调用或完整创作流程通过。未操作真实用户插件配置，未提交、未打包。

2026-10-15：主智能体已验收；追加 Windows 长路径修复，插件回归 32 项通过，真实官方市场长路径接入及重复接入验收 1 项通过。保持完整候选，不修改全局 Git 或系统设置。

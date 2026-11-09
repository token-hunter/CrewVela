# 开源介绍与传播准备

## 定位

首屏只讲一个可展示的价值：**让 Claude Code 和 Codex 在同一个桌面工作台里直接协作**。分屏、项目任务、共享记忆是支撑能力。避免一开头罗列几十个模块、宣称完全离线或全平台稳定。

GitHub About 中文草稿：

> 让 Claude Code、Codex 等编程 Agent 在同一桌面工作台里沟通、协作和交接。基于 Tauri 的开源多 Agent 工作台。

English description:

> An open-source desktop workspace for Claude Code, Codex, and other coding agents to communicate, collaborate, and hand off work.

建议 topics：`ai-agents`、`claude-code`、`codex`、`multi-agent`、`tauri`、`developer-tools`、`agent-workspace`。发布范围确认后再添加 `windows` / `macos` 等平台标签，不靠无关热门标签引流。

## 最值得拍的 30–45 秒演示

1. 0–5 秒：提出一个小任务，例如实现一个待办列表的筛选功能。
2. 5–15 秒：Claude Code 实现；Codex 在独立会话审查同一任务的结果。
3. 15–25 秒：Codex 发回一个具体问题，Claude Code 收到并修正。
4. 25–35 秒：运行验证，展示最终行为和双方消息。
5. 最后：下载入口、支持平台与版本号。

真实拍摄用全新演示仓库、合成会话和测试账号；不显示真实邮箱、账单、模型 Key 或私人路径。不把等待剪掉后宣称耗时；加速或剪辑需标明。没有完整跑通就不制作成功效果假录屏。

## 首发物料

- README 第一屏：一句定位、工作方式图、当前平台状态、获取方式。
- 三张最终产品图：双 Agent 工作区、通信实收与回复、保留来源历史的交接。
- 当前可先使用有明确标注的生产组件预览，但不能替代上述真实产品图。
- GitHub 社交预览：1280×640 PNG，小于 1 MB；素材不应含私人数据、虚构指标或未经授权 logo。
- 一篇“解决了什么实际工作流问题”的中英文短介绍，给出一个完整例子与安装步骤。

## 发布节奏

请真实开发者按 README 在干净环境试用并记录卡点。优先回答安装、模型配置、收不到消息和数据存储问题；每次修复发可追踪的小版本。适合在相关社区按规则分享真实演示、技术细节与局限，避免同文刷屏、买 Star、互 Star 或夸大验证范围。

Star 是滞后指标。首周更有用的观察是：读者是否能在几分钟内说出用途、下载后是否启动成功、是否完成首次双向通信、主要失败原因、问题响应是否清楚。下载量和新用户反馈比单独追求展示数字更能指导改进。

## 参考

[GitHub README](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)；[GitHub 社交预览尺寸](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/customizing-your-repositorys-social-media-preview)。建议是本项目的发布策略，不承诺 Star 数量。

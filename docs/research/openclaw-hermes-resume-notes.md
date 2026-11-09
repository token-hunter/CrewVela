# OpenClaw 与 Hermes Agent 简历资料边界

核对日期：2026-08-31。仅官方资料调研，不涉及产品代码修改。

## 官方资料

- [OpenClaw 定时任务](https://docs.openclaw.ai/cron)：Gateway 内置调度器，任务持久化，按时间唤醒 Agent，可将结果投递到聊天渠道或 Webhook。
- [OpenClaw Skills](https://docs.openclaw.ai/tools/skills)：以 SKILL.md 组织工具使用流程；支持工作区及共享 Skill 的加载与管理。
- [Hermes 功能概览](https://hermes-agent.nousresearch.com/docs/user-guide/features/overview)：提供持久记忆、可复用 Skill、定时任务和子任务委派能力。
- [Hermes 工具参考](https://hermes-agent.nousresearch.com/docs/reference/tools-reference/)：提供终端执行、文件操作及 Skill 管理等工具。

## 已确认用户实绩

- 用户确认 OpenClaw 已落地四项：每日简报推送、个人知识库（RAG）、链接/收藏管理、多平台多账号视频自动发布。
- 用户确认经常使用 Hermes Agent；主任务另已核实 CrewVela 的 Hermes Runtime 适配，含任务说明注入、会话恢复参数、模型选择及统一命令取消链路。

## 简历事实边界

- 官方功能用于说明方案适配性，不能直接当作用户亲自完成的功能。
- 未知项不写：具体发布平台、推送渠道、向量数据库、检索算法、并发规模、节约时间比例、客户数量及生产级可靠性指标。
- OpenClaw 强调将真实需求转成可执行自动化流程和实际业务落地；Hermes 强调工具使用经验与 Runtime 集成，不声称自研其记忆或自学习引擎。
- FDE 可突出需求拆解、工具集成、部署验证和持续迭代；没有客户交付证据时不写企业客户生产部署。

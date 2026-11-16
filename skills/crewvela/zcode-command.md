---
description: 使用 CrewVela 在已打开的 Agent 原会话之间通信
argument-hint: <通信需求>
skills: [crewvela]
---

先调用 Skill 工具加载 `crewvela`，再按该 Skill 执行用户需求。

用户需求：$ARGUMENTS

没有具体通信需求时，只介绍用法。向其他 Agent 发消息时，通过 CrewVela 查询并选择目标原会话，再向该会话发送；发送与回复遵循 Skill 的身份及回执规则。

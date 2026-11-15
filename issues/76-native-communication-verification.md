Status: ready-for-human

# Claude Code、Codex、ZCodeCLI 正常模式互聊验收

目标：使用新版 Windows EXE 的正式用户配置，验证三种 Runtime 互相实际收件、发送回复与原会话再次收件，不能只检查排队。

Claude Code ↔ Codex 最终双向通过，四条真实发送有成对工具调用/返回、数据库身份与原生转录证据；会话身份稳定，队列为空。修复长指令分流、Claude 长粘贴完成 Hook 比对、Codex 游标归位撤销收件就绪的问题。最终全量 Rust 1135 通过、49 忽略，已重新打包。

ZCodeCLI 未通过：本机 CLI 账号桥依赖桌面版组件路径，存在未修复的定位兼容缺口；复用现有原生 SDK 进行独立诊断后，当前个人 Coding Plan 订阅接口返回空列表，也未配置 Personal API 模型。仍需可用模型连接；账号路径还需补齐兼容后做真实推理与两组双向通信。没有绕过模型权益检查。

验收记录及待处理边界：[正常模式通信验证](../docs/communication-normal-verification-20261115.md)。本任务不能标为三方全部通过。

用户随后明确要求 ZCodeCLI“先不管”，因暂无可用额度，不再追问充值或继续该 Runtime 验证。本轮按 Claude Code ↔ Codex 的真实验证收尾，ZCodeCLI 的上述未验证项及兼容缺口保留后续处理。另见[自我介绍原句修复与验证](../docs/introduction-routing-verification-20261115.md)。

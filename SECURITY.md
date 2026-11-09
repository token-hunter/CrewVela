# 安全说明 / Security policy

当前版本线为 **0.1.x**。安全问题请按下述方式报告；本项目未约定安全维护或响应时限。

## 报告漏洞

请优先使用 GitHub **Private vulnerability reporting**（Security → Advisories → Report a vulnerability）。如果入口尚不存在，请勿在公开 Issue 中提供凭据、利用细节或受影响用户数据；可先提交不含细节的“请启用私密漏洞报告”请求。

私密报告提供版本/提交、平台、受影响入口、合成数据复现和影响说明。不要提交真实 Token、账号文件或完整私人日志。目前未设置可公开验证的安全邮箱，不提供虚构联系方式。

## 安全边界

CrewVela 会启动能够读写项目和执行命令的本地 Agent；只在可信目录中运行，并核对各 Agent 的权限设置。系统凭据库存储不代表所有外部 Agent 登录文件都受 CrewVela 管理。请求发送至所选模型服务；第三方插件、MCP 与 Skill 也有各自的数据访问行为。

隔离首次启动模式用于本地配置与界面验收，会禁用真实 Agent、账号和共享记忆集成；它不是任意命令的操作系统沙箱。

## 当前已知事项

2026-11-09 当前源码已通过兼容依赖更新，使 npm 全量与仅生产依赖审计均为零告警。旧历史和旧附件保留在独立私有备份中；新仓库不复用旧包。Rust 依赖审计和原生平台验收范围见[发布检查清单](docs/release-checklist.md)。密钥扫描零命中不等于证明没有任何形式的秘密，也不等于全部漏洞审计通过。

## English

The current version line is 0.1.x; no security maintenance or response-time commitment is specified. Use GitHub private vulnerability reporting when available. Do not put secrets or exploit details in public issues; if the private channel is unavailable, request its activation without disclosing details. Provide synthetic reproduction steps. Local agents can read/write projects and execute commands; the first-run isolation mode is not an OS sandbox. The current source has zero npm audit findings after dependency updates; old assets and history remain in a separate private backup. Rust dependency auditing and native acceptance have separate verification limits.

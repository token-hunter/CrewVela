Status: ready-for-agent
Execution: completed

# macOS ZCode Individual Coding Plan 模型目录为空

用户Windows截图包含GLM-5.3与GLM-5.3-Flash的low/high/max档位。macOS3.18.2登录缓存已升级enc:v2:os，CrewVela旧helper仅支持enc:v1而将v2当明文，导致当前Individual账号无法识别；默认内置Start Plan规则又触发误导提示。

复用两平台共享的模型、套餐校验和ProjectAccessTokenClient链路，只读兼容官方v2系统/managed-secret密钥。不得输出或写入密钥/令牌、迁移凭据、创建OS密钥或发送真实模型请求；Start Plan原有保护保留。验收为合成回归与本机真实目录/临时会话绑定，不调用推理。

## 实施结果

已增加只读enc:v2 os/env解密，与原生服务名、绝对路径账号摘要、来源固定、密钥指纹、AES-GCM标签验证一致；不迁移凭据、不创建/修改系统密钥。保留Windows旧格式和共享Individual目录、鉴权与推理档位逻辑。未知enc格式明确拒绝；Start Plan提示只针对当前实际选择的暂停套餐。

原生keyring异步对象保持至操作完成；单次账号helper在JSON输出完整发送后退出，避免原生句柄导致父进程超时。首次真实目录验收的45秒超时已通过此修复解决，未增加超时或削弱检查。

- Node 486通过/5跳过；ZCode Rust 69通过/16忽略。
- 已安装macOS ZCode3.18.2真实目录恢复6项，临时会话逐一绑定GLM-5.3/GLM-5.3-Flash × low/high/max成功，Start Plan误报消失。
- 全程无真实模型请求；凭据与聊天数据未输出或写入。Windows无设备实测。

## 用户验收

ARM64修复版DMG通过完整性、严格签名、版本、源码一致性与修复代码嵌入检查；用户安装测试确认没有问题，授权提交、合并main并推送GitHub。

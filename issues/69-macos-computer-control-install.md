Status: ready-for-agent
Execution: completed

# macOS 电脑控制一键安装与授权引导

实现Open Computer Use与KimiCU的官方运行时安装、完整性与签名验证、安装保护、MCP接入及授权引导。保留Windows现有电脑控制行为。完成后构建ARM64 DMG供用户安装测试。

验证与实际限制见下文及 `docs/macos-computer-control.md`。

## 实施结果

已增加两工具的安装并启用入口、官方来源校验、进度、授权与部分失败恢复。Windows安装/配置代码路径未改。

- Node完整回归480通过、5跳过。
- macOS电脑控制和安装器Rust回归16通过、1个联网验签用例默认忽略；该用例单独运行1通过。
- 实际官方OCU1.1.4、KimiCU0.7.6安装包SHA/身份/签名/公证校验通过，未安装服务或授予系统权限。
- 安装进度、失败重试、待授权、配置部分失败及Windows入口对照界面通过。
- KimiCU目前需当前账户具有/Applications写权限；无自动管理员提权。真实安装/launchd/TCC和Windows设备行为未实测。

## 用户实测反馈修复

用户安装后报告OCU权限检查exit1。已用本机正式OCU1.1.4复现：`doctor --no-ui`不受支持，正式版需`doctor --json`，输出为accessibilityTrusted/screenCaptureGranted/allGranted。已修改实际调用、严格解析与汇总一致性校验，保留旧开发JSON格式识别。新增会拒绝错误参数的命令fixture，并以已安装官方运行时通过Rust真实诊断验收（两项权限均true），全程未请求权限。

KimiCU0.7.6只读service-status/xpc-ping检查显示服务与权限正常。Node480通过/5跳过；Rust17通过/2个默认忽略，其中真实安装运行时诊断单独1通过。

## 用户验收

权限诊断修复版ARM64 DMG已通过完整性、严格签名、版本与构建源码一致性检查；用户安装测试后确认没有问题，授权提交、合并main并推送GitHub。

## 后续范围调整（2026-11-15）

以上为历史验收记录。Open Computer Use 支持已按用户要求移除，当前保留 KimiCU；见 issues/71-remove-open-computer-use.md、docs/macos-computer-control.md。

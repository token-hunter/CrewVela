Status: ready-for-human

# 修复交付程序依赖开发服务器导致白屏

## 需求

交付的 `CrewVela-20261107-memory-setup/CrewVela.exe` 在 Vite 未运行时打开后显示 localhost 拒绝连接。发布程序应内嵌前端，能够独立启动，并保留共享记忆向导。

## 验收

- 原交付文件复现 `ERR_CONNECTION_REFUSED`，在生产协议启用后通过相同场景。
- 发布入口明确开启 Tauri custom-protocol，阻止缺少生产协议的 release 构建。
- 自动检查内嵌 index.html 和入口 JS/CSS，验证实际原生窗口与共享记忆入口。
- 修正原交付路径中的可执行文件，保留原有用户配置和会话。

## Comments

- 2026-11-07：已复现原交付文件的原生错误页。根因是上轮直接 `cargo build --release` 未开启 Tauri 生产资源协议，release 优化档位仍然使用 devUrl。生产资源检查已在缺少协议时明确失败。
- 修复：增加 custom-protocol Cargo feature、生产构建入口及 build.rs 发布防护。正常开发模式仍保留 devUrl。缺少生产协议的 release 构建已按预期被拒绝；生产资源测试 1/1，通过内嵌 index.html 及入口 JS/CSS 检查。
- 验证：`npm run build:desktop` 成功；Node 393 通过 / 1 跳过；CLI --version 正常；1420 无监听时，原生 WebView2 从 `http://tauri.localhost/` 正常打开，主界面、模型设置与共享记忆入口均已实际检查。未执行记忆安装或重启 Agent 会话。
- 资源测试曾因磁盘空间不足失败；使用 Cargo 自带的本项目产物清理命令后重新运行通过，未删除源码、配置或会话数据。原有构建警告保留。
- 已替换原交付路径 `output/CrewVela-20261107-memory-setup/CrewVela.exe` 并打开修正版；SHA256 `C2F90B719FF5B440B14B6B8A9C5A5A2D57BF2EE84BB545E625DAA8B5EBA2F87E`，43,699,712 字节。故障二进制备份在忽略的临时诊断目录。

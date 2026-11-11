# 下载与安装

当前版本为 **v0.1.0**，安装包与校验文件见 [GitHub Releases](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.0)。

**附件发布在 `v0.1.0` Release 下，后续 `main` 更新不会自动更新附件。** 不同平台附件的源码提交以 Release 说明为准；本次 macOS DMG 对应 [`783bf9e`](https://github.com/token-hunter/CrewVela/commit/783bf9edd1c084419111acd96a03a8c1fd0c768a)。 新仓库使用清理后的源码与 Windows 安装包，旧历史及旧包保留在独立的私有备份中。隐私脱敏、依赖、素材与安装验收记录见[发布检查清单](release-checklist.md)。

| 资产 | 用途与当前状态 |
| --- | --- |
| `CrewVela-0.1.0-windows-x64-setup.exe` | Release 附件；Windows NSIS 安装向导，当前用户安装、中英文界面；完整安装验收仍待完成 |
| `CrewVela-0.1.0-windows-x64.exe` | 应用本体的对外命名；不是安装向导，尚未作为该 Release 的独立附件提供 |
| `CrewVela-0.1.0-windows-x64-portable.zip` | 计划包含应用 EXE、许可、安装说明和构建信息；尚未作为 Release 附件提供 |
| [CrewVela_0.1.0_macos_aarch64.dmg](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0/CrewVela_0.1.0_macos_aarch64.dmg) | macOS Apple Silicon（ARM64），约 23 MiB；ad-hoc 签名、未公证；原生完整验收仍待完成 |
| [CrewVela_0.1.0_macos_aarch64.dmg.sha256](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0/CrewVela_0.1.0_macos_aarch64.dmg.sha256) | 上述 macOS DMG 的独立校验文件 |
| `SHA256SUMS.txt` | 覆盖实际上传的 Windows 安装包与 macOS DMG；完整性散列不代替签名 |

## Windows

下载 `CrewVela-0.1.0-windows-x64-setup.exe` 并运行安装向导。WebView2 和对应 Agent CLI 仍需本机可用。**使用下载版无需安装 Rust/MSVC 或克隆源码**；运行时使用的 Node、CLI 和 ACP 适配器仍有单独依赖，无源码目录的干净机器验证仍待完成。自行使用 ZIP 分发时，解压后再运行 EXE，不要从压缩包内部启动。

当前源码的发行构建只在应用安装位置和用户运行时 PATH 中发现适配器，不引用构建机器的源码目录；未安装时通过 npx 使用固定版本。Codex 的本地安装和 npx 回退均加载同一个包装桥。开发构建仍可使用项目 node_modules。首次使用可能联网安装适配器，且必须有兼容的 Node/npm；这不代表所有适配器均已内置。

正常模式会加载本机 CrewVela 和对应 Agent 的配置，不代表包中携带了开发者历史。需要测试空白状态时用源码中的独立测试启动器；不要删除自己的正式数据来模拟新用户。

`npm run tauri -- build --no-bundle` 生成应用本体。需要安装向导时，在 Windows 按顺序执行：

```sh
npm run tauri -- build --no-bundle
npm run bundle:windows
```

第二步独立生成 NSIS 安装包，原始输出在 `src-tauri/target/release/bundle/nsis/`，对外统一命名为 `CrewVela-0.1.0-windows-x64-setup.exe`。这不是给普通 EXE 改名。Windows 专属配置提供当前用户安装、中英语言选择和许可材料；缺少 WebView2 时下载引导程序，需要网络。Node、Agent CLI 与 ACP 依赖不会由该安装包全部安装。

本地发行文件整理在 `output/releases/v0.1.0/` 并被 Git 忽略。构建、向导检查、签名和安装/卸载验收边界见 [安装包记录](release-installers-validation.md)。

未签名文件可能显示系统来源/信誉提示。公开说明签名状态并核对发行源；不要求用户关闭 Defender、SmartScreen 或其他系统防护。

## macOS Apple Silicon

直接下载 [macOS Apple Silicon DMG](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0/CrewVela_0.1.0_macos_aarch64.dmg) 和[校验文件](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0/CrewVela_0.1.0_macos_aarch64.dmg.sha256)，放在同一目录后执行：

```sh
shasum -a 256 -c CrewVela_0.1.0_macos_aarch64.dmg.sha256
```

校验通过后打开 DMG，将应用拖入 Applications；替换旧版本前先完整退出应用。使用下载版无需安装 Rust 或克隆源码，但 Node/npm、所用 Agent CLI 与账号仍需本机准备。

该包对应已合入 main 的 `783bf9e`，修复 macOS 首次展开右侧“开始”页时入口卡片高度异常。包内应用为 `CrewVela.app`；保留安装版 Chat 启动修复及现有工作区、协作、设置和 Skill 功能。原生 WKWebView 和 Chromium 的首次打开、连续 10 轮收起展开通过；Chromium 的 Windows 平台分支、入口回调、键盘、长文本及小窗口回归通过，未做 Windows 实机验证。本轮 Node 440 通过、0 失败、5 项原有跳过；生产资源和包校验通过，未重跑 Rust 全库测试。具体范围见 [Release 说明](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.0)。自行构建见 [Mac 指南](../MACOS-CODEX-HANDOFF.md)。

本地 ad-hoc 构建不等于 Developer ID 签名或公证；不提供批量删除隔离属性或关闭 Gatekeeper 的安装步骤。签名凭据不得写入源码或报告。

当前隔离测试要求 macOS 14+；这不是正式模式的最低系统版本承诺。正式版最低版本、Intel 支持均需额外验收，不能根据架构编译成功直接填写。

## 升级与卸载

升级前从旧窗口或托盘完整退出，避免同一数据目录有多个主程序。替换 EXE 或 app 不自动清除个人设置、凭据或历史；不得把删除数据当作升级必需步骤。变更数据格式时在 Release notes 写出迁移与恢复方法。

参考：[GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)、[Tauri Windows 安装包](https://v2.tauri.app/distribute/windows-installer/)、[Tauri Mac 签名](https://v2.tauri.app/distribute/sign/macos/)。

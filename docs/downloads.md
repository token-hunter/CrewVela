# 下载与安装

[English](downloads.en.md)

[2026-11-28 Beta](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.1-beta.20261128) 提供最新的 Windows x64 安装包。macOS Apple Silicon 安装包见 [2026-11-26 Beta](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.0-beta.20261126)。

| 资产 | 用途与当前状态 |
| --- | --- |
| [CrewVela-0.1.1-beta.20261128-windows-x64-setup.exe](https://github.com/token-hunter/CrewVela/releases/download/v0.1.1-beta.20261128/CrewVela-0.1.1-beta.20261128-windows-x64-setup.exe) | Windows x64 当前用户安装向导，提供简体中文和 English。应用本体可从源码单独构建 |
| [SHA256SUMS.txt](https://github.com/token-hunter/CrewVela/releases/download/v0.1.1-beta.20261128/SHA256SUMS.txt) | 上述安装包的完整性校验值 |
| [CrewVela_0.1.0_macos_aarch64.dmg](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261126/CrewVela_0.1.0_macos_aarch64.dmg) | macOS Apple Silicon 安装镜像 |
| [CrewVela_0.1.0_macos_aarch64.dmg.sha256](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261126/CrewVela_0.1.0_macos_aarch64.dmg.sha256) | macOS 安装镜像校验值 |

## Windows

Windows 安装包的源码见 [v0.1.1-beta.20261128 标签](https://github.com/token-hunter/CrewVela/tree/v0.1.1-beta.20261128)。

下载 `CrewVela-0.1.1-beta.20261128-windows-x64-setup.exe` 并运行安装向导。WebView2 和对应 Agent CLI 仍需本机可用。**使用下载版无需安装 Rust/MSVC 或克隆源码**；运行时使用的 Node、CLI 和 ACP 适配器仍有单独依赖，无源码目录的干净机器验证仍待完成。自行使用 ZIP 分发时，解压后再运行 EXE，不要从压缩包内部启动。

运行安装向导前，可在下载目录使用 PowerShell 核对 SHA-256。将 `Hash` 与同一 Release 的 `SHA256SUMS.txt` 比较。

```powershell
Get-FileHash .\CrewVela-0.1.1-beta.20261128-windows-x64-setup.exe -Algorithm SHA256
```

应用界面语言在「设置 → 常规」切换，独立于安装向导所选语言。

当前源码的发行构建只在应用安装位置和用户运行时 PATH 中发现适配器，不引用构建机器的源码目录；未安装时通过 npx 使用固定版本。Codex 的本地安装和 npx 回退均加载同一个包装桥。开发构建仍可使用项目 node_modules。首次使用可能联网安装适配器，且必须有兼容的 Node/npm；这不代表所有适配器均已内置。

正常模式会加载本机 CrewVela 和对应 Agent 的配置，不代表包中携带了开发者历史。需要测试空白状态时用源码中的独立测试启动器；不要删除自己的正式数据来模拟新用户。

`npm run tauri -- build --no-bundle` 生成应用本体。需要安装向导时，在 Windows 按顺序执行：

```sh
npm run tauri -- build --no-bundle
npm run bundle:windows
```

第二步独立生成 NSIS 安装包，原始输出在 `src-tauri/target/release/bundle/nsis/`，对外统一命名为 `CrewVela-0.1.1-beta.20261128-windows-x64-setup.exe`。这不是给普通 EXE 改名。Windows 专属配置提供当前用户安装、中英语言选择和许可材料；缺少 WebView2 时下载引导程序，需要网络。Node、Agent CLI 与 ACP 依赖不会由该安装包全部安装。

本地发行文件整理在 `output/releases/<tag>/` 并被 Git 忽略。构建、向导检查、签名和安装/卸载验收边界见 [安装包记录](release-installers-validation.md)。

未签名文件可能显示系统来源/信誉提示。公开说明签名状态并核对发行源；不要求用户关闭 Defender、SmartScreen 或其他系统防护。

## macOS Apple Silicon

DMG 由提交 [`35d1c0e`](https://github.com/token-hunter/CrewVela/commit/35d1c0ea35b141b5b61afe14f7a29758417391ef) 构建，使用 ad-hoc 签名，未经 Apple 公证。将 DMG 和对应的 `.sha256` 文件下载到同一目录，运行：

```sh
shasum -a 256 -c CrewVela_0.1.0_macos_aarch64.dmg.sha256
```

通过校验后，打开 DMG 并将 CrewVela 拖入「应用程序」。源码构建及原生验收步骤见 [Mac 指南](../MACOS-CODEX-HANDOFF.md)。Intel Mac 和 Linux 暂无已验证的安装包。

## 升级与卸载

升级前从旧窗口或托盘完整退出，避免同一数据目录有多个主程序。替换 EXE 或 app 不自动清除个人设置、凭据或历史；不得把删除数据当作升级必需步骤。变更数据格式时在 Release notes 写出迁移与恢复方法。

参考：[GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)、[Tauri Windows 安装包](https://v2.tauri.app/distribute/windows-installer/)、[Tauri Mac 签名](https://v2.tauri.app/distribute/sign/macos/)。

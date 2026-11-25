# Download and install

[简体中文](downloads.md)

The latest download is [Windows x64 Beta · 2026-11-25](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.0-beta.20261125), built from [`81e4f28`](https://github.com/token-hunter/CrewVela/commit/81e4f2836053352111d7089ea9ccbff5f2779477). The installer reports app version `0.1.0`. See [GitHub Releases](https://github.com/token-hunter/CrewVela/releases) for earlier builds.

## Windows x64

1. Download the [Windows installer](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261125/CrewVela-0.1.0-windows-x64-setup.exe) and [SHA256SUMS.txt](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261125/SHA256SUMS.txt).
2. In PowerShell, run the command below from your downloads folder. Compare the `Hash` value with the entry in `SHA256SUMS.txt`.
3. Run the installer. The installer is unsigned, installs for the current Windows user, and offers English and Simplified Chinese. In the app, choose your interface language in **Settings → General**.

```powershell
Get-FileHash .\CrewVela-0.1.0-windows-x64-setup.exe -Algorithm SHA256
```

Windows WebView2 is required. The installer can download its bootstrapper if WebView2 is missing. Install and sign in to the agents you plan to use. Some native-chat integrations require Node.js and an ACP adapter. Rust and MSVC are only needed to build CrewVela from source.

Before upgrading, quit the old app from the system tray. Settings, sessions, and credentials are stored separately from the application. The release notes list the checks performed on each build.

## macOS and Linux

GitHub Releases currently has Windows installers. On an Apple Silicon Mac, follow the [macOS source build instructions](../README.en.md#run-from-source). Local macOS builds use ad-hoc signing. Developer ID signing, notarization, and full native acceptance are pending. Linux and Intel Mac have no verified install downloads.

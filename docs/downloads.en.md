# Download and install

[简体中文](downloads.md)

The [2026-11-26 Beta](https://github.com/token-hunter/CrewVela/releases/tag/v0.1.0-beta.20261126) provides Windows x64 and macOS Apple Silicon installers. Both use app version `0.1.0`.

## Windows x64

The Windows installer was built from [`0062a19`](https://github.com/token-hunter/CrewVela/commit/0062a198a441973c794c085e81c4fa975a35e50f).

1. Download the [Windows installer](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261126/CrewVela-0.1.0-windows-x64-setup.exe) and [SHA256SUMS.txt](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261126/SHA256SUMS.txt).
2. In PowerShell, run the command below from your downloads folder. Compare the `Hash` value with the entry in `SHA256SUMS.txt`.
3. Run the installer. The installer is unsigned, installs for the current Windows user, and offers English and Simplified Chinese. In the app, choose your interface language in **Settings → General**.

```powershell
Get-FileHash .\CrewVela-0.1.0-windows-x64-setup.exe -Algorithm SHA256
```

Windows WebView2 is required. The installer can download its bootstrapper if WebView2 is missing. Install and sign in to the agents you plan to use. Some native-chat integrations require Node.js and an ACP adapter. Rust and MSVC are only needed to build CrewVela from source.

Before upgrading, quit the old app from the system tray. Settings, sessions, and credentials are stored separately from the application. The release notes list the checks performed on each build.

## macOS Apple Silicon

The DMG was built from [`35d1c0e`](https://github.com/token-hunter/CrewVela/commit/35d1c0ea35b141b5b61afe14f7a29758417391ef). It is ad-hoc signed and has not been notarized by Apple.

Download the [ARM64 DMG](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261126/CrewVela_0.1.0_macos_aarch64.dmg) and its [SHA-256 file](https://github.com/token-hunter/CrewVela/releases/download/v0.1.0-beta.20261126/CrewVela_0.1.0_macos_aarch64.dmg.sha256) into the same folder, then run:

```sh
shasum -a 256 -c CrewVela_0.1.0_macos_aarch64.dmg.sha256
```

After verification, open the DMG and drag CrewVela into Applications. See the [macOS source build instructions](../README.en.md#run-from-source) to build locally. Intel Mac and Linux currently have no verified installers.

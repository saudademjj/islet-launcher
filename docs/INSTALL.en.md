# Installing Islet

[简体中文](INSTALL.md)

Packages are on the [Releases](https://github.com/saudademjj/islet-launcher/releases/latest) page. They aren't code-signed yet, so your system may warn you the first time you open Islet. Here is what to do on each one.

To make sure a download is intact, compare its SHA-256 with `SHA256SUMS.txt` on the release page:

```powershell
Get-FileHash .\Islet-0.4.0-x64-setup.exe -Algorithm SHA256   # Windows
```

```bash
shasum -a 256 Islet-0.4.0-arm64.dmg                        # macOS / Linux
```

## Windows

Requires 64-bit Windows 10 or 11. On an ARM PC (Snapdragon and the like), get the files with `arm64` in their names.

- **Installer** `Islet-<version>-x64-setup.exe`: double-click to install, no administrator rights needed. Updates install from inside the launcher.
- **Portable** `Islet-<version>-x64-portable.exe`: put it in any folder and double-click. Your data still lives in `%APPDATA%\com.saudade.mclauncher`.

If you see "Windows protected your PC", click **More info → Run anyway**.

Islet draws its interface with the system's WebView2. Windows 11 and up-to-date Windows 10 already have it, and the installer downloads it when it's missing. If the portable version won't open, install the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) first.

## macOS

Requires macOS 12 or later. Get `arm64` for Apple silicon, `x86_64` for Intel.

1. Open `Islet-<version>-arm64.dmg` (`Islet-<version>-x86_64.dmg` on Intel) and drag **Islet** into **Applications**.
2. If the first launch says the developer cannot be verified, right-click Islet in **Applications** → **Open** → **Open**. On newer macOS, go to **System Settings → Privacy & Security** and click **Open Anyway** near the bottom.

When it runs from **Applications**, Islet can update itself.

## Linux (experimental)

Requires x86_64 or ARM64 and WebKitGTK 4.1, which Ubuntu 22.04, Debian 12 and newer already include. On ARM64, read `aarch64` for `x86_64` and `arm64` for `amd64` in the names below.

- **AppImage**: `chmod +x Islet-<version>-x86_64.AppImage`, then run it. Run as an AppImage, Islet can update itself.
- **deb**: `sudo apt install ./Islet-<version>-amd64.deb`.

Microsoft sign-in tokens are kept in the Secret Service (GNOME Keyring, KWallet and the like). Without one running, Islet can't remember your sign-in.

## Uninstalling

- Windows: uninstall from **Settings → Apps**. For the portable version, delete the exe.
- macOS: move **Islet** to the Trash.
- Linux: delete the AppImage, or remove the deb in your software manager.

Uninstalling leaves your game data alone. To remove everything, first back up the saves you want to keep (`minecraft/saves` in each world's folder), then delete the data folder:

| System | Data folder |
|---|---|
| Windows | `%APPDATA%\com.saudade.mclauncher` |
| macOS | `~/Library/Application Support/com.saudade.mclauncher` |
| Linux | `~/.local/share/com.saudade.mclauncher` |

Microsoft sign-in tokens are stored separately in the system credential store, under the service name `com.saudade.mclauncher.auth`. Delete them in Windows Credential Manager or in macOS Keychain Access.

## Troubleshooting

- When the game crashes, Islet works out why, and the common causes take one click to fix.
- Islet's own logs are in `logs/` inside the data folder.
- Still stuck? [Open an issue](https://github.com/saudademjj/islet-launcher/issues/new/choose) with the link you get from **Upload log for help** in the log panel.

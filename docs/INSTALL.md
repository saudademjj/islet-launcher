# 安装说明

[English](INSTALL.en.md)

安装包在 [Releases](https://github.com/saudademjj/islet-launcher/releases/latest) 页面。安装包目前没有代码签名，第一次打开时系统可能会提示风险，下面写了各系统的处理方法。

想确认下载的文件完整无误，可以和 Release 页 `SHA256SUMS.txt` 里的 SHA-256 核对：

```powershell
Get-FileHash .\Islet-0.4.0-x64-setup.exe -Algorithm SHA256   # Windows
```

```bash
shasum -a 256 Islet-0.4.0-arm64.dmg                        # macOS / Linux
```

## Windows

系统要求：64 位 Windows 10 或 11。

- **安装版** `Islet-<版本>-x64-setup.exe`：双击安装，不需要管理员权限。之后可以在启动器里直接更新。
- **免安装版** `Islet-<版本>-portable.exe`：放到任意文件夹双击运行；数据仍然保存在 `%APPDATA%\com.saudade.mclauncher`。

如果出现「Windows 已保护你的电脑」，点「更多信息 → 仍要运行」。

启动器用系统的 WebView2 显示界面。Windows 11 和更新过的 Windows 10 自带；安装版在缺少时会自动下载，免安装版打不开时请先安装 [WebView2 运行时](https://developer.microsoft.com/microsoft-edge/webview2/)。

## macOS

系统要求：macOS 12 或更新，Apple 芯片。

1. 打开 `Islet-<版本>-arm64.dmg`，把「Islet」拖进「应用程序」。
2. 第一次打开如果提示「无法验证开发者」：在「应用程序」里右键点它 →「打开」→「打开」。较新的 macOS 上需要到「系统设置 → 隐私与安全性」底部点「仍要打开」。

从「应用程序」运行时可以在启动器里直接更新。

## Linux（实验性）

需要 x86_64 系统和 WebKitGTK 4.1（Ubuntu 22.04 / Debian 12 及更新的发行版自带）。

- **AppImage**：`chmod +x Islet-<版本>-x86_64.AppImage` 后运行。以 AppImage 运行时可以在启动器里直接更新。
- **deb**：`sudo apt install ./Islet-<版本>-amd64.deb`。

微软账号令牌保存在 Secret Service（GNOME 钥匙圈、KWallet 等），没有运行这类服务时无法保存登录状态。

## 卸载

- Windows：在「设置 → 应用」里卸载；免安装版直接删除 exe。
- macOS：把「Islet」移到废纸篓。
- Linux：删除 AppImage；deb 安装的在软件管理器里卸载。

卸载不会删除游戏数据。需要彻底清除时，先备份想留的存档（各世界目录下的 `minecraft/saves`），再删除数据目录：

| 系统 | 数据目录 |
|---|---|
| Windows | `%APPDATA%\com.saudade.mclauncher` |
| macOS | `~/Library/Application Support/com.saudade.mclauncher` |
| Linux | `~/.local/share/com.saudade.mclauncher` |

微软账号令牌另存在系统凭据库，服务名为 `com.saudade.mclauncher.auth`，可以在 Windows 凭据管理器或 macOS「钥匙串访问」里删除。

## 遇到问题

- 游戏崩溃时启动器会自动分析原因，多数情况下可以一键修复。
- 启动器自己的日志在数据目录下的 `logs/`。
- 仍然解决不了，请[提交 Issue](https://github.com/saudademjj/islet-launcher/issues/new/choose)，附上日志面板里「上传日志求助」得到的链接。

# 参与开发

感谢你愿意帮忙！提交 Issue 和 Pull Request 都欢迎，中文或英文都可以。

- 报告问题、提建议：用 [Issue 模板](https://github.com/saudademjj/islet-launcher/issues/new/choose)。
- 较大的改动（新功能、改交互、加依赖）请先开 Issue 讨论，避免白做。
- 安全问题不要公开提交，见 [SECURITY.md](SECURITY.md)。

提交的代码以本项目的 [GPL-3.0](LICENSE) 许可发布。

## 环境

- Rust stable（验证版本 1.96），Node.js 20+（验证版本 24）
- [Tauri 2 的系统依赖](https://tauri.app/start/prerequisites/)：
  - macOS：Xcode Command Line Tools
  - Windows：Visual Studio Build Tools（C++ 与 Windows SDK），WebView2（Windows 11 自带）
  - Linux：`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libdbus-1-dev`

## 常用命令

```bash
npm install
npm run dev                # 只看界面：浏览器打开 http://localhost:5173，使用内置演示数据
npm run tauri dev          # 真实窗口 + 真实后端，改前端代码即时生效
cargo test -p islet  # 核心库单元测试
npm test                   # 界面逻辑单元测试（vitest）
npx tsc --noEmit -p .      # 类型检查
```

提交前请确认 `cargo test`、`npm test` 和类型检查都通过；改了界面的话附上截图。

### 开发辅助

- `MC_LAUNCHER_DATA_DIR=<目录>`：使用独立的数据目录，不碰你真实的账号和世界。
- 浏览器预览（`npm run dev`）在没有 Tauri 时自动换成 `web/src/api/mock.ts` 的演示后端，可以完整点击安装、启动、日志、登录等流程。新增后端命令时记得在这里补上演示实现。
- 前后端接口自测：在 `web/.env.development.local` 写入 `VITE_SELFTEST=1`，再用独立数据目录运行 `npm run tauri dev`。`web/src/dev/selftest.ts` 会在真实窗口里依次调用各个命令、等待对应事件，结果写进数据目录下的 `logs/launcher.log.*`（搜索 `[selftest]`）。它会创建并删除一个世界和一个离线账号，不下载游戏文件。
- 页面脚本报错会经 `frontend_log` 命令写进同一份日志；如果页面没能显示窗口，Rust 端 4 秒后会强制显示。

## 代码结构

```
src/                         核心库，与界面无关
  services.rs                服务层：后台任务、事件（UiEvent）
  core/                      auth、mojang、download、java、launch、instances、p2p、settings ...
src-tauri/
  src/main.rs                窗口、插件、窗口显示兜底
  src/commands.rs            前端可调用的命令
  src/events.rs              事件转发与日志批量发送
  tauri.conf.json            窗口与打包配置
  capabilities/default.json  前端权限白名单
web/src/
  api/                       类型定义、Tauri 后端与浏览器演示后端
  store.ts                   状态与动作（zustand）
  layout/ pages/ overlays/ components/
  styles/                    tokens.css（颜色、动效令牌）与 app.css
  dev/selftest.ts            前后端接口自测（仅开发）
scripts/                     打包、更新清单、第三方许可清单、字体子集化
```

## 界面约定

- 视觉：森林绿外框、纸色内容区、草方块绿作唯一强调色，全部纯实色，不用渐变和毛玻璃；像素风图标和地形。
- 动效：统一时长（120 / 200 / 300 ms），进场 ease-out、退场 ease-in；尊重系统和设置里的「减少动效」。
- 文案：所有界面文字用 `t("中文", "English")` 同时写中英文。
- 界面小字用的 Fusion Pixel 已按用到的字符子集化。新增中文文字后，下载 [Fusion Pixel](https://github.com/TakWolf/fusion-pixel-font/releases) 的 12px proportional zh_hans 字体，运行 `python3 scripts/subset-font.py <字体文件>` 重新生成（需要 fontTools 和 brotli），否则新字会缺字。

## 打包

```bash
./scripts/package-macos.sh                 # macOS：build/Islet.app 和 .dmg
```

```powershell
.\scripts\package-windows.ps1              # Windows：NSIS 安装包 + 免安装 exe，在 build\windows
.\scripts\package-windows.ps1 -Quick       # 自测用：thin LTO，链接快得多，exe 稍大
```

Linux 包（deb、AppImage）在 GitHub Actions 里构建，见 `.github/workflows/linux.yml`。

macOS 打包默认 4 个并行编译任务以减少笔记本发热，`JOBS=8 ./scripts/package-macos.sh` 可以加快；Windows 默认用满所有核心，设 `JOBS=4` 可以限制。

构建时可选的环境变量（本机可以写进 `.env`，参照 `.env.example`，不入库）：

| 变量 | 作用 |
|---|---|
| `MC_LAUNCHER_CLIENT_ID` | 内置的微软登录 Azure Client ID（见 [docs/azure-setup.md](docs/azure-setup.md)） |
| `MC_LAUNCHER_UPDATE_URL` | 更新清单地址（https）；不设则不检查更新 |
| `RELEASE_BASE_URL`、`RELEASE_PAGE` | 设置后用 `scripts/update-manifest.py` 把安装包和 SHA-256 写进 `build/latest.json` |
| `APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_TEAM_ID`、`APPLE_PASSWORD` | macOS Developer ID 签名与公证 |
| `WINDOWS_SIGN_CERT` + `WINDOWS_SIGN_PASSWORD`，或 `WINDOWS_SIGN_THUMBPRINT` | Windows 代码签名 |

## 发布

1. 更新 `Cargo.toml`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 和 `package.json` 里的版本号。
2. 推送 `v<版本>` 标签。`.github/workflows/release.yml` 会构建 Windows、Linux 和 macOS 三个平台，生成 `latest.json`，创建一个**草稿** Release。
3. 在打包好的版本上按 [docs/acceptance-checklist.md](docs/acceptance-checklist.md) 验收，填写更新说明后发布草稿。发布之后，已安装的启动器才会看到新版本。

仓库的 Actions Variables 里可以设置 `MC_LAUNCHER_CLIENT_ID`；Secrets 里可以设置 Windows 签名证书（`WINDOWS_CERTIFICATE`，base64 的 .pfx，及 `WINDOWS_CERTIFICATE_PASSWORD`）。

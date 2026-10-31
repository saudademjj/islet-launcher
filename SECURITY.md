# 安全问题报告 / Security Policy

## 支持的版本 / Supported versions

只为最新发布的版本修复安全问题。
Security fixes go into the latest release only.

## 报告方式 / Reporting a vulnerability

请**不要**在公开 Issue 里报告安全问题。到仓库的 [Security → Report a vulnerability](https://github.com/saudademjj/islet-launcher/security/advisories/new) 私下提交，写清楚：

- 受影响的版本和系统；
- 复现步骤或概念验证；
- 可能造成的影响。

一般会在一周内回复。修复发布之前请不要公开细节；修复后会在 Release 说明里致谢（如果你愿意）。

Please **do not** report security problems in public issues. Use [Security → Report a vulnerability](https://github.com/saudademjj/islet-launcher/security/advisories/new) with the affected version and system, steps to reproduce and the impact. Expect a reply within a week; please keep details private until a fix is released.

## 范围 / Scope

特别关心的问题 / Of particular interest:

- 账号令牌泄露（写进文件、日志或上传的日志） / sign-in tokens leaking into files, logs or uploaded logs
- 下载内容的校验被绕过（游戏文件、Java、EasyTier、更新包） / bypassing verification of downloaded game files, Java, EasyTier or updates
- 界面能读写数据目录以外的文件，或执行任意命令 / the UI reading or writing outside the data folder, or running arbitrary commands
- P2P 联机房间被未授权的人加入或窃听 / unauthorized access to or eavesdropping on P2P rooms

模组、整合包和服务器本身的安全问题请报告给它们的作者。
Problems in third-party mods, modpacks or servers belong with their authors.

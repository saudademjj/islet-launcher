# Azure 应用注册（微软正版登录）

微软正版登录使用 OAuth 2.0 公开客户端流程，不需要 client secret。正式发布的安装包会内置本项目的 Client ID；自己从源码构建时，用 `MC_LAUNCHER_CLIENT_ID` 环境变量内置你自己的，或者在「设置 → 微软登录」里填写。不要复用其他启动器的 Client ID。

> [!IMPORTANT]
> 新注册的 Azure 应用**默认不能登录 Minecraft**：微软账号授权和 Xbox Live 认证都能成功，但最后一步换取 Minecraft 令牌会被拒绝。必须先按下面的「Mojang AppID 审核」申请，通过后才能使用。本项目的申请正在进行中，在此之前发布版的微软登录不可用。

## 步骤

1. 打开 [Azure Portal](https://portal.azure.com/)，登录你的微软账号。
2. 进入 **Microsoft Entra ID → App registrations → New registration**。
3. 填写：
   - **Name**：你的启动器名称。名称里不要包含「Minecraft」（审核会因此拒绝）
   - **Supported account types**：选择 **Personal Microsoft accounts only**
   - **Redirect URI**：
     - 平台选择 **Mobile and desktop applications**
     - 填写 `http://localhost`
4. 点击 **Register**。
5. 在应用概览页复制 **Application (client) ID**（一个 GUID，形如 `00000000-0000-0000-0000-000000000000`）。
6. 进入 **Authentication**：
   - 确认 Redirect URI 中有 `http://localhost`
   - 将 **Allow public client flows** 设为 **Yes**
   - 保存
7. 不需要创建 client secret，也不需要预配 API permissions。登录时启动器会请求 `XboxLive.signin offline_access` 权限。
8. 回到启动器「设置 → 微软登录」，把 Client ID 粘贴进去并保存；或者构建时设置 `MC_LAUNCHER_CLIENT_ID`。
9. 按下一节申请 Mojang AppID 审核。

## Mojang AppID 审核

Mojang 只允许审核通过的 Azure 应用调用 Minecraft 登录接口（`login_with_xbox`）。

1. 打开 [aka.ms/mce-reviewappid](https://aka.ms/mce-reviewappid)，填写申请表：Client ID、Tenant ID（应用概览页的 Directory (tenant) ID）、启动器名称和网站、用途说明。
2. 审核通常需要几天到几周，结果会发到申请邮箱。
3. 通过前登录会在「换取 Minecraft 令牌」这一步失败；外置登录不受影响；离线账号要等有拥有 Java 版的微软账号登录后才能添加。

被拒绝时邮件不一定说明原因，可能的原因有：

- 应用名称包含「Minecraft」或 Mojang 的其他商标；
- 没有公开的网站或项目主页，或者主页上看不出这是一个启动器；
- 看起来不像面向公众发布的项目（例如还没有公开的发行版本）。

改正后可以再次申请。项目主页写清楚「非 Mojang / Microsoft 官方产品」会有帮助。

## 为什么重定向地址只填 `http://localhost`

启动器登录时会在 `127.0.0.1` 上启动一个临时回调服务，端口随机。微软对桌面应用的 loopback 重定向允许使用任意端口，所以注册 `http://localhost` 即可，不需要固定端口。

如果本机防火墙拦截了回环回调，可以在登录时改用「设备代码登录」：启动器会显示一个代码和登录链接，不需要本机监听端口。

## 常见错误

| 错误 | 原因 | 处理 |
|---|---|---|
| `Azure Client ID 无效，或应用没有启用公开客户端` | Client ID 填错，或没有开启 public client flows | 检查 GUID，回到 Authentication 开启 Allow public client flows |
| `redirect_uri` 不匹配 | Redirect URI 没有配置 `http://localhost` | 在 Authentication 中添加 Mobile and desktop applications 平台和 `http://localhost` |
| `该微软账号未拥有 Minecraft Java 版` | 账号没有 Java 版拥有记录 | 换用拥有 Java 版的账号；离线账号不能用于正版在线服务器 |
| `该微软账号还没有 Xbox 档案` | 微软账号未创建 Xbox 档案 | 先在 xbox.com 登录并创建档案 |
| `儿童账号需要加入 Microsoft 家庭组` | 账号是儿童账号 | 由家庭组织者加入家庭组后再登录 |
| 浏览器授权成功，但最后提示无法登录 Minecraft | Azure 应用还没有通过 Mojang AppID 审核 | 按上文申请审核；通过前只能用外置登录 |
| 浏览器登录完成后启动器没有反应 | 回环回调被拦截/超时 | 重新登录并改用设备代码登录 |

## 安全说明

- 启动器只拿到微软 OAuth 的 refresh token，并把它保存在系统凭据库（Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service）；access token 只在内存中使用。
- 启动器不会接触你的微软密码，密码只在微软官方登录页面输入。
- 不需要也不应该把 client secret 放进启动器；公开客户端使用 PKCE 保证授权码安全。

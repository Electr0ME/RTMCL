# 微软 Device Code 登录配置

当前仅在 Windows 联调；WSL / WSLg 验收按用户要求暂缓。测试用离线登录已移除，游戏启动必须使用微软账户并完成在线认证。

## 应用注册

1. 在 Microsoft Entra 应用注册中，确认“支持的账户类型”包含个人 Microsoft 账户。当前实现使用 `consumers` 端点，不使用仅组织账户的租户登录。
2. 在“身份验证 / Authentication”设置中，将“允许公共客户端流 / Allow public client flows”设为“是”，保存。
3. 当前直接调用 OAuth Device Code 协议，不需要配置重定向 URL，也不需要 Client Secret。授权由终端显示网址与设备码，用户在微软页面登录并同意，终端轮询取得结果。

依据：[微软公共客户端配置](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-registration)、[无需重定向 URI 的协议说明](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url)、[Device Code 协议及 consumers 端点](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code)。本项目没有使用浏览器授权码回调；若将来切换 Authorization Code + PKCE，再按该流程配置 redirect URI。

## Windows 使用

在 PowerShell 中运行，将占位符替换为应用的 Application（client）ID：

```powershell
$env:RTMCL_MICROSOFT_CLIENT_ID = '<ClientID>'
cargo run --locked -- --data-dir .rtmcl/p0-windows account login
cargo run --locked -- --data-dir .rtmcl/p0-windows account list
cargo run --locked -- --data-dir .rtmcl/p0-windows p0 launch <实例ID> --account <账户ID>
```

也可用 `account login --client-id <ClientID>` 单次传入。Client ID 在账户资料中与账户关联，不硬编码进仓库。当前授权范围为 `XboxLive.signin offline_access`；认证专用网络不经过 BMCLAPI 下载路由。

刷新令牌写入 Windows 凭据服务；失败时明确标记 SessionOnly，不写明文。仅会话模式应在同一进程登录并启动：

```powershell
cargo run --locked -- --data-dir .rtmcl/p0-windows account login --client-id <ClientID> --session-only --launch <实例ID>
```

## 验收顺序

1. 取得设备码，在微软页面确认本项目应用并完成授权，取得 Minecraft 玩家资料。
2. 关闭当前 CLI 后重新运行，确认系统凭据可用于刷新及启动游戏。
3. 验证刷新令牌轮换和退出登录清理，不将账户文件存在当作凭据可用。

仅注册 Client ID 或成功取得设备码不等于整个 Minecraft 登录链路通过；还需实际完成 Microsoft → Xbox Live → XSTS → Minecraft Services → 玩家资料。真实结果记录于 [P0 验证记录](p0-validation.zh-CN.md)。

如果浏览器显示授权成功而终端报错，以终端后续认证结果判断是否取得玩家资料。终端会区分 Microsoft、Xbox Live、XSTS、Minecraft Services 令牌和玩家资料阶段，保留 HTTP 状态码；对 `Invalid app registration` 仅显示固定提示，不输出响应正文。该错误需核对本项目 Client ID 的服务接入，不能仅凭浏览器成功页面判定完整登录通过。

## 当前联调结果

Windows 真实授权已通过 Microsoft、Xbox Live 和 XSTS；Minecraft Services `/authentication/login_with_xbox` 返回 HTTP 403，尚未取得玩家资料、保存刷新凭据或启动正版会话。无令牌的可达性探测返回预期的 HTTP 400 缺参错误。

用户确认只注册了 Entra 应用，尚未申请 Minecraft Java Edition API 接入。应用注册 / 浏览器同意与 Minecraft 服务放行需要分别确认；403 本身不足以确定具体错误消息，但目前缺少 API 接入申请是明确待办。

## 已核实的官方申请入口

2026-10-20 通过浏览器实际读取 [Java Edition Game Service API Review or Application Process](https://help.minecraft.net/hc/en-us/articles/16254801392141)。官方说明新应用需请求人工审核并加入允许列表，正文指向 [AppID Review 申请表](https://aka.ms/mce-reviewappid)。本轮已打开表单核实，未填写或提交；无需转向 ID@Xbox 游戏发行申请来代替这个表单。

新申请选择 `New AppID for Approval`，需准备与 Azure Portal 信息可核对的联系邮箱、应用名、Application（Client）ID、Directory（Tenant）ID、项目网站 / 介绍地址，以及真实的用途说明。Tenant ID 不是 Client ID，需从应用概览另取。当前应用显示名为 RTMCL。表单称按周审核，不承诺获批时间，不重复提交催办。

### 当前测试模式与申请条件

申请表明确要求游戏前认证及所有权校验，不接受绕过认证、所有权、许可或其他安全功能的应用，且不因服务器配置不同而豁免。

测试用离线登录已按用户最新要求从应用移除，包括创建命令、UUID 生成和免认证启动参数。旧测试账户不能选择；当前启动只走微软账户的在线刷新和 Minecraft Services / 玩家资料流程，失败即停止。相关凭据测试使用仅在测试构建中的合成微软资料，不提供可玩会话。

该删除不等同于已获批，也不等同于真实所有权、完整正版启动、系统凭据及刷新验收通过；这些仍须在 API 放行后验证。历史主菜单截图使用已移除的测试登录，只能作为安装 / 渲染基础设施的历史证据。

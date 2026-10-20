# RTMCL

**Rust TUI Minecraft Launcher** 是使用 Rust 和 LightyLauncherLib 开发的开源 Minecraft Java 版启动器。

[English](README.md)

项目旨在通过终端界面管理游戏实例、账户、Java、下载和游戏进程。目标平台为 Windows x64 与 Linux x64，目前以 Windows 本机验证为主。

## 当前能力

- 原版与 Fabric 的独立实例、安装和启动。
- 官方与 BMCLAPI 下载来源、有序回退、断点续传及大小 / 哈希校验。
- Java 探测、选择和托管运行时安装。
- 独立游戏进程监督与每次运行的游戏日志。
- 可用的 CLI，以及最小 TUI 实例浏览界面。

## 开发状态

项目处于早期开发阶段。P0 安装与启动基础设施已实现，P1 应用服务与 CLI 继续完善，完整 TUI 和打包发布尚在计划中。

Windows 开发账户启动已验证。真实微软授权、Xbox Live 和 XSTS 已通过检查，但 Minecraft Services 令牌交换返回 HTTP 403；API 接入、完整正版启动、所有权及凭据刷新验收仍未完成。项目不宣称已获 Microsoft 或 Mojang 批准。

空缓存、官方端点不可达条件下的真实镜像完整安装也仍待验收。新增 WSL / WSLg 验收暂缓；已配置 Windows 与 Ubuntu CI。

## 账户规则

默认构建创建或启动本地账户时，须刷新其绑定的微软账户，并检查 Java 版所有权与可玩授权。绑定账户被移除或认证失败时拒绝访问。本地身份不提供微软正版联机会话。

源码提供显式的 `dev-local-accounts` 特性，用于本机 debug 开发时使用无绑定账户。它默认关闭，标准 release 构建禁止启用，开发专用账户不能被默认构建用于启动。这项开发例外如实公开，不代表已取得 API 批准。

## 构建要求

固定使用 Rust **1.98.0** 与 LightyLauncherLib **26.9.3**，构建需安装平台 C/C++ 工具链。使用 `cargo build --workspace --locked` 构建，`cargo run --locked -- --help` 查看 CLI；配置参考 [config.example.toml](config.example.toml)。

仓库不包含 Minecraft 文件、Java 归档、账户凭据或本机开发记录。

## 许可

自有代码采用 [MIT 许可证](LICENSE)。Lighty 保留独立许可与版权声明，见 [第三方说明](THIRD_PARTY_NOTICES.md) 和 [本地补丁边界](vendor/lighty/RTMCL-PATCHES.md)。

RTMCL 非 Minecraft 官方产品，与 Mojang 或 Microsoft 没有关联，也未获其背书。

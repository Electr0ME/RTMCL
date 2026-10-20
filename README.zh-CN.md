# RTMCL — Rust TUI Minecraft Launcher

[English](README.md) · [文档索引](docs/README.md) · [应用接入审核状态](docs/app-review-status.md)

面向 Windows x64 与 Linux x64 的 Minecraft Java 启动器。核心为固定版本 **LightyLauncherLib 26.9.3**，已实现 P0 安装与启动基础设施。测试用离线登录已移除，启动必须使用微软账户并完成在线认证。当前以 Windows 验收为主，WSL / WSLg 验收按用户要求暂缓；完整 TUI 留在 P2，当前通过 CLI 验证安装与启动。

工具链固定为 **Rust 1.98.0**。首次构建需要网络及平台 C/C++ 编译工具；Linux 凭据依赖采用 vendored D-Bus 构建。

```sh
cargo run --locked -- doctor
cargo run --locked -- --data-dir .rtmcl/demo config init
cargo run --locked -- --data-dir .rtmcl/demo instance create "原版测试" --game-version 1.21.1
cargo run --locked -- --data-dir .rtmcl/demo instance create "Fabric 测试" --game-version 1.21.1 --fabric-version 0.16.10
cargo run --locked -- --data-dir .rtmcl/demo instance list
```

以下 `<实例ID>` 使用创建命令返回的 UUID：

```sh
cargo run --locked -- --data-dir .rtmcl/demo p0 --source bmclapi versions --limit 10
cargo run --locked -- --data-dir .rtmcl/demo p0 --source mirror-first install <实例ID>
```

`--source official` / `bmclapi` 为严格单一来源，`mirror-first` / `official-first` 按序回退；省略则使用全局配置。严格 BMCLAPI 暂不能自动安装 Java，需预先准备并用 `--java` 指定。中断后可重跑安装，已有文件会重新校验复用。

省略 Java 时按元数据需求探测 PATH 或安装经过 SHA256 校验的 Adoptium 运行时。默认启动后 CLI 返回，独立监督继续管理游戏与锁；`--wait` 等待退出，Ctrl+C 只结束等待，显式终止用 `p0 stop`。

微软登录使用 Device Code：无需配置重定向 URL，应用须允许个人微软账户，并开启“允许公共客户端流”。Windows 实测已通过 Microsoft、Xbox Live 和 XSTS，Minecraft Services 令牌交换返回 403，API 接入尚未申请；完整正版登录及真实凭据刷新尚未通过。配置步骤见 [微软登录说明](docs/microsoft-login.zh-CN.md) 与 [审核状态](docs/app-review-status.md)。下面的启动命令需要先完成真实认证，当前 403 未解决时不会启动游戏。

```sh
cargo run --locked -- --data-dir .rtmcl/demo account login --client-id <ClientID>
cargo run --locked -- --data-dir .rtmcl/demo account list
cargo run --locked -- --data-dir .rtmcl/demo p0 launch <实例ID> --account <账户ID>
cargo run --locked -- --data-dir .rtmcl/demo p0 launch <实例ID> --account <账户ID> --java "/absolute/path/to/java" --wait
cargo run --locked -- --data-dir .rtmcl/demo p0 stop <实例ID>
cargo run --locked -- --data-dir .rtmcl/demo account logout <账户ID>
```

可用 `RTMCL_MICROSOFT_CLIENT_ID` 替代参数。系统凭据不可用时明确使用本次会话，可通过 `account login --session-only --client-id <ClientID> --launch <实例ID>` 在同一进程中登录并启动。令牌不写入 TOML，认证失败即停止。旧版测试账户记录不会进入可选账户列表。

`--data-dir` / `RTMCL_DATA_DIR` 覆盖数据根目录；缺省遵循系统目录规范。`paths` 查看路径，[配置示例](config.example.toml) 包含来源及启动设置。无子命令或 `tui` 打开最小 TUI，支持 WASD / 方向键、Enter / 空格、Esc；账户和游戏操作目前使用 CLI。

```sh
cargo fmt --package launcher-app --package launcher-terminal -- --check
cargo clippy --workspace --all-targets --no-deps --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

上游禁用 tracing 时有部分 unused/dead-code 警告，项目自身 Clippy 拒绝警告。格式检查限定应用 crate，避免重排第三方快照。

P0 尚未全部验收。真实安装、主菜单与缺少的账户/镜像证据分别记录于 [P0 验证记录](docs/p0-validation.zh-CN.md)。另见 [规划](docs/launcher-plan.zh-CN.md)、[工程结构](docs/architecture.zh-CN.md)、[上游补丁](vendor/lighty/RTMCL-PATCHES.md)。

自有代码采用 [MIT 许可证](LICENSE)。Lighty 保留独立版权声明，详见 [第三方说明](THIRD_PARTY_NOTICES.md)。本项目非 Mojang 或 Microsoft 官方产品。

# P0 实现与验证记录

日期：2026-10-20（Asia/Hong_Kong）。版本查询、原版 / Fabric 安装、Java 与进程基础设施已实现。测试用离线登录已按用户最新要求移除；当前启动必须使用微软账户并成功在线认证。Windows / WSLg 曾用旧测试账户进入主菜单，这些为历史证据，不代表当前微软账户路径已验收。**P0 尚未全部验收**：API 接入后的完整正版会话，以及空缓存、官方断网的真实镜像安装仍待补充。

当前范围调整：按用户最新要求，从现在起暂缓 WSL / WSLg 验收，后续只运行 Windows 验收；本文件已有 Linux 记录保留为历史证据。用户已注册并提供 Client ID，恢复 Windows 微软 Device Code 联调；该流程不需要重定向 URL，配置见 [微软登录说明](microsoft-login.zh-CN.md)。真实登录及凭据刷新结果需另行记录，不能用已有代码代替验收。

本次 Windows 登录进度：用户已完成两次设备码授权（微软页面显示 RTMCL）。诊断版本确认 Microsoft OAuth → Xbox Live → XSTS 均成功，后续 `POST https://api.minecraftservices.com/authentication/login_with_xbox` 返回 HTTP 403。尚未取得 Minecraft 令牌或真实玩家资料，因此没有保存该账户的刷新凭据，也不能验收保存、轮换及重新启动后的刷新。

无用户令牌的独立可达性探测向该端点发送空 JSON，收到 HTTP 400 及缺少 identityToken 的 JSON 错误，说明该探测路径可以到达服务，但不证明认证请求一定不存在其他网络策略影响。当前响应证据仅能确定失败阶段与 HTTP 状态，不能把它直接写成已确认的 `Invalid app registration`。用户随后确认仅注册 Entra 应用，尚未申请 Minecraft API 接入；该接入状态成为下一项明确待办，当前不再重复要求用户授权。

认证阶段诊断及服务错误白名单均有脱敏回归；白名单兼容 `Invalid app registration` 带官方 AppRegInfo 链接的形式，但不回显完整响应正文。临时设备码和用户令牌不写入仓库文档。

通过浏览器实际读取 [官方审核说明](https://help.minecraft.net/hc/en-us/articles/16254801392141)，确认新应用需申请人工审核及加入允许列表；该页链接到 [官方 AppID Review 表单](https://aka.ms/mce-reviewappid)，表单已实际打开核实，未填写或提交。表单要求认证和游戏所有权校验。用户随后要求移除离线测试登录，应用当前已删除该路径；真实 API 接入、所有权及完整会话仍须复验，不能把代码变更视作审核通过。更多说明见 [微软登录说明](microsoft-login.zh-CN.md)。

## 依赖与补丁

- 工具链固定 Rust 1.98.0，Windows x64 与 Linux x64 均已实际构建。
- Lighty 固定 26.9.3，对应提交 `94bd0553beeb89fa7868eb0e0a9f446ad85af793`；九个发布包均下载后独立校验 SHA-256，并核对 `.cargo_vcs_info.json`、`Cargo.toml.orig` 与发布的 `src/` 文件。补丁前源码与指定提交匹配。
- 根包归档 SHA-256：`30ef7ff44ec32416e7ac300a6e073f447e5346db9420db9f841b7125de0e21df`。全部结果见 [发布包审计](lighty-release-audit.json)，复核工具见 [审核脚本](../scripts/audit-lighty.ps1)。
- 使用仓库内固定快照及通用补丁，未发布远端 fork 或声称上游已合入。MIT 许可保留，应用没有复制 rmcl 代码。
- 启用 vanilla、fabric、events；关闭上游 tracing。应用负责来源策略、下载执行、Java 与进程，复用 Lighty 元数据归一化和结构化参数生成。
- 已补下载上下文、显式目录初始化、库 OS 规则过滤和敏感参数日志等；逐文件边界见 [补丁说明](../vendor/lighty/RTMCL-PATCHES.md)。

来源：[固定上游提交](https://github.com/Lighty-Launcher/LightyLauncherLib/tree/94bd0553beeb89fa7868eb0e0a9f446ad85af793)、[crates.io 根包元数据](https://crates.io/api/v1/crates/lighty-launcher/26.9.3)。

## 本轮实现

| 路径 | 已实现行为 |
| --- | --- |
| 账户边界 | 仅微软账户；启动必须成功在线认证；旧测试记录不可选择，缺失凭据不允许启动 |
| 下载来源 | 官方 / BMCLAPI 严格模式、有序回退；精确主机及路径映射，未知来源不猜测；策略按任务快照固定 |
| 下载可靠性 | 同源 Range 续传、200/206/416 处理、有限重试、429 秒数 Retry-After、大小和 SHA1 校验、临时文件与原子提交 |
| 元数据 | 版本清单、版本 JSON、资产索引、Fabric profile 与 Maven 校验侧文件经过注入上下文；版本 JSON 校验发布 SHA1 |
| Java | 实际执行探测版本 / 架构，支持手动路径；自动获取 Adoptium 运行时，解压前校验 SHA256 |
| 安装 | 原版 / Fabric 元数据和所有安装文件处理；成功后才提交完成记录及 Ready；失败 / 取消不标记成功，可重跑恢复 |
| 实例隔离 | UUID 目录、平台归属校验、安装 / 启动跨进程操作锁、中文 / 空格路径与 Windows 深路径处理 |
| 游戏进程 | 独立监督持有锁、日志文件直连、CLI 退出后继续运行、显式停止并等待确认、记录退出状态 |

启动使用 `p0 launch <实例ID> --account <微软账户ID>`，前提是已成功完成真实认证；当前 Minecraft Services 403 未解决时不会启动游戏。完整命令见 [README](../README.md)。TUI 仍为最小壳，P0 操作通过 CLI。

## 历史真实游戏冒烟（测试登录已移除）

固定样本：Minecraft 1.21.1；Fabric Loader 0.16.10；历史测试玩家 P0Tester。四次主菜单都经过实际画面检查，所有测试游戏已退出。这些验证使用现已移除的测试登录，不能代替当前微软账户、所有权或完整正版会话验收。

| 平台 / 实例 | Java 与安装情况 | 主菜单及退出证据 |
| --- | --- | --- |
| Windows 原版 `4b72d264-d9d5-423c-8030-1769474450d0` | 手动 Microsoft JDK 21.0.11；先严格 BMCLAPI 下载，中断后使用官方来源续装并复用校验通过的文件 | [主菜单](evidence/windows-vanilla-menu.png)；正常 Quit，退出码 0 |
| Windows Fabric `dc0da2a7-f1cf-4d0a-a36f-db40bb4af394` | 独立实例；预置同版本已校验资产 / 库 / 客户端，严格 BMCLAPI 获取 Fabric 元数据及依赖并复核安装 | [Fabric 主菜单](evidence/windows-fabric-menu.png)；显式 `p0 stop`，退出码 1，锁释放 |
| Linux 原版 `b813aa1c-e82d-456c-b895-549c73fa86d3` | FedoraLinux-44 / WSL2 x86_64 / WSLg；严格 BMCLAPI，预置不可变文件后重新校验并获取 Linux 原生库；自动安装 Adoptium JRE 21 | [主菜单](evidence/linux-vanilla-menu.png)；正常 Quit，退出码 0 |
| Linux Fabric `09a8470f-82f7-4bab-94b7-851cee83aeec` | 同一 Linux 环境、独立实例；严格 BMCLAPI 获取 Fabric 与 Linux 所需文件；使用托管 JRE 21 | [Fabric 主菜单](evidence/linux-fabric-menu.png)；正常 Quit，退出码 0 |

Linux 自动 Java 的实际归档大小为 52,059,408 字节，下载后通过 SHA256。实测发现并修复 DrvFs 上“先执行暂存运行时导致目录重命名失败”的问题，改为持锁提交目录后探测。Windows 实测发现深路径临时文件提交失败，已统一规范化临时文件和目标父目录，并增加回归测试。

上述镜像测试包含缓存预置，Windows 原版也曾切到官方完成资产下载；**不能将其写成干净用户目录、官方完全不可达条件下的完整 BMCLAPI 验收**。WSLg 也不能代替原生 Linux 桌面发布验收。

## 自动检查

```text
cargo fmt --package launcher-app --package launcher-terminal -- --check
cargo clippy --workspace --all-targets --no-deps --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
cargo run --locked -- doctor
```

当前只新增 Windows 检查，WSL / WSLg 结果保留为历史证据。应用格式检查限定两个自有 crate，避免重排第三方快照；上游关闭 tracing 时存在 unused/dead-code 警告，自有 crate 的 Clippy 拒绝警告。GitHub CI 配置覆盖 Windows / Ubuntu，远端结果通过仓库 Actions 查看，不等同于 WSL 验收。

测试覆盖以下行为：

- 旧测试账户不可选、同文件微软资料仍可读取、未来 schema / 未知类型拒绝且不改写文件、微软资料退出清理；缺失会话凭据时在请求网络前拒绝认证；移除的 CLI 命令和参数被拒绝，启动强制提供账户。模拟令牌轮换及会话模式不落明文。认证诊断仅保留固定阶段与白名单提示，不回显响应正文或秘密。模拟凭据不代表真实系统凭据验证。
- 精确 URL 映射、编码和查询保留、严格来源拒绝未知资源；元数据缓存损坏后重取。
- 206 续传、200 重写、416 重启、302 重定向、错误哈希不提交、切源不续接旧文件、429 与取消、官方候选不可达后回退。
- `p0_mirror` 集成测试调用真实 Lighty 和 P0 安装器，在空临时目录安装微型原版 / Fabric。所有夹具 URL 指向官方域名，由严格策略转入本地模拟镜像；检查版本 JSON、资产索引 / 对象、普通库 / 原生库、客户端、日志、Fabric profile / Maven / SHA1 侧文件均到达模拟服务，并确认其他 OS 库未下载。它证明该夹具下的调用链覆盖，不替代外网镜像服务验收。
- 配置保护、中文 / 空格路径、实例隔离、跨进程锁、取消状态与终态快照、Windows 深路径。
- 独立监督进程测试：交互端句柄关闭后子进程仍运行且实例仍被锁定；显式停止后确认退出、锁释放及结果记录。另有终端渲染及 CLI 测试。

测试数量与最终检查结果见本文件末尾。原始测试日志及下载 / 游戏日志位于忽略目录 `.rtmcl/`，不作为发布源文件提交；主菜单截图、审核记录和可重复运行的测试保留在仓库。

## 尚待验收与限制

1. 官方资源端点被阻断、空缓存条件下，使用真实 BMCLAPI 完整安装并启动原版 / Fabric；需要网络阻断证据。当前有本地故障测试、模拟镜像完整安装和真实镜像含缓存实测，证据层次分别记录。
2. 微软正版登录已实际通过 Microsoft、Xbox Live、XSTS，Minecraft Services 令牌交换返回 403。离线测试登录已删除，下一步完成 API 接入，并重新验收真实所有权、完整正版启动、系统凭据保存 / 刷新 / 退出清理。
3. 补充原版 1.20.1 与当前最新稳定版、Windows 托管 Java ZIP，以及原生 Linux 桌面冒烟。已测固定样本为 1.21.1。
4. 条件请求、ETag / Last-Modified、全局内容缓存、HTTP-date Retry-After、全主机冷却和重定向后的独立主机调度未完成。当前进度为限频的当前资源字节数，尚非全安装聚合进度。
5. 对镜像改写的有哈希版本 JSON，当前校验失败即报错 / 按配置回退；尚无专门的重写元数据兼容适配，绝不静默忽略校验。
6. 外部强杀监督进程后的孤儿游戏及陈旧标记恢复仍需增强；普通 CLI 退出和显式停止已测。严格 BMCLAPI 尚不提供托管 Java 来源，需预备 Java 或允许官方 Java 下载。
7. Lighty 每进程只能初始化一个数据根目录；不同根目录必须使用独立进程。完整 TUI、更多加载器和内容平台仍属后续阶段。WSL / WSLg 验收暂缓，当前 P0 不再以新增 WSL 验收为门槛。

当前离线测试登录已移除，API 未放行且无法取得玩家资料时，游戏启动会停止。完整 P0 验收须继续补齐上述未暂缓项。

## 最终检查结果

| 环境 | 格式 / Clippy / 构建 | 自动测试 | doctor |
| --- | --- | --- | --- |
| Windows x64，Rust 1.98.0 | 通过 | 36 项通过，0 失败 | 通过 |
| FedoraLinux-44 / WSL2 x64，Rust 1.98.0（暂缓前历史结果） | 通过 | 32 项通过，0 失败 | 通过 |

范围调整前为 Windows 33 项、Linux 32 项，差异来自 Windows 专用深路径回归。新增认证诊断回归后，仅在 Windows 重新运行全部 34 项测试、Clippy 和构建并通过；没有恢复 WSL 验收。此次日志为 `.rtmcl/p0-auth-diagnostics-test.log`、`.rtmcl/p0-auth-diagnostics-clippy.log` 与 `.rtmcl/p0-auth-diagnostics-build.log`。

移除离线测试登录后，替换旧账户测试并增加缺失凭据拒绝与 CLI 边界回归，Windows 全部 36 项测试、Clippy 和构建通过；格式检查通过。日志为 `.rtmcl/remove-test-auth-tests.log`、`.rtmcl/remove-test-auth-clippy.log` 与 `.rtmcl/remove-test-auth-build.log`。该变更没有新增 WSL 检查，Linux 行继续保留原始历史结果。

Linux 历史检查的最后一条诊断命令最初被本地验证脚本尾部 CRLF 影响；当时已修正脚本并独立复验，构建和测试本身均通过。历史日志为 `.rtmcl/p0-final-test-windows.log`、`.rtmcl/p0-final-clippy-windows.log`、`.rtmcl/p0-final-linux.log`、`.rtmcl/p0-final-cli-windows.log` 与 `.rtmcl/p0-final-cli-linux.log`。

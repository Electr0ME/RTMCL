# 工程结构与 P0 边界

依赖方向保持 `launcher-terminal → launcher-app → Lighty / 基础设施`。终端只使用应用类型，不引用 Lighty，不接触账户令牌。

```text
crates/launcher-app/src/
  domain.rs       稳定 ID、实例、加载器、Java 与启动配置
  config.rs       schema、来源策略与配置验证
  paths.rs        系统目录与数据根目录覆盖
  storage.rs      同目录临时文件、原子提交与跨进程文件锁
  service.rs      实例和配置服务
  tasks.rs        任务状态机、权威快照、取消与清理确认
  accounts.rs     微软账户、凭据分离、刷新轮换、旧测试账户排除
  network.rs      精确来源映射、下载、续传、校验、元数据缓存
  java.rs         实际探测、手动选择、托管 Java 校验及提交
  p0.rs           安装、启动描述、进程监督与退出记录
  core/mod.rs     LightyAdapter 与应用核心端口
  downloads.rs    Provider / Router / Executor / Metadata 端口
  ports.rs        账户、凭据、Java 与进程的后续服务契约
crates/launcher-terminal/src/
  cli.rs          Clap 命令
  p0_cli.rs       P0 与账户命令、参数及任务取消
  main.rs         Tokio 入口与文件日志
  tui.rs          最小实例列表和详情；完整页面留在 P2
vendor/lighty/    固定上游源码及通用补丁
scripts/         发布包审核脚本
```

Lighty 26.9.3 的九个发布包已独立校验并与规划提交比对；应用固定 Rust 1.98.0，提交 Cargo.lock。补丁及来源见 [RTMCL-PATCHES](../vendor/lighty/RTMCL-PATCHES.md)。消费者注入下载上下文，BMCLAPI 策略留在应用中。

P0 复用 Lighty 的原版 / Fabric 元数据归一化及参数生成。应用控制资源下载、安装完成记录、Java 和进程，不调用高层 `run()`。Lighty 路径每进程只初始化一次，不同根目录用独立进程；内存元数据缓存 TTL 置零，避免跨任务混用来源策略，应用文件缓存独立管理。

实例使用 UUID 目录，记录 OS/架构归属。安装与游戏监督持有操作锁。成功提交 installation.json 后才写 Ready，失败或取消写 Failed；强制中断留下的 Installing 可再次安装恢复。每实例启动设置可整组覆盖全局值。

已知官方主机分别映射到 BMCLAPI assets、maven、fabric-meta 等路径，未知主机不猜测。严格来源无隐式官方回退。Java 镜像覆盖独立，当前托管 Java 使用 Adoptium 官方 API/归档。

同源临时文件保存身份，有预期 SHA1 时允许续传；206 验证 Content-Range，200 完整重写，416 重新下载，切源丢弃旧部分文件。传输、大小与预期 SHA1 校验后原子提交。无发布哈希的元数据采用完整 HTTP 传输，记录观察大小和摘要，不能等同于发布者哈希。中间进度限频 100ms，目前表示当前资源而非整个安装的总字节进度。

`.verified.toml` 记录原始/候选请求地址、来源、抓取时间、大小及摘要。可变元数据 TTL 为 5 分钟，有预期哈希的内容可复用。条件请求、ETag/Last-Modified、全局内容缓存、重定向后的独立主机调度仍待补齐。429 当前支持秒数形式 Retry-After（最长 60 秒），尚未覆盖 HTTP-date。

测试用离线登录已移除：应用只接受微软账户，CLI 启动必须提供账户 ID；每次启动都经过在线刷新、Xbox / XSTS / Minecraft Services 及玩家资料获取，失败即停止。旧版 kind=local 记录在只读加载时排除，不会成为可选账户，也不会阻断同文件中的微软账户；未知类型和未来 schema 仍报错。读取不改写原文件，后续正常保存只包含支持的微软账户。

微软认证保持独立 HTTP 路径，使用本项目 Client ID 和 consumers 端点；Device Code 不需要 redirect_uri。刷新令牌使用系统凭据服务，失败时明确报告 SessionOnly，不降级到明文文件。`login --launch` 支持同一 CLI 会话登录并启动。已有入口不代表完成真实登录、系统凭据和刷新验收；当前 Minecraft Services 403 仍是外部接入待办。应用配置见 [微软登录说明](microsoft-login.zh-CN.md)。

敏感启动描述只通过私有 stdin 管道传给独立监督进程，不落盘、不 Debug。监督进程先取实例锁，再启动游戏；stdout/stderr 直接写日志。普通 CLI 退出或停止等待不关闭游戏，`p0 stop` 按会话 ID 请求终止并等待确认。退出结果写入 last-run.json。外部强杀监督进程后的恢复仍需增强。

认证通过每次调用独立的 Lighty 事件总线识别最后阶段，只接受固定的阶段标签，不输出失败事件原文。HTTP 错误只显示阶段、状态码与明确白名单提示，不透传任意服务响应；浏览器授权完成不等于已完成 Xbox / Minecraft 后续认证。

下载使用流式异步 I/O，任务 watch 保留终态快照。取消后等待活跃下载与清理。TUI 仍为最小壳，P0 操作集中在 CLI。当前暂停 WSL / WSLg 验收，不再启动这些环境执行检查；保留已有 Linux 实现、CI 和历史实测证据。证据及未通过项见 [P0 记录](p0-validation.zh-CN.md)。

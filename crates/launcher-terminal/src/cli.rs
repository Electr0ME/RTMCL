use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use launcher_app::domain::{AccountId, InstanceId};

#[derive(Debug, Parser)]
#[command(
    name = "rtmcl",
    version,
    about = "Rust TUI Minecraft Launcher · 项目框架 / P0 验证入口"
)]
pub struct Cli {
    /// 覆盖所有应用目录（也可设置 RTMCL_DATA_DIR）
    #[arg(long, global = true, env = "RTMCL_DATA_DIR")]
    pub data_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 微软 Device Code 登录与账户管理
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// 核心技术验证：来源、安装、Java 与实际启动
    P0 {
        /// 本次操作使用的来源；不修改全局配置
        #[arg(long, value_enum)]
        source: Option<SourceChoice>,
        #[command(subcommand)]
        command: P0Command,
    },
    #[command(name = "__supervise", hide = true)]
    Supervise,
    /// 显示框架与 Lighty 集成状态，不修改文件
    Doctor,
    /// 显示实际数据路径
    Paths,
    /// 查看或初始化全局配置
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// 管理本地实例配置；尚不下载或安装游戏
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },
    /// 打开最小 TUI 框架（无子命令时默认）
    Tui,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SourceChoice {
    Official,
    Bmclapi,
    MirrorFirst,
    OfficialFirst,
}

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    List,
    /// 登录微软账户；系统凭据不可用时明确使用本次会话
    Login {
        #[arg(long, env = "RTMCL_MICROSOFT_CLIENT_ID")]
        client_id: String,
        #[arg(long)]
        session_only: bool,
        /// 登录后在同一进程中启动实例，支持仅本次会话的凭据
        #[arg(long)]
        launch: Option<InstanceId>,
        #[arg(long, requires = "launch")]
        java: Option<PathBuf>,
        #[arg(long, requires = "launch")]
        wait: bool,
    },
    /// 清理本地账户资料和系统刷新令牌
    Logout {
        account: AccountId,
    },
}

#[derive(Debug, Subcommand)]
pub enum P0Command {
    Versions {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Install {
        instance: InstanceId,
    },
    Launch {
        instance: InstanceId,
        #[arg(long)]
        account: AccountId,
        #[arg(long)]
        java: Option<PathBuf>,
        /// 等待退出并报告退出码；Ctrl+C 只关闭等待，游戏继续运行
        #[arg(long)]
        wait: bool,
    },
    /// 显式终止游戏并等待监督进程确认
    Stop {
        instance: InstanceId,
    },
    Java {
        executable: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// 原子写入默认配置；已存在时保留
    Init,
    /// 显示已有配置，不存在时显示框架默认值
    Show,
}

#[derive(Debug, Subcommand)]
pub enum InstanceCommand {
    List,
    Create(CreateInstance),
}

#[derive(Debug, Args)]
pub struct CreateInstance {
    pub name: String,
    #[arg(long)]
    pub game_version: String,
    /// 指定则创建 Fabric 实例，必须给出确定的加载器版本
    #[arg(long)]
    pub fabric_version: Option<String>,
}

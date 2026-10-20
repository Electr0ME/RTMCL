mod cli;
mod p0_cli;
mod tui;

use std::{error::Error, io::IsTerminal, process::ExitCode};

use clap::Parser;
use launcher_app::{
    AppPaths, LauncherService,
    core::{LIGHTY_REVISION, LIGHTY_RUST_VERSION, LIGHTY_VERSION},
    domain::Loader,
};

use cli::{Cli, Command, ConfigCommand, InstanceCommand};

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("错误: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn Error + Send + Sync>> {
    if launcher_app::accounts::DEVELOPMENT_LOCAL_ACCOUNTS
        && !matches!(&cli.command, Some(Command::Supervise))
    {
        eprintln!("开发构建：未绑定的本地账户仅用于本机测试；此构建不得作为发行版本。");
    }
    let paths = AppPaths::discover(cli.data_dir.as_deref())?;
    let service = LauncherService::new(paths.clone());
    match cli.command.unwrap_or(Command::Tui) {
        Command::Supervise => launcher_app::p0::supervise().await?,
        Command::P0 { source, command } => p0_cli::run(service, source, command).await?,
        Command::Account { command } => p0_cli::accounts(service, command).await?,
        Command::Doctor => {
            println!(
                "RTMCL {} · P0 技术验证中，尚未全部验收",
                env!("CARGO_PKG_VERSION")
            );
            println!("平台: {}", launcher_app::domain::current_platform());
            println!("工具链: Rust 1.98.0（rust-toolchain.toml）");
            println!("Lighty 核心: {LIGHTY_VERSION} / {LIGHTY_REVISION}");
            println!("Lighty 最低 Rust: {LIGHTY_RUST_VERSION}；已链接固定源码与本地补丁");
            println!("可用: 配置初始化、实例配置创建/列表、最小 TUI");
            println!(
                "P0 CLI: 版本查询、安装、Java、账户与监督进程；正版登录 Windows 联调中，WSL 验收暂缓"
            );
            println!("项目状态与使用说明: README.zh-CN.md");
            println!(
                "本地账户开发例外: {}",
                launcher_app::accounts::DEVELOPMENT_LOCAL_ACCOUNTS
            );
        }
        Command::Paths => {
            println!("config    {}", paths.config.display());
            println!("instances {}", paths.instances.display());
            println!("cache     {}", paths.cache.display());
            println!("runtimes  {}", paths.runtimes.display());
            println!("logs      {}", paths.logs.display());
        }
        Command::Config { command } => {
            let config = tokio::task::spawn_blocking(move || match command {
                ConfigCommand::Init => service.initialize(),
                ConfigCommand::Show => service.settings(),
            })
            .await??;
            // No account credentials are part of the configuration model.
            println!("schema_version = {}", config.schema_version);
            println!("downloads = {:?}", config.downloads);
            println!("bmclapi = {}", config.mirrors.bmclapi.base_url);
            println!("配置文件: {}", paths.settings_file().display());
        }
        Command::Instance { command } => match command {
            InstanceCommand::Logs { instance, lines } => {
                let log = tokio::task::spawn_blocking(move || {
                    service.game_log_tail(instance, lines as usize)
                })
                .await??;
                eprintln!("游戏日志: {}", log.path.display());
                if log.byte_limit_reached {
                    eprintln!("日志片段已限制为末尾 1 MiB，可能不足请求的行数。");
                }
                // Game/mod output is untrusted terminal text. Keep line layout, strip controls.
                let text: String = log
                    .text
                    .chars()
                    .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                    .collect();
                print!("{text}");
            }
            InstanceCommand::List => {
                let instances =
                    tokio::task::spawn_blocking(move || service.list_instances()).await??;
                if instances.is_empty() {
                    println!("暂无实例，使用 instance create 创建配置。");
                }
                for instance in instances {
                    println!(
                        "{}  {}  {}  {:?}  {:?}",
                        instance.id,
                        instance.name,
                        instance.game_version,
                        instance.loader,
                        instance.install_state
                    );
                }
            }
            InstanceCommand::Create(args) => {
                let instance = tokio::task::spawn_blocking(move || {
                    let loader = args
                        .fabric_version
                        .map_or(Loader::Vanilla, |version| Loader::Fabric { version });
                    service.create_instance(args.name, args.game_version, loader)
                })
                .await??;
                println!(
                    "已创建实例配置: {} ({})；游戏尚未安装。",
                    instance.name, instance.id
                );
            }
        },
        Command::Tui => {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                return Err("TUI 需要交互式终端；请使用 doctor、paths 或 instance list".into());
            }
            let instances = tokio::task::spawn_blocking(move || {
                service.settings()?;
                service.list_instances()
            })
            .await??;
            // File logs avoid corrupting the TUI screen. Never log tokens or launch arguments.
            let log_paths = paths.clone();
            let log_file = tokio::task::spawn_blocking(move || {
                std::fs::create_dir_all(&log_paths.logs)?;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_paths.logs.join("launcher.log"))
            })
            .await??;
            tracing_subscriber::fmt()
                .with_ansi(false)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".into()),
                )
                .with_writer(log_file)
                .try_init()?;
            tracing::info!("打开 TUI 框架");
            tui::run(instances).await?;
        }
    }
    Ok(())
}

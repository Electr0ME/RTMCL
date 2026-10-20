use std::error::Error;

use launcher_app::{
    AppError, LauncherService,
    accounts::Accounts,
    config::{AppConfig, SourceMode},
    downloads::SourceId,
    network::Network,
    p0::{self, LaunchOptions},
    tasks::{TaskContext, TaskHandle, TaskOutcome},
};

use crate::cli::{AccountCommand, P0Command, SourceChoice};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub async fn accounts(service: LauncherService, command: AccountCommand) -> Result<()> {
    let accounts = Accounts::new(service.paths().clone());
    match command {
        AccountCommand::List => {
            for record in tokio::task::spawn_blocking(move || accounts.list()).await?? {
                println!(
                    "{}  {}  {:?}  UUID={}",
                    record.id, record.username, record.provider, record.minecraft_uuid
                );
            }
        }
        AccountCommand::Logout { account } => {
            tokio::task::spawn_blocking(move || accounts.remove(account)).await??;
            println!("账户资料及可用的本地凭据已清理。");
        }
        AccountCommand::Login {
            client_id,
            session_only,
            launch,
            java,
            wait,
        } => {
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let cancellation = tokio_util_token();
            let cancel = cancellation.clone();
            let future = accounts.login_microsoft(client_id, session_only, sender, cancellation);
            tokio::pin!(future);
            let record = loop {
                tokio::select! {
                    result = &mut future => break result?,
                    prompt = receiver.recv() => if let Some((code, url)) = prompt { println!("打开 {url}\n输入设备码: {code}"); },
                    signal = tokio::signal::ctrl_c() => { signal?; cancel.cancel(); }
                }
            };
            println!(
                "已登录: {} ({}) · {:?}",
                record.username, record.id, record.provider
            );
            if matches!(
                record.provider,
                launcher_app::accounts::AccountKind::Microsoft {
                    persistence: launcher_app::ports::CredentialPersistence::SessionOnly,
                    ..
                }
            ) {
                println!(
                    "凭据仅保存在当前会话，CLI 退出后失效；可用 login --launch 在本会话启动，未写入明文文件。"
                );
            }
            if let Some(instance) = launch {
                let (handle, task) = TaskHandle::new(Some(instance));
                task.start()?;
                let network =
                    Network::new(service.paths().clone(), service.settings()?, task.clone())?;
                let java = java
                    .map(|path| {
                        if path.is_absolute() {
                            Ok(path)
                        } else {
                            std::env::current_dir().map(|cwd| cwd.join(path))
                        }
                    })
                    .transpose()?;
                let future = p0::launch(
                    service.clone(),
                    instance,
                    accounts.clone(),
                    LaunchOptions {
                        account: record.id,
                        java,
                        wait,
                    },
                    network,
                    task.clone(),
                );
                tokio::pin!(future);
                let result = loop {
                    tokio::select! {
                        result = &mut future => break result,
                        signal = tokio::signal::ctrl_c() => { signal?; handle.request_cancel(); }
                    }
                }?;
                task.finish(TaskOutcome::Succeeded)?;
                println!(
                    "PID={} · 日志: {} · 退出码 {:?}",
                    result.process_id,
                    result.log_file.display(),
                    result.exit_code
                );
            }
        }
    }
    Ok(())
}

// Keeps cancellation types within the existing application/task dependency boundary.
fn tokio_util_token() -> launcher_app::tasks::Cancellation {
    launcher_app::tasks::Cancellation::new()
}

pub async fn run(
    service: LauncherService,
    source: Option<SourceChoice>,
    command: P0Command,
) -> Result<()> {
    if let P0Command::Stop { instance } = command {
        p0::stop(&service, instance).await?;
        println!("已确认游戏退出。");
        return Ok(());
    }
    if let P0Command::Java { executable } = command {
        let runtime = launcher_app::java::inspect(&executable).await?;
        println!(
            "Java {} / {} · {}",
            runtime.major_version,
            runtime.architecture,
            runtime.executable.display()
        );
        return Ok(());
    }
    let settings_service = service.clone();
    let mut settings = tokio::task::spawn_blocking(move || settings_service.settings()).await??;
    override_source(&mut settings, source);
    let instance_id = match &command {
        P0Command::Install { instance } | P0Command::Launch { instance, .. } => Some(*instance),
        _ => None,
    };
    let (handle, task) = TaskHandle::new(instance_id);
    task.start()?;
    let network = Network::new(service.paths().clone(), settings, task.clone())?;
    let operation = execute(service, command, network, task.clone());
    tokio::pin!(operation);
    let mut progress = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut last_progress = None;
    let result = loop {
        tokio::select! {
            result = &mut operation => break result,
            _ = progress.tick() => {
                let snapshot = handle.snapshot();
                let current = (snapshot.progress.source, snapshot.progress.downloaded_bytes, snapshot.progress.total_bytes);
                if last_progress != Some(current) {
                    if let Some(source) = snapshot.progress.source { eprintln!("下载来源 {source:?} · {} / {} 字节", snapshot.progress.downloaded_bytes, snapshot.progress.total_bytes.map_or_else(|| "?".into(), |n| n.to_string())); }
                    last_progress = Some(current);
                }
            }
            signal = tokio::signal::ctrl_c() => { signal?; handle.request_cancel(); eprintln!("正在取消操作；安装清理完成后返回。已运行游戏由监督进程继续管理。"); }
        }
    };
    match &result {
        Ok(()) => task.finish(TaskOutcome::Succeeded)?,
        Err(error) if matches!(error.downcast_ref::<AppError>(), Some(AppError::Cancelled)) => {
            task.finish(TaskOutcome::Cancelled)?
        }
        Err(error) => task.finish(TaskOutcome::Failed(launcher_app::tasks::TaskFailure {
            stage: launcher_app::tasks::TaskStage::Install,
            resource: None,
            source: None,
            retryable: false,
            cause: error.to_string(),
        }))?,
    }
    result
}

fn override_source(settings: &mut AppConfig, source: Option<SourceChoice>) {
    if let Some(source) = source {
        let (mode, sources) = match source {
            SourceChoice::Official => (SourceMode::Strict, vec![SourceId::Official]),
            SourceChoice::Bmclapi => (SourceMode::Strict, vec![SourceId::Bmclapi]),
            SourceChoice::MirrorFirst => (
                SourceMode::Ordered,
                vec![SourceId::Bmclapi, SourceId::Official],
            ),
            SourceChoice::OfficialFirst => (
                SourceMode::Ordered,
                vec![SourceId::Official, SourceId::Bmclapi],
            ),
        };
        settings.downloads.mode = mode;
        settings.downloads.sources = sources;
    }
}

async fn execute(
    service: LauncherService,
    command: P0Command,
    network: Network,
    task: TaskContext,
) -> Result<()> {
    match command {
        P0Command::Versions { limit } => {
            for version in p0::versions(&network).await?.into_iter().take(limit) {
                println!("{}  {}", version.id, version.kind);
            }
        }
        P0Command::Install { instance } => {
            p0::install(service, instance, network, task).await?;
            println!("安装完成，所有必需资源已校验。");
        }
        P0Command::Launch {
            instance,
            account,
            java,
            wait,
        } => {
            let accounts = Accounts::new(service.paths().clone());
            let java = java
                .map(|path| {
                    if path.is_absolute() {
                        Ok(path)
                    } else {
                        std::env::current_dir().map(|cwd| cwd.join(path))
                    }
                })
                .transpose()?;
            let launched = p0::launch(
                service,
                instance,
                accounts,
                LaunchOptions {
                    account,
                    java,
                    wait,
                },
                network,
                task,
            )
            .await?;
            println!(
                "PID={} · 日志: {} · 退出码: {:?}",
                launched.process_id,
                launched.log_file.display(),
                launched.exit_code
            );
            if launched.success == Some(false) {
                return Err(format!(
                    "游戏异常退出，退出码 {:?}（Unix 信号终止可能无数字退出码）",
                    launched.exit_code
                )
                .into());
            }
        }
        P0Command::Stop { .. } | P0Command::Java { .. } => unreachable!(),
    }
    Ok(())
}

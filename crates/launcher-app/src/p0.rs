//! P0 orchestration: Lighty resolves loaders and arguments; application owns I/O/processes.
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use futures_util::{StreamExt, stream};
use lighty_launcher::{
    core::AppState,
    launch::LaunchArguments,
    loaders::{LoaderExtensions, VersionMetaData, vanilla::vanilla::VANILLA},
    version::VersionBuilder,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use crate::{
    AppError, LauncherService, Result,
    accounts::Accounts,
    domain::{
        AccountId, InstallState, InstanceConfig, InstanceId, JavaPolicy, Loader, current_platform,
    },
    error::io_error,
    java,
    network::{MANIFEST_URL, Network, resource},
    storage::{FileLock, write_bytes},
    tasks::TaskContext,
};

type Version = lighty_launcher::loaders::Version;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Serialize, Deserialize)]
struct Installed {
    schema_version: u32,
    instance: InstanceId,
    game_version: String,
    loader: Loader,
    platform: String,
    metadata: Version,
    logging_argument: Option<String>,
}

static INIT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn builder(
    service: &LauncherService,
    instance: &InstanceConfig,
) -> Result<VersionBuilder<lighty_launcher::Loader>> {
    let mut initialized = INIT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| AppError::Core("核心目录锁失效".into()))?;
    if let Some(path) = initialized.as_ref() {
        if path != &service.paths().instances {
            return Err(AppError::Core(
                "Lighty 每进程只能绑定一个数据根目录；请使用独立进程".into(),
            ));
        }
    } else {
        AppState::init_with_paths(lighty_launcher::_core::app_state::LauncherPaths {
            name: "RTMCL".into(),
            data_dir: service.paths().instances.clone(),
            config_dir: service.paths().config.clone(),
            cache_dir: service.paths().cache.clone(),
        })
        .map_err(|e| AppError::Core(e.to_string()))?;
        *initialized = Some(service.paths().instances.clone());
    }
    let (loader, loader_version) = match &instance.loader {
        Loader::Vanilla => (lighty_launcher::Loader::Vanilla, ""),
        Loader::Fabric { version } => (lighty_launcher::Loader::Fabric, version.as_str()),
    };
    let mut builder = VersionBuilder::new(
        &instance.id.to_string(),
        loader,
        loader_version,
        &instance.game_version,
    )
    .with_ttl_duration(Duration::ZERO);
    builder.game_dirs = service.paths().instance_dir(instance.id).join("minecraft");
    builder.runtime_dir = builder.game_dirs.clone();
    builder.java_dirs = service.paths().runtimes.clone();
    Ok(builder)
}

pub async fn versions(network: &Network) -> Result<Vec<VersionSummary>> {
    #[derive(Deserialize)]
    struct Manifest {
        versions: Vec<VersionSummary>,
    }
    Ok(serde_json::from_slice::<Manifest>(&network.fetch(MANIFEST_URL, None).await?)?.versions)
}

pub async fn install(
    service: LauncherService,
    id: InstanceId,
    network: Network,
    task: TaskContext,
) -> Result<()> {
    let dir = service.paths().instance_dir(id);
    let _lock = FileLock::acquire(&dir.join("operation.lock"))?;
    let mut instance = service.instance(id)?;
    instance.install_state = InstallState::Installing;
    service.save_instance(&instance)?;
    let mut result = install_inner(&service, &instance, &network, task.clone()).await;
    if task.child_token().is_cancelled() {
        result = Err(AppError::Cancelled);
    }
    instance.install_state = if result.is_ok() {
        InstallState::Ready
    } else {
        InstallState::Failed
    };
    service.save_instance(&instance)?;
    result
}

async fn install_inner(
    service: &LauncherService,
    instance: &InstanceConfig,
    network: &Network,
    task: TaskContext,
) -> Result<()> {
    let version = builder(service, instance)?;
    let cancellation = task.child_token();
    let context: Arc<dyn lighty_launcher::_core::download_context::DownloadContext> =
        Arc::new(network.clone());
    let (mut metadata, logging) = lighty_launcher::_core::download_context::scope(context, async {
        let metadata = version
            .get_metadata()
            .await
            .map_err(|e| AppError::Core(e.to_string()))?;
        let metadata = match metadata.as_ref() {
            VersionMetaData::Version(version) => version.clone(),
            _ => return Err(AppError::Core("Lighty 返回了不完整元数据".into())),
        };
        let raw = VANILLA
            .get_raw(&version)
            .await
            .map_err(|e| AppError::Core(e.to_string()))?;
        // Serialize the logging descriptor rather than leaking upstream raw types.
        let logging = raw
            .logging
            .as_ref()
            .and_then(|logging| logging.client.as_ref())
            .map(|entry| {
                (
                    entry.argument.clone(),
                    entry.file.id.clone(),
                    entry.file.url.clone(),
                    entry.file.sha1.clone(),
                    entry.file.size,
                )
            });
        Ok::<_, AppError>((metadata, logging))
    })
    .await?;
    if cancellation.is_cancelled() {
        return Err(AppError::Cancelled);
    }
    let game = &version.game_dirs;
    let mut files: Vec<(crate::downloads::ResourceRequest, PathBuf)> = Vec::new();
    for library in &metadata.libraries {
        if let (Some(url), Some(path)) = (&library.url, &library.path) {
            files.push((
                resource(url, library.sha1.as_deref(), library.size)?,
                safe_join(&game.join("libraries"), path)?,
            ));
        }
    }
    // Modern LWJGL loads natives from the classpath as well as from extracted DLL/SO files.
    if let Some(natives) = &metadata.natives {
        for native in natives {
            if let (Some(url), Some(path)) = (&native.url, &native.path) {
                files.push((
                    resource(url, native.sha1.as_deref(), native.size)?,
                    safe_join(&game.join("libraries"), path)?,
                ));
                if !metadata
                    .libraries
                    .iter()
                    .any(|library| library.path == native.path)
                {
                    metadata.libraries.push(lighty_launcher::loaders::Library {
                        name: native.name.clone(),
                        url: native.url.clone(),
                        path: native.path.clone(),
                        sha1: native.sha1.clone(),
                        size: native.size,
                    });
                }
            }
        }
    }
    let client = metadata
        .client
        .as_ref()
        .ok_or_else(|| AppError::Core("缺少客户端 JAR".into()))?;
    files.push((
        resource(
            client
                .url
                .as_deref()
                .ok_or_else(|| AppError::Core("客户端地址缺失".into()))?,
            client.sha1.as_deref(),
            client.size,
        )?,
        game.join(format!("{}.jar", instance.id)),
    ));
    if let Some(assets) = &metadata.assets {
        for asset in assets.objects.values() {
            if asset.hash.len() != 40 || !asset.hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(AppError::Core("资产 hash 无效".into()));
            }
            let path = game
                .join("assets/objects")
                .join(&asset.hash[..2])
                .join(&asset.hash);
            let url = asset.url.clone().unwrap_or_else(|| {
                format!(
                    "https://resources.download.minecraft.net/{}/{}",
                    &asset.hash[..2],
                    asset.hash
                )
            });
            files.push((resource(&url, Some(&asset.hash), Some(asset.size))?, path));
        }
    }
    let logging_argument = if let Some((argument, name, url, hash, size)) = logging {
        let path = safe_join(&game.join("assets/log_configs"), &name)?;
        files.push((resource(&url, Some(&hash), Some(size))?, path.clone()));
        Some(argument.replace("${path}", &path.to_string_lossy()))
    } else {
        None
    };
    // Same resource may be referenced by several asset names; never race its cache lock.
    let mut seen = HashSet::new();
    files.retain(|(_, path)| seen.insert(path.clone()));
    let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let results = stream::iter(files.into_iter().map(|(request, target)| {
        let network = network.clone();
        let failed = failed.clone();
        async move {
            if failed.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(());
            }
            let result = network.download(&request, &target).await.map(|_| ());
            if result.is_err() {
                failed.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            result
        }
    }))
    .buffer_unordered(16)
    .collect::<Vec<_>>()
    .await;
    // Drain active futures before returning an error, so file writers release their locks.
    for result in results {
        result?;
    }
    let natives_dir = game.join("natives");
    tokio::fs::create_dir_all(&natives_dir)
        .await
        .map_err(|e| io_error("创建原生库目录", &natives_dir, e))?;
    if let Some(natives) = &metadata.natives {
        for native in natives {
            if cancellation.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            if let Some(path) = &native.path {
                let jar = safe_join(&game.join("libraries"), path)?;
                let reader = tokio::io::BufReader::new(
                    tokio::fs::File::open(&jar)
                        .await
                        .map_err(|e| io_error("打开原生库 JAR", &jar, e))?,
                );
                lighty_launcher::core::extract::zip_extract(reader, &natives_dir, None)
                    .await
                    .map_err(|e| AppError::Core(e.to_string()))?;
            }
        }
    }
    if cancellation.is_cancelled() {
        return Err(AppError::Cancelled);
    }
    let installed = Installed {
        schema_version: 1,
        instance: instance.id,
        game_version: instance.game_version.clone(),
        loader: instance.loader.clone(),
        platform: current_platform(),
        metadata,
        logging_argument,
    };
    let path = service
        .paths()
        .instance_dir(instance.id)
        .join("installation.json");
    write_bytes(&path, &serde_json::to_vec(&installed)?)?;
    Ok(())
}

pub struct LaunchOptions {
    pub account: AccountId,
    pub java: Option<PathBuf>,
    pub wait: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LaunchResult {
    pub process_id: u32,
    pub log_file: PathBuf,
    pub exit_code: Option<i32>,
    pub success: Option<bool>,
}

// Contains sensitive argv. Sent only through a private pipe; never Debug or saved to disk.
#[derive(Serialize, Deserialize)]
struct ProcessSpec {
    paths: crate::AppPaths,
    instance: InstanceId,
    executable: PathBuf,
    arguments: Vec<String>,
    working_dir: PathBuf,
    log_file: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Running {
    schema_version: u32,
    session: uuid::Uuid,
    process_id: u32,
}

pub async fn launch(
    service: LauncherService,
    id: InstanceId,
    accounts: Accounts,
    options: LaunchOptions,
    network: Network,
    task: TaskContext,
) -> Result<LaunchResult> {
    let dir = service.paths().instance_dir(id);
    let lock_path = dir.join("operation.lock");
    let _lock = FileLock::acquire(&lock_path)?;
    let instance = service.instance(id)?;
    if instance.install_state != InstallState::Ready {
        return Err(AppError::Core("实例未完成安装；请先执行 p0 install".into()));
    }
    let installed_path = dir.join("installation.json");
    let installed: Installed = serde_json::from_slice(
        &tokio::fs::read(&installed_path)
            .await
            .map_err(|e| io_error("读取安装完成记录", &installed_path, e))?,
    )?;
    if installed.schema_version != 1
        || installed.instance != id
        || installed.game_version != instance.game_version
        || installed.loader != instance.loader
        || installed.platform != current_platform()
    {
        return Err(AppError::Core(
            "安装记录与实例配置不一致，请重新安装".into(),
        ));
    }
    let settings = instance
        .launch
        .clone()
        .unwrap_or(network.policy().launch.clone());
    let policy = options
        .java
        .map_or(settings.java.clone(), |path| JavaPolicy::Executable {
            path,
        });
    let runtime = java::resolve(
        service.paths(),
        &network,
        &policy,
        u32::from(installed.metadata.java_version.major_version),
        task.clone(),
    )
    .await?;
    let account = accounts.get(options.account)?;
    let profile = accounts.authenticate(&account, task.child_token()).await?;
    let version = builder(&service, &instance)?;
    let mut variables = HashMap::from([
        ("resolution_width".into(), settings.window_width.to_string()),
        (
            "resolution_height".into(),
            settings.window_height.to_string(),
        ),
        ("launcher_name".into(), "RTMCL".into()),
        ("launcher_version".into(), env!("CARGO_PKG_VERSION").into()),
    ]);
    variables.insert("version_name".into(), instance.game_version.clone());
    let jvm = HashMap::from([
        ("Xms".into(), format!("{}M", settings.min_memory_mib)),
        ("Xmx".into(), format!("{}M", settings.max_memory_mib)),
    ]);
    let mut arguments = version.build_arguments(
        &installed.metadata,
        Some(&profile),
        &variables,
        &HashSet::new(),
        &jvm,
        &HashSet::new(),
        &[],
    );
    let mut additional = settings.jvm_args.clone();
    if let Some(logging) = installed.logging_argument {
        additional.push(logging);
    }
    additional.append(&mut arguments);
    let cancellation = task.child_token();
    if cancellation.is_cancelled() {
        return Err(AppError::Cancelled);
    }
    tokio::fs::create_dir_all(&service.paths().logs)
        .await
        .map_err(|e| io_error("创建日志目录", &service.paths().logs, e))?;
    let log_file = service
        .paths()
        .logs
        .join(format!("game-{id}-{}.log", uuid::Uuid::new_v4()));
    let spec = ProcessSpec {
        paths: service.paths().clone(),
        instance: id,
        executable: runtime.executable,
        arguments: additional,
        working_dir: version.game_dirs,
        log_file,
    };
    // The supervisor takes the authoritative lock before spawning. A competing launch can
    // win the handoff race, in which case this launch cleanly fails without spawning a game.
    drop(_lock);
    let executable = std::env::current_exe().map_err(|e| io_error("定位启动器进程", &dir, e))?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("__supervise")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(false);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    #[cfg(unix)]
    command.process_group(0);
    let mut supervisor = command
        .spawn()
        .map_err(|e| io_error("启动游戏监督进程", &dir, e))?;
    let mut input = supervisor
        .stdin
        .take()
        .ok_or_else(|| AppError::Core("监督进程没有输入管道".into()))?;
    input
        .write_all(&serde_json::to_vec(&spec)?)
        .await
        .map_err(|e| io_error("传递私有启动描述", &dir, e))?;
    input
        .shutdown()
        .await
        .map_err(|e| io_error("结束启动描述输入", &dir, e))?;
    drop(input);
    let output = supervisor
        .stdout
        .take()
        .ok_or_else(|| AppError::Core("监督进程没有结果管道".into()))?;
    let mut reader = tokio::io::BufReader::new(output);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(20), reader.read_line(&mut line))
        .await
        .map_err(|_| AppError::Core("监督进程启动超时，请检查实例运行状态".into()))?
        .map_err(|e| io_error("读取启动结果", &dir, e))?;
    if line.is_empty() {
        return Err(AppError::Core(
            "监督进程未启动游戏（实例可能已锁定）".into(),
        ));
    }
    let started: LaunchResult = serde_json::from_str(&line)?;
    if !options.wait {
        return Ok(started);
    }
    line.clear();
    tokio::select! {
        result = reader.read_line(&mut line) => { result.map_err(|e| io_error("读取游戏退出结果", &dir, e))?; },
        _ = cancellation.cancelled() => return Ok(started), // Game and lock remain with supervisor.
    }
    if line.is_empty() {
        return Err(AppError::Core(
            "监督进程连接中断；请查看实例日志与 last-run.json".into(),
        ));
    }
    Ok(serde_json::from_str(&line)?)
}

/// Hidden CLI worker. It outlives the interactive launcher and owns the instance lock.
pub async fn supervise() -> Result<()> {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::Core(format!("读取启动描述失败: {e}")))?;
    let spec: ProcessSpec = serde_json::from_slice(&bytes)?;
    let service = LauncherService::new(spec.paths.clone());
    let dir = spec.paths.instance_dir(spec.instance);
    let _lock = FileLock::acquire(&dir.join("operation.lock"))?;
    let instance = service.instance(spec.instance)?;
    if instance.install_state != InstallState::Ready || spec.working_dir != dir.join("minecraft") {
        return Err(AppError::Core("监督进程的实例状态或目录不匹配".into()));
    }
    let stdout = std::fs::File::create(&spec.log_file)
        .map_err(|e| io_error("创建游戏日志", &spec.log_file, e))?;
    let stderr = stdout
        .try_clone()
        .map_err(|e| io_error("复制游戏日志句柄", &spec.log_file, e))?;
    let mut command = tokio::process::Command::new(&spec.executable);
    command
        .args(&spec.arguments)
        .current_dir(&spec.working_dir)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .kill_on_drop(false);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|e| io_error("启动 Java", &spec.executable, e))?;
    let process_id = child
        .id()
        .ok_or_else(|| AppError::Core("Java 没有 PID".into()))?;
    let running = Running {
        schema_version: 1,
        session: uuid::Uuid::new_v4(),
        process_id,
    };
    let running_path = dir.join("running.json");
    if let Err(error) = write_bytes(&running_path, &serde_json::to_vec(&running)?) {
        let _ = child.kill().await;
        return Err(error);
    }
    let mut output = std::io::stdout();
    let _ = writeln!(
        output,
        "{}",
        serde_json::to_string(&LaunchResult {
            process_id,
            log_file: spec.log_file.clone(),
            exit_code: None,
            success: None
        })?
    );
    let _ = output.flush();
    let stop_path = dir.join(format!("stop-{}", running.session));
    let mut check_stop = tokio::time::interval(Duration::from_millis(200));
    let exit = loop {
        tokio::select! {
            result = child.wait() => break result.map_err(|e| io_error("监督游戏退出", &spec.log_file, e))?,
            _ = check_stop.tick() => if stop_path.exists() { break child.kill().await.and_then(|_| child.try_wait()).map_err(|e| io_error("终止游戏进程", &spec.log_file, e))?.ok_or_else(|| AppError::Core("终止后未确认游戏退出".into()))?; },
        }
    };
    let result = LaunchResult {
        process_id,
        log_file: spec.log_file,
        exit_code: exit.code(),
        success: Some(exit.success()),
    };
    write_bytes(&dir.join("last-run.json"), &serde_json::to_vec(&result)?)?;
    let _ = tokio::fs::remove_file(&running_path).await;
    let _ = tokio::fs::remove_file(&stop_path).await;
    let _ = writeln!(output, "{}", serde_json::to_string(&result)?);
    let _ = output.flush();
    Ok(())
}

pub async fn stop(service: &LauncherService, id: InstanceId) -> Result<()> {
    service.instance(id)?;
    let dir = service.paths().instance_dir(id);
    let running_path = dir.join("running.json");
    let running: Running = serde_json::from_slice(
        &tokio::fs::read(&running_path)
            .await
            .map_err(|e| io_error("读取正在运行的会话", &running_path, e))?,
    )?;
    let path = dir.join(format!("stop-{}", running.session));
    write_bytes(&path, b"stop")?;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if !running_path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| AppError::Core("已请求终止，但监督进程未确认退出；请查看日志".into()))?;
    Ok(())
}

pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if relative.contains('\\')
        || relative.contains(':')
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        || relative.is_empty()
    {
        return Err(AppError::Core("元数据含有不安全的相对路径".into()));
    }
    Ok(root.join(path))
}

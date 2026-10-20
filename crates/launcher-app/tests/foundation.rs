use launcher_app::{
    AppError, AppPaths, LauncherService,
    config::{AppConfig, SourceMode},
    core::{CoreAdapter, LightyAdapter},
    domain::{InstallState, InstanceConfig, Loader},
    storage::{FileLock, read_toml, write_toml},
    tasks::{DownloadProgress, TaskHandle, TaskOutcome, TaskState},
};

fn service(root: &std::path::Path) -> LauncherService {
    LauncherService::new(AppPaths::discover(Some(root)).unwrap())
}

#[test]
fn defaults_do_not_create_files_and_initialization_preserves_settings() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp.path().join("应用 空格"));
    assert_eq!(service.settings().unwrap().schema_version, 1);
    assert!(!service.paths().config.exists());
    service.initialize().unwrap();
    let path = service.paths().settings_file();
    let mut config = service.settings().unwrap();
    config.launch.max_memory_mib = 4096;
    let _lock = FileLock::acquire(&service.paths().config.join("settings.lock")).unwrap();
    write_toml(&path, &config).unwrap();
    drop(_lock);
    service.initialize().unwrap();
    assert_eq!(service.settings().unwrap().launch.max_memory_mib, 4096);
}

#[test]
fn future_schema_and_malformed_config_are_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(temp.path());
    service.initialize().unwrap();
    let path = service.paths().settings_file();
    for contents in [
        "schema_version = 999\nlaunch = 'changed format'\n",
        "not valid toml",
        "[downloads]\n",
    ] {
        std::fs::write(&path, contents).unwrap();
        assert!(service.initialize().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    }
    std::fs::write(&path, "schema_version = 999\n").unwrap();
    assert!(matches!(
        service.settings(),
        Err(AppError::UnsupportedSchema { found: 999, .. })
    ));
}

#[test]
fn strict_sources_and_mirror_credentials_are_validated() {
    let mut config = AppConfig::default();
    config.downloads.mode = SourceMode::Strict;
    assert!(config.validate().is_err());
    config.downloads.sources.truncate(1);
    assert!(config.validate().is_ok());
    config.mirrors.bmclapi.base_url = "https://user:secret@example.org".parse().unwrap();
    assert!(config.validate().is_err());
}

#[test]
fn instances_have_unique_ids_isolated_directories_and_explicit_fabric_versions() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(&temp.path().join("中文 路径"));
    let first = service
        .create_instance("同名".into(), "1.21.1".into(), Loader::Vanilla)
        .unwrap();
    let second = service
        .create_instance(
            "同名".into(),
            "1.21.1".into(),
            Loader::Fabric {
                version: "0.16.10".into(),
            },
        )
        .unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(first.install_state, InstallState::NotInstalled);
    assert!(first.launch.is_none());
    for instance in [&first, &second] {
        assert!(
            service
                .paths()
                .instance_dir(instance.id)
                .join("minecraft")
                .is_dir()
        );
    }
    assert_eq!(service.list_instances().unwrap().len(), 2);
    assert!(
        service
            .create_instance(
                "bad".into(),
                "1.21.1".into(),
                Loader::Fabric {
                    version: String::new()
                }
            )
            .is_err()
    );
    assert_eq!(service.list_instances().unwrap().len(), 2);
}

#[test]
fn rejects_instance_identity_and_platform_mismatches() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(temp.path());
    let mut instance = service
        .create_instance("test".into(), "1.20.1".into(), Loader::Vanilla)
        .unwrap();
    let original = service.paths().instance_dir(instance.id);
    instance.id = Default::default();
    write_toml(&original.join("instance.toml"), &instance).unwrap();
    assert!(service.list_instances().is_err());
    instance.platform = "other-os-arch".into();
    assert!(instance.validate().is_err());
}

#[test]
fn atomic_replacement_remains_parseable_without_temp_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.toml");
    let config = AppConfig::default();
    write_toml(&path, &config).unwrap();
    write_toml(&path, &config).unwrap();
    let loaded: AppConfig = read_toml(&path).unwrap();
    loaded.validate().unwrap();
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn atomic_configuration_commit_supports_deep_windows_paths() {
    let temp = tempfile::tempdir().unwrap();
    let mut parent = temp.path().to_owned();
    for _ in 0..8 {
        parent.push("deep-maven-artifact-中文路径-123456789");
    }
    std::fs::create_dir_all(&parent).unwrap();
    let path = parent.join("artifact.jar.verified.toml");
    write_toml(&path, &AppConfig::default()).unwrap();
    let config: AppConfig = read_toml(&path).unwrap();
    config.validate().unwrap();
}

#[test]
fn lock_excludes_another_process_and_is_released_on_drop() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("instance.lock");
    let lock = FileLock::acquire(&path).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cross_process_lock_probe", "--nocapture"])
        .env("RTMCL_TEST_LOCK_PATH", &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    drop(lock);
    FileLock::acquire(&path).unwrap();
}

#[test]
fn cross_process_lock_probe() {
    if let Some(path) = std::env::var_os("RTMCL_TEST_LOCK_PATH") {
        assert!(FileLock::acquire(std::path::Path::new(&path)).is_err());
    }
}

#[tokio::test]
async fn late_subscriber_reads_retained_terminal_snapshot() {
    let (handle, context) = TaskHandle::new(None);
    context.start().unwrap();
    context
        .progress(DownloadProgress {
            downloaded_bytes: 42,
            total_bytes: Some(42),
            source: None,
        })
        .unwrap();
    context.finish(TaskOutcome::Succeeded).unwrap();
    let state = handle.subscribe();
    assert_eq!(state.borrow().state, TaskState::Succeeded);
    assert_eq!(state.borrow().progress.downloaded_bytes, 42);
    assert!(context.progress(DownloadProgress::default()).is_err());
    handle.request_cancel();
    assert_eq!(handle.snapshot().state, TaskState::Succeeded);
}

#[tokio::test]
async fn cancellation_is_acknowledged_only_after_worker_cleanup() {
    let (handle, context) = TaskHandle::new(None);
    context.start().unwrap();
    let child = context.child_token();
    handle.request_cancel();
    assert!(child.is_cancelled());
    assert_eq!(handle.snapshot().state, TaskState::Cancelling);
    context.finish(TaskOutcome::Cancelled).unwrap();
    assert_eq!(handle.snapshot().state, TaskState::Cancelled);
    assert!(context.finish(TaskOutcome::Succeeded).is_err());
}

#[test]
fn queued_cancellation_cannot_be_started_or_report_success() {
    let (handle, context) = TaskHandle::new(None);
    assert!(context.finish(TaskOutcome::Succeeded).is_err());
    handle.request_cancel();
    assert!(context.start().is_err());
    context.finish(TaskOutcome::Cancelled).unwrap();
    assert_eq!(handle.snapshot().state, TaskState::Cancelled);
}

#[tokio::test]
async fn uninstalled_instance_cannot_be_launched() {
    let temp = tempfile::tempdir().unwrap();
    let service = service(temp.path());
    let adapter = LightyAdapter::new(
        service.clone(),
        launcher_app::accounts::Accounts::new(service.paths().clone()),
    );
    let instance = InstanceConfig::new("test".into(), "1.21.1".into(), Loader::Vanilla).unwrap();
    let (_, context) = TaskHandle::new(Some(instance.id));
    assert!(adapter.launch(instance, context).await.is_err());
}

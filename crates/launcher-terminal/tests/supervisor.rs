use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use launcher_app::{
    AppPaths, LauncherService,
    domain::{InstallState, Loader},
    storage::FileLock,
};

#[test]
fn detached_supervisor_keeps_lock_and_accepts_explicit_stop() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::discover(Some(temp.path())).unwrap();
    let service = LauncherService::new(paths.clone());
    let mut instance = service
        .create_instance("process-probe".into(), "test".into(), Loader::Vanilla)
        .unwrap();
    // This fixture exercises the private process worker without downloading or running a game.
    instance.install_state = InstallState::Ready;
    service.save_instance(&instance).unwrap();
    let dir = paths.instance_dir(instance.id);
    #[cfg(windows)]
    let (executable, arguments) = (
        std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("System32/ping.exe"),
        vec!["-n", "30", "127.0.0.1"],
    );
    #[cfg(unix)]
    let (executable, arguments) = (std::path::PathBuf::from("/bin/sleep"), vec!["30"]);
    let spec = serde_json::json!({ "paths": paths, "instance": instance.id, "executable": executable, "arguments": arguments, "working_dir": dir.join("minecraft"), "log_file": paths.logs.join("process-probe.log") });
    let mut child = Command::new(env!("CARGO_BIN_EXE_rtmcl"))
        .arg("__supervise")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&spec).unwrap().as_bytes())
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert!(
        line.contains("process_id"),
        "supervisor did not start: {line}"
    );
    drop(output);
    drop(child); // Equivalent to the interactive launcher closing its handle and result pipe.
    assert!(dir.join("running.json").exists());
    assert!(FileLock::acquire(&dir.join("operation.lock")).is_err());
    let stopped = Command::new(env!("CARGO_BIN_EXE_rtmcl"))
        .arg("--data-dir")
        .arg(temp.path())
        .args(["p0", "stop", &instance.id.to_string()])
        .output()
        .unwrap();
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(!dir.join("running.json").exists());
    assert!(dir.join("last-run.json").is_file());
    FileLock::acquire(&dir.join("operation.lock")).unwrap();
}

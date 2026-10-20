use std::{
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rtmcl"))
        .arg("--data-dir")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn doctor_and_paths_do_not_initialize_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("absent");
    for args in [vec!["doctor"], vec!["paths"]] {
        let output = run(&root, &args);
        assert!(output.status.success());
        assert!(!root.exists());
    }
}

#[test]
fn cli_can_create_and_list_persisted_instance_config() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("中文 空格");
    let output = run(
        &root,
        &[
            "instance",
            "create",
            "测试实例",
            "--game-version",
            "1.21.1",
            "--fabric-version",
            "0.16.10",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listed = run(&root, &["instance", "list"]);
    assert!(listed.status.success());
    let text = String::from_utf8(listed.stdout).unwrap();
    assert!(text.contains("测试实例") && text.contains("0.16.10") && text.contains("NotInstalled"));
}

#[test]
fn noninteractive_tui_returns_useful_error() {
    let temp = tempfile::tempdir().unwrap();
    let output = run(temp.path(), &["tui"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("交互式终端"));
}

#[test]
fn launch_requires_an_account_and_rejects_implicit_unverified_shortcuts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("absent");
    let instance = "00000000-0000-4000-8000-000000000000";
    for args in [
        vec!["p0", "launch", instance, "--local-player", "TestPlayer"],
        vec!["p0", "launch", instance],
    ] {
        let output = run(&root, &args);
        assert!(!output.status.success());
        assert!(!root.exists());
    }
    let help = run(&root, &["p0", "launch", "--help"]);
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(help.status.success() && text.contains("--account <ACCOUNT>"));
    assert!(!text.contains("local-player"));
}

#[test]
fn local_creation_checks_ownership_unless_explicitly_built_for_development() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("absent");
    let output = run(&root, &["account", "add-local", "DevPlayer"]);
    assert_eq!(
        output.status.success(),
        cfg!(feature = "dev-local-accounts")
    );
    if cfg!(feature = "dev-local-accounts") {
        let contents = std::fs::read_to_string(root.join("config/accounts.toml")).unwrap();
        assert!(contents.contains("development_only = true"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("开发构建"));
        let listed = run(&root, &["account", "list"]);
        assert!(
            listed.status.success()
                && String::from_utf8_lossy(&listed.stdout).contains("DevPlayer")
        );
    } else {
        assert!(String::from_utf8_lossy(&output.stderr).contains("请先登录"));
        assert!(!root.exists());
    }
}

#[test]
fn instance_log_command_reads_game_output_and_rejects_invalid_limits() {
    use launcher_app::{AppPaths, LauncherService, domain::Loader};
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::discover(Some(root.path())).unwrap();
    let service = LauncherService::new(paths.clone());
    let instance = service
        .create_instance("logs".into(), "1.21.1".into(), Loader::Vanilla)
        .unwrap();
    let id = instance.id.to_string();
    assert!(
        !run(root.path(), &["instance", "logs", &id])
            .status
            .success()
    );
    let path = paths.logs.join(format!(
        "game-{id}-00000000-0000-4000-8000-000000000001.log"
    ));
    std::fs::write(path, "old\nrendered\n\u{1b}[31mlast\n").unwrap();
    let output = run(root.path(), &["instance", "logs", &id, "--lines", "2"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("rendered") && text.contains("last"));
    assert!(!text.contains("old") && !text.contains('\u{1b}'));
    for limit in ["0", "10001", "-1"] {
        assert!(
            !run(root.path(), &["instance", "logs", &id, "--lines", limit])
                .status
                .success()
        );
    }
}

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
fn rejects_removed_test_login_and_requires_an_account_before_creating_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("absent");
    let instance = "00000000-0000-4000-8000-000000000000";
    for args in [
        vec!["account", "add-local", "TestPlayer"],
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

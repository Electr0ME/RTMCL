use launcher_app::{
    AppError, AppPaths,
    accounts::{AccountKind, Accounts},
    domain::AccountId,
};

fn write_accounts(paths: &AppPaths, content: &str) {
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(paths.config.join("accounts.toml"), content).unwrap();
}

fn microsoft_entry(id: AccountId) -> String {
    format!(
        r#"
[[accounts]]
id = "{id}"
username = "SyntheticUser"
minecraft_uuid = "00000000000000000000000000000001"
[accounts.provider]
kind = "microsoft"
client_id = "00000000-0000-4000-8000-000000000000"
persistence = "session_only"
system_credential = false
"#
    )
}

#[test]
fn legacy_test_accounts_cannot_be_selected_and_do_not_hide_microsoft_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::discover(Some(temp.path())).unwrap();
    let legacy = AccountId::new();
    let microsoft = AccountId::new();
    let content = format!(
        r#"schema_version = 1
[[accounts]]
id = "{legacy}"
username = "OldTestPlayer"
minecraft_uuid = "00000000000000000000000000000002"
[accounts.provider]
kind = "local"
{}"#,
        microsoft_entry(microsoft)
    );
    write_accounts(&paths, &content);
    let accounts = Accounts::new(paths.clone());
    assert!(accounts.get(legacy).is_err());
    let records = accounts.list().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, microsoft);
    assert_eq!(
        std::fs::read_to_string(paths.config.join("accounts.toml")).unwrap(),
        content
    );
}

#[test]
fn microsoft_metadata_round_trips_and_logout_removes_it() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::discover(Some(temp.path())).unwrap();
    let id = AccountId::new();
    write_accounts(
        &paths,
        &format!("schema_version = 1\n{}", microsoft_entry(id)),
    );
    let accounts = Accounts::new(paths.clone());
    assert!(matches!(
        accounts.get(id).unwrap().provider,
        AccountKind::Microsoft { .. }
    ));
    accounts.remove(id).unwrap();
    assert!(Accounts::new(paths.clone()).list().unwrap().is_empty());
    assert!(
        !std::fs::read_to_string(paths.config.join("accounts.toml"))
            .unwrap()
            .contains("SyntheticUser")
    );
}

#[test]
fn future_schema_and_unknown_providers_are_rejected_without_rewriting() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::discover(Some(temp.path())).unwrap();
    let id = AccountId::new();
    let future = format!("schema_version = 999\n{}", microsoft_entry(id));
    write_accounts(&paths, &future);
    assert!(matches!(
        Accounts::new(paths.clone()).list(),
        Err(AppError::UnsupportedSchema { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(paths.config.join("accounts.toml")).unwrap(),
        future
    );
    let unknown = format!(
        "schema_version = 1\n{}",
        microsoft_entry(id).replace("\"microsoft\"", "\"unknown-provider\"")
    );
    write_accounts(&paths, &unknown);
    assert!(Accounts::new(paths.clone()).list().is_err());
    assert_eq!(
        std::fs::read_to_string(paths.config.join("accounts.toml")).unwrap(),
        unknown
    );
}

#[tokio::test]
async fn microsoft_metadata_alone_does_not_unlock_local_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::discover(Some(temp.path())).unwrap();
    let owner = AccountId::new();
    let content = format!("schema_version = 1\n{}", microsoft_entry(owner));
    write_accounts(&paths, &content);
    let accounts = Accounts::new(paths.clone());
    assert!(
        accounts
            .add_local(
                "TestPlayer".into(),
                Some(owner),
                launcher_app::tasks::Cancellation::new()
            )
            .await
            .is_err()
    );
    assert_eq!(accounts.list().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(paths.config.join("accounts.toml")).unwrap(),
        content
    );
}

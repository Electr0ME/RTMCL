use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use lighty_launcher::auth::{
    AuthProvider, Authenticator, ExposeSecret, MicrosoftAuth, SecretString, UserProfile,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    AppError, AppPaths, Result,
    domain::{AccountId, SCHEMA_VERSION, check_schema},
    ports::CredentialPersistence,
    storage::{FileLock, read_toml, write_toml},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountKind {
    Microsoft {
        client_id: String,
        persistence: CredentialPersistence,
        #[serde(default)]
        system_credential: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    pub id: AccountId,
    pub username: String,
    pub minecraft_uuid: String,
    pub provider: AccountKind,
}

#[derive(Serialize)]
struct AccountFile {
    schema_version: u32,
    accounts: Vec<AccountRecord>,
}

#[derive(Deserialize)]
struct StoredAccountFile {
    schema_version: u32,
    accounts: Vec<toml::Value>,
}

#[derive(Clone)]
pub struct Accounts {
    paths: AppPaths,
    // Secrets only in memory when system storage is unavailable or explicitly disabled.
    session: Arc<Mutex<HashMap<AccountId, SecretString>>>,
}

impl Accounts {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            session: Arc::default(),
        }
    }

    pub fn list(&self) -> Result<Vec<AccountRecord>> {
        let path = self.paths.config.join("accounts.toml");
        match read_toml::<StoredAccountFile>(&path) {
            Ok(file) => {
                check_schema(file.schema_version)?;
                file.accounts
                    .into_iter()
                    // Ignore records from the removed test mode. They cannot be selected
                    // or authenticated, and reading never rewrites the old file.
                    .filter(|record| {
                        record
                            .get("provider")
                            .and_then(|provider| provider.get("kind"))
                            .and_then(toml::Value::as_str)
                            != Some("local")
                    })
                    .map(|record| Ok(record.try_into::<AccountRecord>()?))
                    .collect()
            }
            Err(AppError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error),
        }
    }

    fn save_record(&self, record: AccountRecord) -> Result<()> {
        self.paths.ensure()?;
        let _lock = FileLock::acquire(&self.paths.config.join("accounts.lock"))?;
        let mut accounts = self.list()?;
        accounts.retain(|account| account.id != record.id);
        accounts.push(record);
        write_toml(
            &self.paths.config.join("accounts.toml"),
            &AccountFile {
                schema_version: SCHEMA_VERSION,
                accounts,
            },
        )
    }

    pub fn get(&self, id: AccountId) -> Result<AccountRecord> {
        self.list()?
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| AppError::Account(format!("账户不存在: {id}")))
    }

    pub async fn login_microsoft(
        &self,
        client_id: String,
        session_only: bool,
        prompt: tokio::sync::mpsc::UnboundedSender<(String, String)>,
        cancellation: CancellationToken,
    ) -> Result<AccountRecord> {
        if Uuid::parse_str(&client_id).is_err() {
            return Err(AppError::Account(
                "请提供本项目注册的 Microsoft 应用 Client ID（UUID）".into(),
            ));
        }
        let mut auth = MicrosoftAuth::new(client_id.clone());
        auth.set_device_code_callback(move |code, url| {
            let _ = prompt.send((code.to_owned(), url.to_owned()));
        });
        let events = lighty_launcher::event::EventBus::new(32);
        let mut receiver = events.subscribe();
        let profile = tokio::select! {
            _ = cancellation.cancelled() => return Err(AppError::Cancelled),
            result = auth.authenticate(Some(&events)) => result.map_err(|error| auth_error(error, &mut receiver))?,
        };
        self.persist_profile(profile, client_id, session_only).await
    }

    async fn persist_profile(
        &self,
        profile: UserProfile,
        client_id: String,
        session_only: bool,
    ) -> Result<AccountRecord> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let existing = this
                .list()?
                .into_iter()
                .find(|account| account.minecraft_uuid == profile.uuid);
            let prior_system_credential = existing.as_ref().is_some_and(|account| {
                matches!(
                    account.provider,
                    AccountKind::Microsoft {
                        persistence: CredentialPersistence::System,
                        ..
                    } | AccountKind::Microsoft {
                        system_credential: true,
                        ..
                    }
                )
            });
            let id = existing.map(|account| account.id).unwrap_or_default();
            let token = match &profile.provider {
                AuthProvider::Microsoft {
                    refresh_token: Some(token),
                    ..
                } => token.clone(),
                _ => return Err(AppError::Account("微软未返回刷新令牌".into())),
            };
            let persistence = if !session_only && store_system_token(id, &token).is_ok() {
                this.session
                    .lock()
                    .map_err(|_| AppError::Account("会话凭据锁失效".into()))?
                    .remove(&id);
                CredentialPersistence::System
            } else {
                this.session
                    .lock()
                    .map_err(|_| AppError::Account("会话凭据锁失效".into()))?
                    .insert(id, token);
                CredentialPersistence::SessionOnly
            };
            let record = AccountRecord {
                id,
                username: profile.username,
                minecraft_uuid: profile.uuid,
                provider: AccountKind::Microsoft {
                    client_id,
                    persistence,
                    system_credential: prior_system_credential
                        || persistence == CredentialPersistence::System,
                },
            };
            this.save_record(record.clone())?;
            Ok(record)
        })
        .await
        .map_err(|_| AppError::Account("保存账户任务失败".into()))?
    }

    /// Returns an upstream profile only inside the application/core boundary.
    pub(crate) async fn authenticate(
        &self,
        record: &AccountRecord,
        cancellation: CancellationToken,
    ) -> Result<UserProfile> {
        let AccountKind::Microsoft {
            client_id,
            persistence,
            ..
        } = &record.provider;
        let this = self.clone();
        let id = record.id;
        let session_only = *persistence == CredentialPersistence::SessionOnly;
        let token = tokio::task::spawn_blocking(move || {
            if let Some(token) = this
                .session
                .lock()
                .map_err(|_| AppError::Account("会话凭据锁失效".into()))?
                .get(&id)
            {
                return Ok(token.clone());
            }
            if session_only {
                return Err(AppError::Account("本次会话凭据已失效，请重新登录".into()));
            }
            load_system_token(id)
        })
        .await
        .map_err(|_| AppError::Account("读取凭据任务失败".into()))??;
        let mut auth = MicrosoftAuth::new(client_id.clone());
        let events = lighty_launcher::event::EventBus::new(32);
        let mut receiver = events.subscribe();
        let profile = tokio::select! {
            _ = cancellation.cancelled() => return Err(AppError::Cancelled),
            result = auth.authenticate_with_refresh_token(&token, Some(&events)) => result.map_err(|error| auth_error(error, &mut receiver))?,
        };
        // Rotate the refresh token before allowing this authentication to succeed.
        self.persist_profile(
            profile.clone(),
            client_id.clone(),
            *persistence == CredentialPersistence::SessionOnly,
        )
        .await?;
        Ok(profile)
    }

    pub fn remove(&self, id: AccountId) -> Result<()> {
        let record = self.get(id)?;
        if matches!(
            record.provider,
            AccountKind::Microsoft {
                persistence: CredentialPersistence::System,
                ..
            } | AccountKind::Microsoft {
                system_credential: true,
                ..
            }
        ) {
            let entry = keyring::Entry::new("rtmcl.microsoft.refresh", &id.to_string())
                .map_err(|_| AppError::Account("无法访问系统凭据服务".into()))?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(_) => {
                    return Err(AppError::Account(
                        "删除系统刷新令牌失败，账户资料已保留以便重试".into(),
                    ));
                }
            }
        }
        self.session
            .lock()
            .map_err(|_| AppError::Account("会话凭据锁失效".into()))?
            .remove(&id);
        let _lock = FileLock::acquire(&self.paths.config.join("accounts.lock"))?;
        let mut accounts = self.list()?;
        accounts.retain(|account| account.id != id);
        write_toml(
            &self.paths.config.join("accounts.toml"),
            &AccountFile {
                schema_version: SCHEMA_VERSION,
                accounts,
            },
        )
    }
}

fn store_system_token(id: AccountId, token: &SecretString) -> Result<()> {
    keyring::Entry::new("rtmcl.microsoft.refresh", &id.to_string())
        .and_then(|entry| entry.set_password(token.expose_secret()))
        .map_err(|_| AppError::Account("系统凭据服务不可用".into()))
}

fn load_system_token(id: AccountId) -> Result<SecretString> {
    keyring::Entry::new("rtmcl.microsoft.refresh", &id.to_string())
        .and_then(|entry| entry.get_password())
        .map(SecretString::from)
        .map_err(|_| AppError::Account("无可用刷新令牌，请重新登录；会话凭据不会跨进程保存".into()))
}

fn auth_error(
    error: lighty_launcher::auth::AuthError,
    receiver: &mut lighty_launcher::_event::EventReceiver,
) -> AppError {
    use lighty_launcher::auth::AuthError;
    use lighty_launcher::event::{AuthEvent, Event};
    // Read only known progress labels. Failure events and arbitrary response text may
    // contain credentials; neither is copied into application errors or logs.
    let mut stage = "微软认证";
    while let Ok(event) = receiver.try_next() {
        if let Event::Auth(AuthEvent::AuthenticationInProgress { step, .. }) = event {
            stage = match step.as_str() {
                "Requesting device code" => "请求设备码",
                "Waiting for user authorization" => "等待授权 / 交换 Microsoft 令牌",
                "Refreshing Microsoft token" => "刷新 Microsoft 令牌",
                "Exchanging for Xbox Live token" => "Xbox Live 认证",
                "Exchanging for XSTS token" => "XSTS 授权",
                "Exchanging for Minecraft token" => "Minecraft Services 令牌交换",
                "Fetching Minecraft profile" => "Minecraft 玩家资料",
                _ => stage,
            };
        }
    }
    let description = match error {
        AuthError::HttpStatus { status, body } => {
            // Allowlist a service error needed to diagnose new app registrations.
            // Do not forward the raw body, URL, request headers, or token values.
            let invalid_registration = body.len() <= 65536
                && serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .is_some_and(|value| {
                        ["error", "errorMessage"].iter().any(|key| {
                            value[*key].as_str().is_some_and(|text| {
                                text.eq_ignore_ascii_case("Invalid app registration")
                                    || text.to_ascii_lowercase().starts_with(
                                        "invalid app registration, see https://aka.ms/appreginfo",
                                    )
                            })
                        })
                    });
            let hint = if invalid_registration {
                "（Invalid app registration：服务拒绝了本项目应用注册，请核对该 Client ID 的 Minecraft Services 接入）"
            } else {
                ""
            };
            format!("{stage}返回 HTTP {status}{hint}")
        }
        AuthError::DeviceCodeExpired => "设备码已过期".into(),
        AuthError::Cancelled => "登录已取消".into(),
        AuthError::MinecraftNotOwned => "账户没有 Minecraft Java 版资料或授权".into(),
        AuthError::XboxLiveUnavailable => "Xbox Live 服务不可用".into(),
        AuthError::InvalidToken => format!("{stage}失败，请重新登录"),
        _ => format!("{stage}失败，请检查网络、Client ID、授权与账户状态后重试"),
    };
    AppError::Account(description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_diagnostics_keep_stage_but_never_echo_secrets() {
        use lighty_launcher::{
            auth::AuthError,
            event::{AuthEvent, Event, EventBus},
        };
        let bus = EventBus::new(32);
        let mut receiver = bus.subscribe();
        for step in [
            "Exchanging for Xbox Live token",
            "Exchanging for XSTS token",
            "Exchanging for Minecraft token",
        ] {
            bus.emit(Event::Auth(AuthEvent::AuthenticationInProgress {
                provider: "Microsoft".into(),
                step: step.into(),
            }));
        }
        bus.emit(Event::Auth(AuthEvent::AuthenticationFailed {
            provider: "Microsoft".into(),
            error: "synthetic-secret-event".into(),
        }));
        let error = auth_error(AuthError::HttpStatus {
            status: 403,
            body: r#"{"errorMessage":"Invalid app registration","access_token":"synthetic-secret-body"}"#.into(),
        }, &mut receiver).to_string();
        assert!(error.contains("Minecraft Services 令牌交换") && error.contains("HTTP 403"));
        assert!(error.contains("Invalid app registration"));
        assert!(!error.contains("synthetic-secret") && !error.contains("access_token"));
        let with_help_link = auth_error(
            AuthError::HttpStatus {
                status: 403,
                body: r#"{"errorMessage":"Invalid app registration, see https://aka.ms/AppRegInfo for more information","extra":"synthetic-secret-body"}"#.into(),
            },
            &mut receiver,
        ).to_string();
        assert!(with_help_link.contains("Invalid app registration"));
        assert!(!with_help_link.contains("synthetic-secret") && !with_help_link.contains("extra"));
        for body in [
            "synthetic-secret-body",
            r#"{"error":"synthetic-secret-body Invalid app registration"}"#,
        ] {
            let error = auth_error(
                AuthError::HttpStatus {
                    status: 403,
                    body: body.into(),
                },
                &mut receiver,
            )
            .to_string();
            assert!(
                !error.contains("synthetic-secret") && !error.contains("Invalid app registration")
            );
        }
    }

    fn profile(refresh: &str) -> UserProfile {
        UserProfile {
            id: None,
            username: "SyntheticUser".into(),
            uuid: "00000000000000000000000000000001".into(),
            access_token: Some(SecretString::from("synthetic-access-secret".to_owned())),
            xuid: None,
            email: None,
            email_verified: true,
            money: None,
            role: None,
            banned: false,
            provider: AuthProvider::Microsoft {
                client_id: "test-client".into(),
                refresh_token: Some(SecretString::from(refresh.to_owned())),
            },
        }
    }

    #[tokio::test]
    async fn missing_session_credentials_cannot_authenticate() {
        let temp = tempfile::tempdir().unwrap();
        let accounts = Accounts::new(AppPaths::discover(Some(temp.path())).unwrap());
        let record = accounts
            .persist_profile(profile("synthetic-refresh"), "test-client".into(), true)
            .await
            .unwrap();
        let reopened = Accounts::new(accounts.paths.clone());
        assert!(matches!(
            reopened
                .authenticate(&record, CancellationToken::new())
                .await,
            Err(AppError::Account(_))
        ));
    }

    #[tokio::test]
    async fn session_tokens_rotate_without_plaintext_persistence() {
        let temp = tempfile::tempdir().unwrap();
        let accounts = Accounts::new(AppPaths::discover(Some(temp.path())).unwrap());
        let first = accounts
            .persist_profile(profile("synthetic-refresh-one"), "test-client".into(), true)
            .await
            .unwrap();
        let rotated = accounts
            .persist_profile(profile("synthetic-refresh-two"), "test-client".into(), true)
            .await
            .unwrap();
        assert_eq!(first.id, rotated.id);
        assert_eq!(
            accounts
                .session
                .lock()
                .unwrap()
                .get(&first.id)
                .unwrap()
                .expose_secret(),
            "synthetic-refresh-two"
        );
        let text = std::fs::read_to_string(accounts.paths.config.join("accounts.toml")).unwrap();
        assert!(!text.contains("synthetic-refresh") && !text.contains("synthetic-access"));
        assert!(matches!(
            rotated.provider,
            AccountKind::Microsoft {
                persistence: CredentialPersistence::SessionOnly,
                system_credential: false,
                ..
            }
        ));
        assert!(
            Accounts::new(accounts.paths.clone())
                .session
                .lock()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn session_fallback_retains_system_credential_cleanup_requirement() {
        let temp = tempfile::tempdir().unwrap();
        let accounts = Accounts::new(AppPaths::discover(Some(temp.path())).unwrap());
        let mut first = accounts
            .persist_profile(profile("synthetic-refresh"), "test-client".into(), true)
            .await
            .unwrap();
        first.provider = AccountKind::Microsoft {
            client_id: "test-client".into(),
            persistence: CredentialPersistence::System,
            system_credential: true,
        };
        accounts.save_record(first).unwrap();
        let record = accounts
            .persist_profile(profile("rotated-secret"), "test-client".into(), true)
            .await
            .unwrap();
        assert!(matches!(
            record.provider,
            AccountKind::Microsoft {
                persistence: CredentialPersistence::SessionOnly,
                system_credential: true,
                ..
            }
        ));
    }
}

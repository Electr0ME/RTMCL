//! Future integrations implement these application-owned ports; no upstream types escape.
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::{
    Result,
    domain::{AccountId, JavaPolicy},
    tasks::TaskContext,
};

#[derive(Debug, Clone)]
pub struct AccountProfile {
    pub id: AccountId,
    pub display_name: String,
    pub minecraft_uuid: String,
}

/// Intentionally has no Debug or Serialize implementation. Never store in TOML.
pub struct RefreshToken(String);

impl RefreshToken {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPersistence {
    System,
    SessionOnly,
}

#[async_trait]
pub trait CredentialStore: Send + Sync {
    fn persistence(&self) -> CredentialPersistence;
    async fn load(&self, account: AccountId) -> Result<Option<RefreshToken>>;
    /// Must replace rotated tokens before reporting refresh success.
    async fn save(&self, account: AccountId, token: RefreshToken) -> Result<()>;
    async fn remove(&self, account: AccountId) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct DeviceCodePrompt {
    pub verification_uri: url::Url,
    pub user_code: String,
    pub expires_in_seconds: u64,
}

#[async_trait]
pub trait AccountService: Send + Sync {
    /// Prompt is delivered while polling continues; authentication bypasses download routing.
    async fn login(
        &self,
        prompt: tokio::sync::mpsc::Sender<DeviceCodePrompt>,
        context: TaskContext,
    ) -> Result<AccountProfile>;
    async fn refresh(&self, account: AccountId, context: TaskContext) -> Result<AccountProfile>;
    async fn logout(&self, account: AccountId) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct JavaRuntime {
    pub executable: PathBuf,
    pub major_version: u32,
    pub architecture: String,
}

#[async_trait]
pub trait JavaService: Send + Sync {
    /// Execute the binary to inspect version and architecture; existence is insufficient.
    async fn inspect(&self, executable: &Path) -> Result<JavaRuntime>;
    async fn resolve(
        &self,
        policy: &JavaPolicy,
        required_major: u32,
        context: TaskContext,
    ) -> Result<JavaRuntime>;
}

#[derive(Debug, Clone)]
pub struct GameSession {
    pub process_id: u32,
    pub log_file: PathBuf,
}

#[async_trait]
pub trait ProcessService: Send + Sync {
    /// Explicit termination must await OS-confirmed exit. Normal launcher exit is separate.
    async fn terminate(&self, session: &GameSession) -> Result<()>;
}

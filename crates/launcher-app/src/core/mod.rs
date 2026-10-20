//! Lighty stays behind this application-owned boundary.
use async_trait::async_trait;

use crate::{
    AppError, LauncherService, Result, accounts::Accounts, config::AppConfig,
    domain::InstanceConfig, network::Network, ports::GameSession, tasks::TaskContext,
};

pub const LIGHTY_VERSION: &str = "26.9.3";
pub const LIGHTY_REVISION: &str = "94bd0553beeb89fa7868eb0e0a9f446ad85af793";
pub const LIGHTY_RUST_VERSION: &str = "1.98";

#[derive(Debug, Clone)]
pub struct InstallRequest {
    pub instance: InstanceConfig,
    /// Immutable per-task policy snapshot; settings changes affect subsequent tasks.
    pub settings: AppConfig,
}

#[async_trait]
pub trait CoreAdapter: Send + Sync {
    async fn install(&self, request: InstallRequest, context: TaskContext) -> Result<()>;
    /// Credentials are resolved internally by account ID; UI never handles access tokens.
    async fn launch(&self, instance: InstanceConfig, context: TaskContext) -> Result<GameSession>;
}

pub struct LightyAdapter {
    service: LauncherService,
    accounts: Accounts,
}

impl LightyAdapter {
    pub fn new(service: LauncherService, accounts: Accounts) -> Self {
        Self { service, accounts }
    }
}

#[async_trait]
impl CoreAdapter for LightyAdapter {
    async fn install(&self, request: InstallRequest, context: TaskContext) -> Result<()> {
        let network = Network::new(
            self.service.paths().clone(),
            request.settings,
            context.clone(),
        )?;
        crate::p0::install(self.service.clone(), request.instance.id, network, context).await
    }

    async fn launch(&self, instance: InstanceConfig, context: TaskContext) -> Result<GameSession> {
        if instance.install_state != crate::domain::InstallState::Ready {
            return Err(AppError::Core("实例尚未完成安装".into()));
        }
        let account = instance.preferred_account.ok_or_else(|| {
            AppError::Account("实例尚未选择微软账户；P0 CLI 请使用 --account".into())
        })?;
        let network = Network::new(
            self.service.paths().clone(),
            self.service.settings()?,
            context.clone(),
        )?;
        let result = crate::p0::launch(
            self.service.clone(),
            instance.id,
            self.accounts.clone(),
            crate::p0::LaunchOptions {
                account,
                java: None,
                wait: false,
            },
            network,
            context,
        )
        .await?;
        Ok(GameSession {
            process_id: result.process_id,
            log_file: result.log_file,
        })
    }
}

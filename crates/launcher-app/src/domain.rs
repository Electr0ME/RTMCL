use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, Result};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }
    };
}

id_type!(InstanceId);
id_type!(TaskId);
id_type!(AccountId);

pub const SCHEMA_VERSION: u32 = 1;

/// Kept separate from the installation state, including while a game is running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    #[default]
    NotInstalled,
    Installing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Loader {
    #[default]
    Vanilla,
    Fabric {
        version: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaPolicy {
    #[default]
    Automatic,
    Executable {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchSettings {
    pub java: JavaPolicy,
    pub min_memory_mib: u32,
    pub max_memory_mib: u32,
    pub jvm_args: Vec<String>,
    pub window_width: u32,
    pub window_height: u32,
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self {
            java: JavaPolicy::Automatic,
            min_memory_mib: 512,
            max_memory_mib: 2048,
            jvm_args: Vec::new(),
            window_width: 854,
            window_height: 480,
        }
    }
}

impl LaunchSettings {
    pub fn validate(&self) -> Result<()> {
        if self.min_memory_mib == 0 || self.min_memory_mib > self.max_memory_mib {
            return Err(AppError::InvalidConfig("内存范围无效".into()));
        }
        if self.window_width == 0 || self.window_height == 0 {
            return Err(AppError::InvalidConfig("窗口尺寸必须大于零".into()));
        }
        if let JavaPolicy::Executable { path } = &self.java
            && !path.is_absolute()
        {
            return Err(AppError::InvalidConfig("Java 路径必须为绝对路径".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub schema_version: u32,
    pub id: InstanceId,
    pub name: String,
    pub game_version: String,
    pub loader: Loader,
    pub platform: String,
    #[serde(default)]
    pub install_state: InstallState,
    /// None means inherit global settings, rather than copying their current values.
    pub launch: Option<LaunchSettings>,
    pub preferred_account: Option<AccountId>,
}

impl InstanceConfig {
    pub fn new(name: String, game_version: String, loader: Loader) -> Result<Self> {
        let instance = Self {
            schema_version: SCHEMA_VERSION,
            id: InstanceId::new(),
            name,
            game_version,
            loader,
            platform: current_platform(),
            install_state: InstallState::NotInstalled,
            launch: None,
            preferred_account: None,
        };
        instance.validate()?;
        Ok(instance)
    }

    pub fn validate(&self) -> Result<()> {
        check_schema(self.schema_version)?;
        if self.name.trim().is_empty() || self.game_version.trim().is_empty() {
            return Err(AppError::InvalidConfig("实例名称和游戏版本不能为空".into()));
        }
        if let Loader::Fabric { version } = &self.loader
            && version.trim().is_empty()
        {
            return Err(AppError::InvalidConfig("Fabric 必须指定加载器版本".into()));
        }
        if self.platform != current_platform() {
            return Err(AppError::InvalidConfig(format!(
                "实例属于 {}，当前平台为 {}，不能共用可变实例目录",
                self.platform,
                current_platform()
            )));
        }
        if let Some(settings) = &self.launch {
            settings.validate()?;
        }
        Ok(())
    }
}

pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Migration dispatch point. No older on-disk schema exists yet.
pub fn check_schema(found: u32) -> Result<()> {
    if found != SCHEMA_VERSION {
        return Err(AppError::UnsupportedSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(())
}

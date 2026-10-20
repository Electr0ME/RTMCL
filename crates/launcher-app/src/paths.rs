use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::{AppError, Result, domain::InstanceId, error::io_error};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppPaths {
    pub config: PathBuf,
    pub instances: PathBuf,
    pub cache: PathBuf,
    pub runtimes: PathBuf,
    pub logs: PathBuf,
}

impl AppPaths {
    pub fn discover(override_root: Option<&Path>) -> Result<Self> {
        if let Some(root) = override_root {
            let root = if root.is_absolute() {
                root.to_owned()
            } else {
                std::env::current_dir()
                    .map_err(|e| io_error("读取工作目录", root, e))?
                    .join(root)
            };
            return Ok(Self {
                config: root.join("config"),
                instances: root.join("instances"),
                cache: root.join("cache"),
                runtimes: root.join("runtimes"),
                logs: root.join("logs"),
            });
        }
        let name = if crate::accounts::DEVELOPMENT_LOCAL_ACCOUNTS {
            "rtmcl-development"
        } else {
            "rtmcl"
        };
        let dirs = ProjectDirs::from("", "", name).ok_or(AppError::MissingDataDirectory)?;
        Ok(Self {
            config: dirs.config_dir().to_owned(),
            instances: dirs.data_local_dir().join("instances"),
            cache: dirs.cache_dir().to_owned(),
            runtimes: dirs.data_local_dir().join("runtimes"),
            logs: dirs.data_local_dir().join("logs"),
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    pub fn instance_dir(&self, id: InstanceId) -> PathBuf {
        self.instances.join(id.to_string())
    }

    pub fn ensure(&self) -> Result<()> {
        for path in [
            &self.config,
            &self.instances,
            &self.cache,
            &self.runtimes,
            &self.logs,
        ] {
            std::fs::create_dir_all(path).map_err(|e| io_error("创建应用目录", path, e))?;
        }
        Ok(())
    }
}

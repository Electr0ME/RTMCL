use std::io::ErrorKind;

use crate::{
    AppError, AppPaths, Result,
    config::AppConfig,
    domain::{InstanceConfig, Loader},
    error::io_error,
    storage::{FileLock, read_toml, write_toml},
};

#[derive(Debug, Clone)]
pub struct LauncherService {
    paths: AppPaths,
}

impl LauncherService {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn instance(&self, id: crate::domain::InstanceId) -> Result<InstanceConfig> {
        let instance: InstanceConfig =
            read_toml(&self.paths.instance_dir(id).join("instance.toml"))?;
        instance.validate()?;
        if instance.id != id {
            return Err(AppError::InvalidConfig("实例目录与 ID 不匹配".into()));
        }
        Ok(instance)
    }

    /// Caller must hold the instance operation lock for read/modify/write operations.
    pub fn save_instance(&self, instance: &InstanceConfig) -> Result<()> {
        instance.validate()?;
        write_toml(
            &self.paths.instance_dir(instance.id).join("instance.toml"),
            instance,
        )
    }

    /// Read-only; absent settings use explicit scaffold defaults without writing files.
    pub fn settings(&self) -> Result<AppConfig> {
        let config = match read_toml::<AppConfig>(&self.paths.settings_file()) {
            Ok(config) => config,
            Err(AppError::Io { source, .. }) if source.kind() == ErrorKind::NotFound => {
                AppConfig::default()
            }
            Err(error) => return Err(error),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn initialize(&self) -> Result<AppConfig> {
        self.paths.ensure()?;
        let _lock = FileLock::acquire(&self.paths.config.join("settings.lock"))?;
        let config = self.settings()?;
        if !self.paths.settings_file().exists() {
            write_toml(&self.paths.settings_file(), &config)?;
        }
        Ok(config)
    }

    /// Configuration only. A newly created instance is always NotInstalled.
    pub fn create_instance(
        &self,
        name: String,
        game_version: String,
        loader: Loader,
    ) -> Result<InstanceConfig> {
        let instance = InstanceConfig::new(name, game_version, loader)?;
        self.initialize()?;
        // Readers and creators share a short catalogue lock to avoid exposing partial creation.
        let _lock = FileLock::acquire(&self.paths.instances.join("catalogue.lock"))?;
        let dir = self.paths.instance_dir(instance.id);
        std::fs::create_dir(&dir).map_err(|e| io_error("创建实例目录", &dir, e))?;
        let result = (|| {
            std::fs::create_dir(dir.join("minecraft"))
                .map_err(|e| io_error("创建游戏目录", &dir, e))?;
            write_toml(&dir.join("instance.toml"), &instance)
        })();
        if let Err(error) = result {
            // This UUID directory was just created here and cannot contain user game data.
            let _ = std::fs::remove_dir_all(&dir);
            return Err(error);
        }
        Ok(instance)
    }

    pub fn list_instances(&self) -> Result<Vec<InstanceConfig>> {
        let entries = match std::fs::read_dir(&self.paths.instances) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error("读取实例目录", &self.paths.instances, error)),
        };
        let _lock = FileLock::acquire(&self.paths.instances.join("catalogue.lock"))?;
        let mut instances = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| io_error("读取实例目录项", &self.paths.instances, e))?;
            let path = entry.path();
            if !entry
                .file_type()
                .map_err(|e| io_error("读取目录类型", &path, e))?
                .is_dir()
            {
                continue;
            }
            let instance: InstanceConfig = read_toml(&path.join("instance.toml"))?;
            instance.validate()?;
            if entry.file_name() != instance.id.to_string().as_str() {
                return Err(AppError::InvalidConfig(format!(
                    "实例 ID 与目录不匹配: {}",
                    path.display()
                )));
            }
            instances.push(instance);
        }
        instances.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(instances)
    }
}

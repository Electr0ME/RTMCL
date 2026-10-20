use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    AppError, Result,
    domain::{LaunchSettings, SCHEMA_VERSION, check_schema},
    downloads::SourceId,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    #[default]
    Ordered,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    pub mode: SourceMode,
    pub sources: Vec<SourceId>,
    pub max_concurrent_per_host: usize,
    pub retries_per_source: u32,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            mode: SourceMode::Ordered,
            sources: vec![SourceId::Official, SourceId::Bmclapi],
            max_concurrent_per_host: 8,
            retries_per_source: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorSettings {
    pub base_url: Url,
}

impl Default for MirrorSettings {
    fn default() -> Self {
        Self {
            base_url: Url::parse("https://bmclapi2.bangbang93.com")
                .expect("static BMCLAPI base URL is valid"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Mirrors {
    pub bmclapi: MirrorSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    // Required on disk: missing versions must not silently become the current schema.
    pub schema_version: u32,
    #[serde(default)]
    pub downloads: DownloadSettings,
    #[serde(default)]
    pub mirrors: Mirrors,
    #[serde(default)]
    pub launch: LaunchSettings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            downloads: DownloadSettings::default(),
            mirrors: Mirrors::default(),
            launch: LaunchSettings::default(),
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        check_schema(self.schema_version)?;
        let downloads = &self.downloads;
        if downloads.sources.is_empty()
            || downloads.max_concurrent_per_host == 0
            || downloads.max_concurrent_per_host > 64
            || downloads.retries_per_source > 10
        {
            return Err(AppError::InvalidConfig(
                "下载来源、并发或重试上限无效".into(),
            ));
        }
        if downloads.mode == SourceMode::Strict && downloads.sources.len() != 1 {
            return Err(AppError::InvalidConfig(
                "严格模式必须且只能指定一个来源".into(),
            ));
        }
        for (index, source) in downloads.sources.iter().enumerate() {
            if downloads.sources[..index].contains(source) {
                return Err(AppError::InvalidConfig("下载来源不能重复".into()));
            }
        }
        let base = &self.mirrors.bmclapi.base_url;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(AppError::InvalidConfig(
                "镜像基地址须为不含凭据、查询及片段的 HTTP(S) 地址".into(),
            ));
        }
        self.launch.validate()
    }
}

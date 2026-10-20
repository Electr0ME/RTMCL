//! Download ports only. Authentication must use a separate client outside this module.
use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Result, tasks::TaskContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceId {
    Official,
    Bmclapi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    VersionManifest,
    VersionMetadata,
    AssetIndex,
    AssetObject,
    Client,
    LoggingConfig,
    Library,
    FabricMetadata,
    FabricMaven,
    JavaMetadata,
    JavaArchive,
}

/// Identity and verification data never change when switching a download location.
#[derive(Debug, Clone)]
pub struct ResourceRequest {
    pub kind: ResourceKind,
    pub logical_id: String,
    pub original_url: Url,
    pub expected_sha1: Option<String>,
    pub expected_size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SourceCandidate {
    pub source: SourceId,
    pub url: Url,
}

pub trait MirrorProvider: Send + Sync {
    fn source(&self) -> SourceId;
    /// Unsupported resources return None; never guess a mapping for unknown hosts.
    fn candidate(&self, request: &ResourceRequest) -> Option<SourceCandidate>;
}

pub trait SourceRouter: Send + Sync {
    /// Strict source selection must return an error when no candidate is supported.
    fn candidates(&self, request: &ResourceRequest) -> Result<Vec<SourceCandidate>>;
}

#[async_trait]
pub trait DownloadExecutor: Send + Sync {
    /// Implementations must verify and atomically commit before returning success.
    /// Switching sources discards partial files; cancellation waits for cleanup.
    async fn download(
        &self,
        request: &ResourceRequest,
        candidates: &[SourceCandidate],
        target: &Path,
        context: TaskContext,
    ) -> Result<()>;
}

#[async_trait]
pub trait MetadataService: Send + Sync {
    /// Cache implementations must retain origin, timestamps, validators and schema.
    async fn fetch(&self, request: &ResourceRequest, context: TaskContext) -> Result<Vec<u8>>;
}

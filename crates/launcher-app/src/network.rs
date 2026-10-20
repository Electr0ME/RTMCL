//! Source policy and verified downloads. Contains no authentication client or tokens.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use reqwest::{
    Client, StatusCode,
    header::{CONTENT_RANGE, RANGE, RETRY_AFTER},
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, Semaphore},
};
use url::Url;

use crate::{
    AppError, AppPaths, Result,
    config::AppConfig,
    downloads::{ResourceKind, ResourceRequest, SourceCandidate, SourceId},
    error::io_error,
    storage::{FileLock, read_toml, write_toml},
    tasks::{DownloadProgress, TaskContext},
};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub fn resource(url: &str, sha1: Option<&str>, size: Option<u64>) -> Result<ResourceRequest> {
    let url = Url::parse(url).map_err(|_| AppError::Network("资源 URL 无效".into()))?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::Network(
            "下载仅接受不带凭据的 HTTP(S) 地址".into(),
        ));
    }
    if sha1.is_some_and(|hash| hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(AppError::Network("资源 SHA1 无效".into()));
    }
    let kind = match url.host_str().unwrap_or_default() {
        "resources.download.minecraft.net" => ResourceKind::AssetObject,
        "libraries.minecraft.net" => ResourceKind::Library,
        "maven.fabricmc.net" => ResourceKind::FabricMaven,
        "meta.fabricmc.net" => ResourceKind::FabricMetadata,
        "piston-meta.mojang.com" | "launchermeta.mojang.com"
            if url.path().contains("version_manifest") =>
        {
            ResourceKind::VersionManifest
        }
        "piston-meta.mojang.com" | "launchermeta.mojang.com" => ResourceKind::VersionMetadata,
        _ => ResourceKind::Client,
    };
    Ok(ResourceRequest {
        kind,
        logical_id: url.to_string(),
        original_url: url,
        expected_sha1: sha1.map(str::to_ascii_lowercase),
        expected_size: size,
    })
}

/// Exact host/path mapping; authentication and unknown origins never receive mirror URLs.
pub fn candidates(config: &AppConfig, request: &ResourceRequest) -> Result<Vec<SourceCandidate>> {
    config.validate()?;
    let original = &request.original_url;
    let mirror_prefix = match (original.scheme(), original.host_str()) {
        ("https", Some("resources.download.minecraft.net")) => Some("/assets"),
        ("https", Some("libraries.minecraft.net" | "maven.fabricmc.net")) => Some("/maven"),
        ("https", Some("meta.fabricmc.net")) => Some("/fabric-meta"),
        (
            "https",
            Some(
                "piston-meta.mojang.com"
                | "launchermeta.mojang.com"
                | "piston-data.mojang.com"
                | "launcher.mojang.com",
            ),
        ) => Some(""),
        _ => None,
    };
    let mut result = Vec::new();
    for source in &config.downloads.sources {
        let url = match source {
            SourceId::Official => original.clone(),
            SourceId::Bmclapi => {
                let Some(prefix) = mirror_prefix else {
                    continue;
                };
                let mut url = config.mirrors.bmclapi.base_url.clone();
                let path = format!(
                    "{}{}{}",
                    url.path().trim_end_matches('/'),
                    prefix,
                    original.path()
                );
                url.set_path(&path);
                url.set_query(original.query());
                url
            }
        };
        result.push(SourceCandidate {
            source: *source,
            url,
        });
    }
    if result.is_empty() {
        return Err(AppError::Network(format!(
            "所选来源不支持该资源: {}（严格策略未回退）",
            original.host_str().unwrap_or_default()
        )));
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub schema_version: u32,
    pub logical_id: String,
    pub original_url: String,
    pub actual_url: String,
    pub source: SourceId,
    pub expected_sha1: Option<String>,
    pub observed_sha1: String,
    pub size: u64,
    pub fetched_at: u64,
}

#[derive(Clone)]
pub struct Network {
    client: Client,
    paths: AppPaths,
    config: AppConfig,
    task: TaskContext,
    hosts: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl Network {
    pub fn new(paths: AppPaths, config: AppConfig, task: TaskContext) -> Result<Self> {
        config.validate()?;
        let client = Client::builder()
            .user_agent("RTMCL/0.1.0 (Minecraft launcher; BMCLAPI support)")
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map_err(|e| AppError::Network(e.to_string()))?;
        Ok(Self {
            client,
            paths,
            config,
            task,
            hosts: Arc::default(),
        })
    }

    pub fn policy(&self) -> &AppConfig {
        &self.config
    }

    pub async fn fetch(&self, url: &str, sha1: Option<&str>) -> Result<Vec<u8>> {
        let request = resource(url, sha1, None)?;
        let key = hex_sha1(url.as_bytes());
        let path = self.paths.cache.join("metadata").join(key);
        // Immutable hash-addressed documents are reusable; mutable manifests have a 5 min TTL.
        if sha1.is_none()
            && let Ok(record) = read_toml::<DownloadRecord>(&sidecar(&path, "verified.toml"))
            && now().saturating_sub(record.fetched_at) >= 300
        {
            let _ = tokio::fs::remove_file(sidecar(&path, "verified.toml")).await;
        }
        self.download(&request, &path).await?;
        let size = tokio::fs::metadata(&path)
            .await
            .map_err(|e| io_error("读取元数据大小", &path, e))?
            .len();
        if size > 64 * 1024 * 1024 {
            return Err(AppError::Network("元数据超过 64 MiB 限制".into()));
        }
        tokio::fs::read(&path)
            .await
            .map_err(|e| io_error("读取元数据", path, e))
    }

    pub async fn download(
        &self,
        request: &ResourceRequest,
        target: &Path,
    ) -> Result<DownloadRecord> {
        let sources = candidates(&self.config, request)?;
        self.download_candidates(request, target, &sources).await
    }

    async fn download_candidates(
        &self,
        request: &ResourceRequest,
        target: &Path,
        sources: &[SourceCandidate],
    ) -> Result<DownloadRecord> {
        let token = self.task.child_token();
        if token.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let parent = target
            .parent()
            .ok_or_else(|| AppError::Network("下载目标缺少父目录".into()))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| io_error("创建下载目录", parent, e))?;
        // Cross-process cache lock. Parent instance lock is held by the install operation.
        let _lock = FileLock::acquire(&sidecar(target, "lock"))?;
        if let Ok(record) = read_toml::<DownloadRecord>(&sidecar(target, "verified.toml"))
            && record.logical_id == request.logical_id
            && record.expected_sha1 == request.expected_sha1
            && request
                .expected_size
                .is_none_or(|expected| record.size == expected)
            && self
                .valid_file(target, Some(&record.observed_sha1), Some(record.size))
                .await?
        {
            return Ok(record);
        }
        if request.expected_sha1.is_some()
            && self
                .valid_file(
                    target,
                    request.expected_sha1.as_deref(),
                    request.expected_size,
                )
                .await?
        {
            let record = DownloadRecord {
                schema_version: 1,
                logical_id: request.logical_id.clone(),
                original_url: request.original_url.to_string(),
                actual_url: request.original_url.to_string(),
                source: SourceId::Official,
                expected_sha1: request.expected_sha1.clone(),
                observed_sha1: request.expected_sha1.clone().unwrap(),
                size: tokio::fs::metadata(target)
                    .await
                    .map_err(|e| io_error("读取已校验文件", target, e))?
                    .len(),
                fetched_at: now(),
            };
            write_toml(&sidecar(target, "verified.toml"), &record)?;
            return Ok(record);
        }
        let mut failures = Vec::new();
        for (index, candidate) in sources.iter().enumerate() {
            if index > 0 {
                self.discard_partial(target).await;
            }
            for attempt in 0..=self.config.downloads.retries_per_source {
                if token.is_cancelled() {
                    return Err(AppError::Cancelled);
                }
                match self.attempt(request, candidate, target).await {
                    Ok(record) => return Ok(record),
                    Err(AppError::Cancelled) => return Err(AppError::Cancelled),
                    Err(error) => failures.push(format!(
                        "{:?} 第 {} 次: {error}",
                        candidate.source,
                        attempt + 1
                    )),
                }
                if attempt < self.config.downloads.retries_per_source {
                    tokio::select! { _ = token.cancelled() => return Err(AppError::Cancelled), _ = tokio::time::sleep(Duration::from_millis(250 * (1u64 << attempt.min(5)))) => {} }
                }
            }
        }
        Err(AppError::Network(failures.join("; ")))
    }

    async fn attempt(
        &self,
        request: &ResourceRequest,
        candidate: &SourceCandidate,
        target: &Path,
    ) -> Result<DownloadRecord> {
        let token = self.task.child_token();
        let host = candidate.url.host_str().unwrap_or_default().to_owned();
        let semaphore = self
            .hosts
            .lock()
            .await
            .entry(host)
            .or_insert_with(|| {
                Arc::new(Semaphore::new(
                    self.config.downloads.max_concurrent_per_host,
                ))
            })
            .clone();
        let _permit = tokio::select! { _ = token.cancelled() => return Err(AppError::Cancelled), permit = semaphore.acquire() => permit.map_err(|_| AppError::Network("下载调度已关闭".into()))? };
        let part = sidecar(target, "part");
        let identity = format!(
            "{}\n{}\n{:?}\n{:?}",
            request.logical_id, candidate.url, request.expected_sha1, request.expected_size
        );
        let identity_path = sidecar(target, "part.identity");
        let reusable = request.expected_sha1.is_some()
            && tokio::fs::read_to_string(&identity_path)
                .await
                .ok()
                .as_deref()
                == Some(identity.as_str());
        if !reusable {
            self.discard_partial(target).await;
        }
        tokio::fs::write(&identity_path, identity)
            .await
            .map_err(|e| io_error("保存续传身份", &identity_path, e))?;
        let offset = if reusable {
            tokio::fs::metadata(&part)
                .await
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };
        let mut http_request = self.client.get(candidate.url.clone());
        if offset > 0 {
            http_request = http_request.header(RANGE, format!("bytes={offset}-"));
        }
        let response = tokio::select! { _ = token.cancelled() => return Err(AppError::Cancelled), response = http_request.send() => response.map_err(|e| AppError::Network(e.without_url().to_string()))? };
        let status = response.status();
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            self.discard_partial(target).await;
            return Err(AppError::Network(
                "HTTP 416，已丢弃临时文件，下次完整重试".into(),
            ));
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let seconds = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1)
                .min(60);
            tokio::select! { _ = token.cancelled() => return Err(AppError::Cancelled), _ = tokio::time::sleep(Duration::from_secs(seconds)) => {} }
            return Err(AppError::Network(
                "HTTP 429（已按 Retry-After 冷却）".into(),
            ));
        }
        if !status.is_success() {
            return Err(AppError::Network(format!("HTTP {}", status.as_u16())));
        }
        let append = status == StatusCode::PARTIAL_CONTENT && offset > 0;
        let mut range_total = None;
        if status == StatusCode::PARTIAL_CONTENT {
            let text = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            let (start, end, total) = parse_content_range(text)
                .ok_or_else(|| AppError::Network("206 缺少有效 Content-Range".into()))?;
            if start != offset
                || end < start
                || total <= end
                || request.expected_size.is_some_and(|size| size != total)
            {
                self.discard_partial(target).await;
                return Err(AppError::Network("206 Content-Range 与资源不一致".into()));
            }
            range_total = Some(total);
        }
        let length = response.content_length();
        let total = request
            .expected_size
            .or(range_total)
            .or(length.map(|n| n + if append { offset } else { 0 }));
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part)
            .await
            .map_err(|e| io_error("打开下载临时文件", &part, e))?;
        let mut stream = response.bytes_stream();
        let mut current = if append { offset } else { 0 };
        let mut last_update = tokio::time::Instant::now() - Duration::from_secs(1);
        loop {
            let chunk = tokio::select! { _ = token.cancelled() => { file.flush().await.map_err(|e| io_error("保存取消进度", &part, e))?; return Err(AppError::Cancelled); }, chunk = stream.next() => chunk };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| AppError::Network(e.without_url().to_string()))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| io_error("写入临时文件", &part, e))?;
            current += chunk.len() as u64;
            if total.is_some_and(|n| current > n) {
                drop(file);
                self.discard_partial(target).await;
                return Err(AppError::Network("收到的数据超过声明大小".into()));
            }
            if last_update.elapsed() >= Duration::from_millis(100) {
                self.task.progress(DownloadProgress {
                    downloaded_bytes: current,
                    total_bytes: total,
                    source: Some(candidate.source),
                })?;
                last_update = tokio::time::Instant::now();
            }
        }
        file.flush()
            .await
            .map_err(|e| io_error("刷新下载临时文件", &part, e))?;
        file.sync_all()
            .await
            .map_err(|e| io_error("同步下载临时文件", &part, e))?;
        drop(file);
        if total.is_some_and(|n| n != current) {
            return Err(AppError::Network("传输未完成（长度不匹配）".into()));
        }
        // Chunked HTTP metadata may omit Content-Length. reqwest only ends this stream
        // successfully after complete HTTP framing; record the observed size and checksum.
        if total.is_none() && request.expected_sha1.is_none() && current == 0 {
            self.discard_partial(target).await;
            return Err(AppError::Network(
                "资源没有可信哈希且传输内容为空，拒绝提交".into(),
            ));
        }
        let observed = self.hash_file(&part).await?;
        if request
            .expected_sha1
            .as_ref()
            .is_some_and(|expected| !observed.eq_ignore_ascii_case(expected))
        {
            self.discard_partial(target).await;
            return Err(AppError::Network("SHA1 校验失败，损坏内容未提交".into()));
        }
        if token.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let commit_from =
            std::fs::canonicalize(&part).map_err(|e| io_error("解析临时文件路径", &part, e))?;
        let commit_to = std::fs::canonicalize(target.parent().expect("download target has parent"))
            .map_err(|e| io_error("解析文件提交目录", target, e))?
            .join(target.file_name().expect("download target has name"));
        // tempfile persist provides replace semantics on both Windows and Unix.
        tokio::task::spawn_blocking(move || {
            let temporary = tempfile::NamedTempFile::from_parts(
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&commit_from)?,
                tempfile::TempPath::try_from_path(commit_from)?,
            );
            temporary.persist(&commit_to).map_err(|e| e.error)?;
            Ok::<(), std::io::Error>(())
        })
        .await
        .map_err(|_| AppError::Network("文件提交任务失败".into()))?
        .map_err(|e| io_error("提交已校验文件", target, e))?;
        let _ = tokio::fs::remove_file(identity_path).await;
        let record = DownloadRecord {
            schema_version: 1,
            logical_id: request.logical_id.clone(),
            original_url: request.original_url.to_string(),
            actual_url: candidate.url.to_string(),
            source: candidate.source,
            expected_sha1: request.expected_sha1.clone(),
            observed_sha1: observed,
            size: current,
            fetched_at: now(),
        };
        write_toml(&sidecar(target, "verified.toml"), &record)?;
        self.task.progress(DownloadProgress {
            downloaded_bytes: current,
            total_bytes: Some(current),
            source: Some(candidate.source),
        })?;
        Ok(record)
    }

    async fn valid_file(&self, path: &Path, hash: Option<&str>, size: Option<u64>) -> Result<bool> {
        let metadata = match tokio::fs::metadata(path).await {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(io_error("读取缓存文件", path, e)),
        };
        if size.is_some_and(|n| n != metadata.len()) {
            return Ok(false);
        }
        match hash {
            Some(hash) => Ok(self.hash_file(path).await?.eq_ignore_ascii_case(hash)),
            None => Ok(false),
        }
    }

    async fn hash_file(&self, path: &Path) -> Result<String> {
        let token = self.task.child_token();
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|e| io_error("打开校验文件", path, e))?;
        let mut hash = Sha1::new();
        let mut buffer = vec![0; 128 * 1024];
        loop {
            if token.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let count = file
                .read(&mut buffer)
                .await
                .map_err(|e| io_error("读取校验文件", path, e))?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        Ok(hex::encode(hash.finalize()))
    }

    async fn discard_partial(&self, target: &Path) {
        let _ = tokio::fs::remove_file(sidecar(target, "part")).await;
        let _ = tokio::fs::remove_file(sidecar(target, "part.identity")).await;
    }
}

impl lighty_launcher::_core::download_context::DownloadContext for Network {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        sha1: Option<&'a str>,
    ) -> lighty_launcher::_core::download_context::FetchFuture<'a> {
        Box::pin(async move { self.fetch(url, sha1).await.map_err(std::io::Error::other) })
    }
}

impl crate::downloads::SourceRouter for AppConfig {
    fn candidates(&self, request: &ResourceRequest) -> Result<Vec<SourceCandidate>> {
        candidates(self, request)
    }
}

impl crate::downloads::MirrorProvider for crate::config::MirrorSettings {
    fn source(&self) -> SourceId {
        SourceId::Bmclapi
    }
    fn candidate(&self, request: &ResourceRequest) -> Option<SourceCandidate> {
        let mut config = AppConfig::default();
        config.downloads.mode = crate::config::SourceMode::Strict;
        config.downloads.sources = vec![SourceId::Bmclapi];
        config.mirrors.bmclapi = self.clone();
        candidates(&config, request).ok()?.into_iter().next()
    }
}

#[async_trait::async_trait]
impl crate::downloads::DownloadExecutor for Network {
    async fn download(
        &self,
        request: &ResourceRequest,
        candidates: &[SourceCandidate],
        target: &Path,
        context: TaskContext,
    ) -> Result<()> {
        if candidates.is_empty() {
            return Err(AppError::Network("没有可用下载候选来源".into()));
        }
        let mut network = self.clone();
        network.task = context;
        network
            .download_candidates(request, target, candidates)
            .await
            .map(|_| ())
    }
}

#[async_trait::async_trait]
impl crate::downloads::MetadataService for Network {
    async fn fetch(&self, request: &ResourceRequest, context: TaskContext) -> Result<Vec<u8>> {
        let mut network = self.clone();
        network.task = context;
        network
            .fetch(
                request.original_url.as_str(),
                request.expected_sha1.as_deref(),
            )
            .await
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{suffix}"));
    name.into()
}

pub fn hex_sha1(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;

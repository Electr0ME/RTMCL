//! Blocking filesystem operations: call from a worker, never from the TUI loop.
use std::{fs::File, io::Write, path::Path};

use serde::{Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

use crate::{Result, domain::check_schema, error::io_error};

/// Hold this guard for the entire operation, including cleanup on cancellation.
pub struct FileLock {
    _file: File,
}

impl FileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| io_error("打开操作锁", path, e))?;
        file.try_lock().map_err(|e| {
            io_error(
                "获取操作锁（可能有其他进程正在操作）",
                path,
                std::io::Error::other(e),
            )
        })?;
        Ok(Self { _file: file })
    }
}

pub fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|e| io_error("读取配置", path, e))?;
    // Inspect schema before decoding domain fields: newer formats may change those fields.
    #[derive(serde::Deserialize)]
    struct Header {
        schema_version: u32,
    }
    let header: Header = toml::from_str(&text)?;
    check_schema(header.schema_version)?;
    Ok(toml::from_str(&text)?)
}

/// Caller holds the appropriate lock. Temp and destination stay on the same filesystem.
pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = toml::to_string_pretty(value)?;
    write_bytes(path, text.as_bytes())
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().expect("configuration path has a parent");
    // tempfile's Windows rename requires an extended-length destination for deep Maven paths.
    let resolved_parent =
        std::fs::canonicalize(parent).map_err(|e| io_error("解析提交目录", parent, e))?;
    let destination = resolved_parent.join(path.file_name().expect("file has a name"));
    let mut temp = NamedTempFile::new_in(&resolved_parent)
        .map_err(|e| io_error("创建配置临时文件", path, e))?;
    temp.write_all(bytes)
        .map_err(|e| io_error("写入配置临时文件", path, e))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| io_error("同步配置临时文件", path, e))?;
    temp.persist(&destination)
        .map_err(|e| io_error("原子提交配置", path, e.error))?;
    // Unix directory fsync makes the rename durable across a system crash.
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| io_error("同步配置目录", parent, e))?;
    Ok(())
}

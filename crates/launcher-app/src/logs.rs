//! Bounded, read-only access to the latest launcher-captured game log.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

use crate::{AppError, LauncherService, Result, domain::InstanceId, error::io_error};

const MAX_TAIL_BYTES: u64 = 1024 * 1024;

pub struct GameLogTail {
    pub path: PathBuf,
    pub text: String,
    pub byte_limit_reached: bool,
}

impl LauncherService {
    /// Snapshot only: does not take the game lock or alter the running process.
    pub fn game_log_tail(&self, instance: InstanceId, lines: usize) -> Result<GameLogTail> {
        if !(1..=10_000).contains(&lines) {
            return Err(AppError::InvalidConfig("日志行数须在 1–10000 之间".into()));
        }
        self.instance(instance)?;
        let missing = || AppError::Core("该实例尚无游戏日志，请先启动游戏".into());
        let entries = match std::fs::read_dir(&self.paths().logs) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(missing()),
            Err(error) => return Err(io_error("读取日志目录", &self.paths().logs, error)),
        };
        let prefix = format!("game-{instance}-");
        let mut latest = None;
        for entry in entries {
            let entry = entry.map_err(|e| io_error("读取日志目录项", &self.paths().logs, e))?;
            let path = entry.path();
            let name = entry.file_name();
            let Some(session) = name
                .to_str()
                .and_then(|name| name.strip_prefix(&prefix))
                .and_then(|name| name.strip_suffix(".log"))
            else {
                continue;
            };
            if uuid::Uuid::parse_str(session).is_err()
                || !entry
                    .file_type()
                    .map_err(|e| io_error("读取日志类型", &path, e))?
                    .is_file()
            {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .map_err(|e| io_error("读取日志时间", &path, e))?;
            let candidate = (modified, path);
            if latest.as_ref().is_none_or(|current| &candidate > current) {
                latest = Some(candidate);
            }
        }
        let (_, path) = latest.ok_or_else(missing)?;
        let (text, byte_limit_reached) = tail(&path, lines)?;
        Ok(GameLogTail {
            path,
            text,
            byte_limit_reached,
        })
    }
}

fn tail(path: &std::path::Path, lines: usize) -> Result<(String, bool)> {
    let mut file = File::open(path).map_err(|e| io_error("打开游戏日志", path, e))?;
    let length = file
        .seek(SeekFrom::End(0))
        .map_err(|e| io_error("定位游戏日志末尾", path, e))?;
    let start = length.saturating_sub(MAX_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| io_error("定位日志片段", path, e))?;
    let mut bytes = Vec::new();
    file.take(length - start)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("读取游戏日志", path, e))?;
    let mut partial = false;
    if start > 0 {
        if let Some(newline) = bytes.iter().position(|b| *b == b'\n') {
            bytes.drain(..=newline);
        } else {
            partial = true;
        }
    }
    let decoded = String::from_utf8_lossy(&bytes);
    let mut selected: Vec<_> = decoded.split_inclusive('\n').rev().take(lines).collect();
    let limited = start > 0 && (partial || selected.len() < lines);
    selected.reverse();
    Ok((selected.concat(), limited))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppPaths, domain::Loader};

    #[test]
    fn latest_log_is_scoped_to_instance_and_does_not_need_the_game_lock() {
        let root = tempfile::tempdir().unwrap();
        let service = LauncherService::new(AppPaths::discover(Some(root.path())).unwrap());
        let instance = service
            .create_instance("日志测试".into(), "1.21.1".into(), Loader::Vanilla)
            .unwrap();
        assert!(service.game_log_tail(instance.id, 10).is_err());
        let path =
            service
                .paths()
                .logs
                .join(format!("game-{}-{}.log", instance.id, uuid::Uuid::new_v4()));
        std::fs::write(&path, "第一行\r\n第二行\r\n末行").unwrap();
        let old_path =
            service
                .paths()
                .logs
                .join(format!("game-{}-{}.log", instance.id, uuid::Uuid::new_v4()));
        std::fs::write(&old_path, "old session").unwrap();
        File::options()
            .write(true)
            .open(old_path)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86400))
            .unwrap();
        std::fs::write(service.paths().logs.join("launcher.log"), "wrong").unwrap();
        std::fs::write(
            service
                .paths()
                .logs
                .join(format!("game-{}-bad.log", instance.id)),
            "wrong",
        )
        .unwrap();
        std::fs::write(
            service.paths().logs.join(format!(
                "game-{}-{}.log",
                InstanceId::new(),
                uuid::Uuid::new_v4()
            )),
            "wrong",
        )
        .unwrap();
        let _lock = crate::storage::FileLock::acquire(
            &service
                .paths()
                .instance_dir(instance.id)
                .join("operation.lock"),
        )
        .unwrap();
        let result = service.game_log_tail(instance.id, 2).unwrap();
        assert_eq!(result.path, path);
        assert_eq!(result.text, "第二行\r\n末行");
        assert!(!result.byte_limit_reached);
        assert!(service.game_log_tail(instance.id, 0).is_err());
    }

    #[test]
    fn oversized_or_non_utf8_logs_are_bounded_and_readable() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.log");
        std::fs::write(&path, vec![b'x'; MAX_TAIL_BYTES as usize + 20]).unwrap();
        let (text, limited) = tail(&path, 100).unwrap();
        assert_eq!(text.len(), MAX_TAIL_BYTES as usize);
        assert!(limited);
        std::fs::write(&path, b"old\n\xff\nlast\n").unwrap();
        assert_eq!(tail(&path, 2).unwrap().0, "\u{fffd}\nlast\n");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(tail(&path, 2).unwrap().0, "");
    }
}

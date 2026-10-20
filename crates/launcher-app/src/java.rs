use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::{
    AppError, AppPaths, Result,
    domain::JavaPolicy,
    error::io_error,
    network::{Network, resource},
    ports::JavaRuntime,
    storage::FileLock,
    tasks::TaskContext,
};

pub async fn inspect(path: &Path) -> Result<JavaRuntime> {
    let path = tokio::fs::canonicalize(path)
        .await
        .map_err(|e| io_error("解析 Java 路径", path, e))?;
    let mut command = tokio::process::Command::new(&path);
    command
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| AppError::Java("Java 探测超时".into()))?
        .map_err(|e| io_error("执行 Java 探测", &path, e))?;
    if !output.status.success() {
        return Err(AppError::Java("Java 探测返回失败退出码".into()));
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let version = property(&text, "java.version")
        .ok_or_else(|| AppError::Java("无法解析 java.version".into()))?;
    let mut components = version.split(['.', '-', '+']);
    let first: u32 = components
        .next()
        .unwrap_or_default()
        .parse()
        .map_err(|_| AppError::Java("Java 版本格式无效".into()))?;
    let major_version = if first == 1 {
        components
            .next()
            .unwrap_or_default()
            .parse()
            .map_err(|_| AppError::Java("旧版 Java 版本格式无效".into()))?
    } else {
        first
    };
    let architecture = match property(&text, "os.arch").unwrap_or_default() {
        "amd64" | "x86_64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        "x86" | "i386" | "i686" => "x86",
        other => other,
    }
    .to_owned();
    Ok(JavaRuntime {
        executable: path,
        major_version,
        architecture,
    })
}

fn property<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        (name.trim() == key).then_some(value.trim())
    })
}

fn verify(runtime: JavaRuntime, major: u32) -> Result<JavaRuntime> {
    if runtime.major_version != major || runtime.architecture != std::env::consts::ARCH {
        return Err(AppError::Java(format!(
            "需要 Java {major} / {}，实际为 Java {} / {}",
            std::env::consts::ARCH,
            runtime.major_version,
            runtime.architecture
        )));
    }
    Ok(runtime)
}

pub async fn resolve(
    paths: &AppPaths,
    network: &Network,
    policy: &JavaPolicy,
    major: u32,
    task: TaskContext,
) -> Result<JavaRuntime> {
    if let JavaPolicy::Executable { path } = policy {
        return verify(inspect(path).await?, major);
    }
    let binary = if cfg!(windows) { "java.exe" } else { "java" };
    if let Some(path) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&path) {
            if let Ok(runtime) = inspect(&entry.join(binary)).await
                && let Ok(runtime) = verify(runtime, major)
            {
                return Ok(runtime);
            }
        }
    }
    tokio::fs::create_dir_all(&paths.runtimes)
        .await
        .map_err(|e| io_error("创建 Java 根目录", &paths.runtimes, e))?;
    let _lock = FileLock::acquire(&paths.runtimes.join(format!("temurin-{major}.lock")))?;
    let runtime_dir = paths.runtimes.join(format!(
        "temurin-{major}-{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    if runtime_dir.is_dir()
        && let Some(executable) = find_java(&runtime_dir)
    {
        return verify(inspect(&executable).await?, major);
    }
    let os = if cfg!(windows) { "windows" } else { "linux" };
    let metadata_url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os={os}&vendor=eclipse"
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&network.fetch(&metadata_url, None).await?)?;
    let package = metadata
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.pointer("/binary/package"))
        .ok_or_else(|| AppError::Java("Adoptium 没有返回兼容运行时".into()))?;
    let link = package["link"]
        .as_str()
        .ok_or_else(|| AppError::Java("Java 下载地址缺失".into()))?;
    let checksum = package["checksum"]
        .as_str()
        .ok_or_else(|| AppError::Java("Java SHA256 缺失".into()))?;
    let size = package["size"]
        .as_u64()
        .ok_or_else(|| AppError::Java("Java 下载大小缺失".into()))?;
    let archive = paths.cache.join("java").join(format!("{checksum}.archive"));
    network
        .download(&resource(link, None, Some(size))?, &archive)
        .await?;
    let mut file = tokio::fs::File::open(&archive)
        .await
        .map_err(|e| io_error("读取 Java 压缩包", &archive, e))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 128 * 1024];
    let cancellation = task.child_token();
    loop {
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|e| io_error("校验 Java 压缩包", &archive, e))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if !hex::encode(hash.finalize()).eq_ignore_ascii_case(checksum) {
        drop(file);
        let _ = tokio::fs::remove_file(&archive).await;
        return Err(AppError::Java("Java SHA256 不匹配，未解压或启用".into()));
    }
    drop(file);
    let staging = paths
        .runtimes
        .join(format!(".staging-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir(&staging)
        .await
        .map_err(|e| io_error("创建 Java 临时目录", &staging, e))?;
    // Await extraction and cleanup even after cancellation; never detach an installation child.
    let mut committed = false;
    let result = async {
        let reader = tokio::io::BufReader::new(
            tokio::fs::File::open(&archive)
                .await
                .map_err(|e| io_error("打开 Java 压缩包", &archive, e))?,
        );
        if cfg!(windows) {
            lighty_launcher::core::extract::zip_extract(reader, &staging, None)
                .await
                .map_err(|e| AppError::Java(e.to_string()))?;
        } else {
            lighty_launcher::core::extract::tar_gz_extract(reader, &staging, None)
                .await
                .map_err(|e| AppError::Java(e.to_string()))?;
        }
        if cancellation.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let _executable = find_java(&staging)
            .ok_or_else(|| AppError::Java("压缩包中没有 Java 可执行文件".into()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&_executable, std::fs::Permissions::from_mode(0o755))
                .await
                .map_err(|e| io_error("设置 Java 权限", &_executable, e))?;
        }
        // DrvFs/Windows scanners may briefly keep freshly executed runtime files open.
        // Retrying the same atomic rename preserves the transaction and the runtime lock.
        for attempt in 0..50 {
            match tokio::fs::rename(&staging, &runtime_dir).await {
                Ok(()) => break,
                Err(error)
                    if error.kind() == std::io::ErrorKind::PermissionDenied && attempt < 49 =>
                {
                    if cancellation.is_cancelled() {
                        return Err(AppError::Cancelled);
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => return Err(io_error("提交 Java 目录", &runtime_dir, error)),
            }
        }
        committed = true;
        let executable =
            find_java(&runtime_dir).ok_or_else(|| AppError::Java("提交后找不到 Java".into()))?;
        verify(inspect(&executable).await?, major)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        if committed {
            let _ = tokio::fs::remove_dir_all(&runtime_dir).await;
        }
    }
    result
}

fn find_java(root: &Path) -> Option<PathBuf> {
    let relative = if cfg!(windows) {
        "bin/java.exe"
    } else {
        "bin/java"
    };
    let direct = root.join(relative);
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        if entry.file_type().ok()?.is_dir() {
            let path = entry.path().join(relative);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

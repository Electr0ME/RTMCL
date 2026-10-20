use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("操作已取消")]
    Cancelled,
    #[error("网络或资源错误: {0}")]
    Network(String),
    #[error("账户错误: {0}")]
    Account(String),
    #[error("Java 错误: {0}")]
    Java(String),
    #[error("核心错误: {0}")]
    Core(String),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{operation} ({path}): {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("配置无效: {0}")]
    InvalidConfig(String),
    #[error("不支持的配置版本 {found}，当前支持 {supported}；文件未修改")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("配置解析失败: {0}")]
    Decode(#[from] toml::de::Error),
    #[error("配置序列化失败: {0}")]
    Encode(#[from] toml::ser::Error),
    #[error("无法确定系统应用目录，请使用 --data-dir")]
    MissingDataDirectory,
    #[error("任务状态转换无效: {0}")]
    InvalidTaskTransition(String),
    #[error("{operation} 尚未接入 Lighty；项目状态见 README.zh-CN.md")]
    CoreUnavailable { operation: &'static str },
}

pub(crate) fn io_error(
    operation: &'static str,
    path: impl Into<PathBuf>,
    source: std::io::Error,
) -> AppError {
    AppError::Io {
        operation,
        path: path.into(),
        source,
    }
}

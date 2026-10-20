//! Foundation utilities (AppState, HTTP client, hashing, download, extract, OS probe).

pub mod app_state;
pub mod download;
pub mod download_context;
pub mod errors;
pub mod extract;
pub mod hash;
pub mod hosts;
pub mod macros;
pub mod system;

pub use errors::{
    AppStateError, AppStateResult, DownloadError, DownloadResult, ExtractError, ExtractResult,
    QueryError, QueryResult, SystemError, SystemResult,
};

pub use hash::{
    HashError, HashResult, calculate_file_sha1_sync, calculate_sha1_bytes,
    calculate_sha1_bytes_raw, verify_file_sha1, verify_file_sha1_streaming, verify_file_sha1_sync,
};

pub use app_state::AppState;

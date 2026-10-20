//! Shared application boundary. Terminal code must not depend on Lighty types.

#[cfg(all(feature = "dev-local-accounts", not(debug_assertions)))]
compile_error!(
    "dev-local-accounts is for explicit debug development builds only; release builds must enforce account ownership"
);

pub mod accounts;
pub mod config;
pub mod core;
pub mod domain;
pub mod downloads;
pub mod error;
pub mod java;
pub mod logs;
pub mod network;
mod ownership;
pub mod p0;
pub mod paths;
pub mod ports;
pub mod service;
pub mod storage;
pub mod tasks;

pub use error::{AppError, Result};
pub use paths::AppPaths;
pub use service::LauncherService;

//! Shared application boundary. Terminal code must not depend on Lighty types.

pub mod accounts;
pub mod config;
pub mod core;
pub mod domain;
pub mod downloads;
pub mod error;
pub mod java;
pub mod network;
pub mod p0;
pub mod paths;
pub mod ports;
pub mod service;
pub mod storage;
pub mod tasks;

pub use error::{AppError, Result};
pub use paths::AppPaths;
pub use service::LauncherService;

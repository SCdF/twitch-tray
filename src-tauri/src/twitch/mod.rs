mod client;
pub mod http;
mod types;

pub use client::TwitchClient;
// HttpClient, HttpResponse, ReqwestClient are used internally and in tests
pub use types::*;

/// API error types for distinguishing recoverable errors
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Token is expired or invalid - can be recovered by refreshing
    #[error("Unauthorized - token expired or invalid")]
    Unauthorized,
    /// Other API or network errors
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

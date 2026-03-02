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

/// Calls `f()` and, on [`ApiError::Unauthorized`], calls `refresh()` once and retries.
///
/// Any other error is returned immediately without retrying.
/// If the retry also returns `Unauthorized`, that error is surfaced to the caller.
pub async fn with_retry<F, Fut, T, R, Rfut>(f: F, refresh: R) -> Result<T, ApiError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, ApiError>>,
    R: Fn() -> Rfut,
    Rfut: std::future::Future<Output = anyhow::Result<()>>,
{
    match f().await {
        Ok(val) => Ok(val),
        Err(ApiError::Unauthorized) => {
            refresh().await.map_err(ApiError::Other)?;
            f().await
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    // =========================================================
    // with_retry
    // =========================================================

    #[tokio::test]
    async fn retry_returns_ok_on_first_success() {
        let result: Result<i32, ApiError> =
            with_retry(|| async { Ok(42) }, || async { Ok(()) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn retry_refreshes_and_succeeds_after_unauthorized() {
        let first_call = Arc::new(AtomicBool::new(false));
        let refresh_called = Arc::new(AtomicBool::new(false));

        let fc = first_call.clone();
        let rc = refresh_called.clone();

        let result: Result<i32, ApiError> = with_retry(
            move || {
                let was_first = !fc.fetch_or(true, Ordering::SeqCst);
                async move {
                    if was_first {
                        Err(ApiError::Unauthorized)
                    } else {
                        Ok(99)
                    }
                }
            },
            move || {
                rc.store(true, Ordering::SeqCst);
                async { Ok::<(), anyhow::Error>(()) }
            },
        )
        .await;

        assert_eq!(result.unwrap(), 99);
        assert!(
            refresh_called.load(Ordering::SeqCst),
            "refresh must be called on 401"
        );
    }

    #[tokio::test]
    async fn retry_surfaces_second_unauthorized_after_refresh() {
        // f always returns Unauthorized; refresh succeeds but the retry also fails
        let result: Result<i32, ApiError> = with_retry(
            || async { Err::<i32, _>(ApiError::Unauthorized) },
            || async { Ok::<(), anyhow::Error>(()) },
        )
        .await;

        assert!(
            matches!(result, Err(ApiError::Unauthorized)),
            "second 401 must be surfaced to caller"
        );
    }

    #[tokio::test]
    async fn retry_does_not_refresh_on_non_401_error() {
        let refresh_called = Arc::new(AtomicBool::new(false));
        let rc = refresh_called.clone();

        let result: Result<i32, ApiError> = with_retry(
            || async { Err::<i32, _>(ApiError::Other(anyhow::anyhow!("network error"))) },
            move || {
                rc.store(true, Ordering::SeqCst);
                async { Ok::<(), anyhow::Error>(()) }
            },
        )
        .await;

        assert!(matches!(result, Err(ApiError::Other(_))));
        assert!(
            !refresh_called.load(Ordering::SeqCst),
            "refresh must NOT be called for non-401 errors"
        );
    }
}

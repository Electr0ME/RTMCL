//! Consumer-owned download I/O, scoped to one operation; authentication never uses it.
use std::{future::Future, io, pin::Pin, sync::Arc};

pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = io::Result<Vec<u8>>> + Send + 'a>>;

pub trait DownloadContext: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str, sha1: Option<&'a str>) -> FetchFuture<'a>;
}

tokio::task_local! {
    static CONTEXT: Arc<dyn DownloadContext>;
}

pub async fn scope<F: Future>(context: Arc<dyn DownloadContext>, future: F) -> F::Output {
    CONTEXT.scope(context, future).await
}

pub async fn fetch(url: &str, sha1: Option<&str>) -> io::Result<Vec<u8>> {
    if let Ok(context) = CONTEXT.try_with(Arc::clone) {
        return context.fetch(url, sha1).await;
    }
    let bytes = crate::hosts::HTTP_CLIENT
        .get(url)
        .send()
        .await
        .map_err(io::Error::other)?
        .error_for_status()
        .map_err(io::Error::other)?
        .bytes()
        .await
        .map_err(io::Error::other)?
        .to_vec();
    if let Some(expected) = sha1 {
        if !crate::calculate_sha1_bytes(&bytes).eq_ignore_ascii_case(expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "metadata SHA1 mismatch",
            ));
        }
    }
    Ok(bytes)
}

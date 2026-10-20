//! Authentication-only license request; never uses download mirrors or persisted flags.
use std::time::Duration;

use lighty_launcher::auth::{ExposeSecret, UserProfile};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::{AppError, Result};

pub(crate) async fn verify(profile: &UserProfile, cancellation: CancellationToken) -> Result<()> {
    let token = profile
        .access_token
        .as_ref()
        .ok_or_else(|| AppError::Account("缺少 Minecraft 访问令牌，无法验证游戏所有权".into()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("RTMCL/0.1.0")
        .build()
        .map_err(|_| AppError::Account("无法初始化游戏所有权检查".into()))?;
    let url = format!(
        "https://api.minecraftservices.com/entitlements/license?requestId={}",
        uuid::Uuid::new_v4()
    );
    check(&client, &url, token.expose_secret(), cancellation).await
}

async fn check(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    cancellation: CancellationToken,
) -> Result<()> {
    let operation = async {
        let mut response = client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| AppError::Account("游戏所有权检查网络失败，请稍后重新验证".into()))?;
        if !response.status().is_success() {
            return Err(AppError::Account(format!(
                "游戏所有权检查返回 HTTP {}",
                response.status().as_u16()
            )));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AppError::Account("游戏所有权响应传输失败".into()))?
        {
            if body.len() + chunk.len() > 65536 {
                return Err(AppError::Account("游戏所有权响应过大".into()));
            }
            body.extend_from_slice(&chunk);
        }
        license(&body)
    };
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(AppError::Cancelled),
        result = operation => result,
    }
}

fn license(body: &[u8]) -> Result<()> {
    #[derive(Deserialize)]
    struct License {
        items: Vec<Item>,
    }
    #[derive(Deserialize)]
    struct Item {
        name: String,
    }
    let response: License = serde_json::from_slice(body)
        .map_err(|_| AppError::Account("游戏所有权响应格式无效".into()))?;
    if response
        .items
        .iter()
        .any(|item| item.name == "product_minecraft")
        && response
            .items
            .iter()
            .any(|item| item.name == "game_minecraft")
    {
        Ok(())
    } else {
        Err(AppError::Account(
            "账户缺少 Minecraft Java 版所有权或可玩授权".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn requires_java_ownership_and_play_entitlements() {
        assert!(
            license(br#"{"items":[{"name":"product_minecraft"},{"name":"game_minecraft"}]}"#)
                .is_ok()
        );
        for body in [
            br#"{"items":[]}"#.as_slice(),
            br#"{"items":[{"name":"product_minecraft"}]}"#,
            br#"{"items":[{"name":"unrelated"}]}"#,
            br#"{"owns_minecraft":true}"#,
            b"synthetic-secret",
        ] {
            let error = license(body).unwrap_err().to_string();
            assert!(!error.contains("synthetic-secret"));
        }
    }

    #[tokio::test]
    async fn http_status_and_cancellation_cannot_unlock_ownership() {
        for status in [200, 401, 403] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!(
                "http://{}/entitlements/license?requestId=fixture",
                listener.local_addr().unwrap()
            );
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    request.extend_from_slice(&chunk[..n]);
                    if n == 0 || request.windows(4).any(|b| b == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                assert!(request.contains("authorization: bearer synthetic-token"));
                let body = r#"{"items":[{"name":"product_minecraft"},{"name":"game_minecraft"}],"extra":"synthetic-secret"}"#;
                socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            });
            let result = check(
                &reqwest::Client::new(),
                &url,
                "synthetic-token",
                CancellationToken::new(),
            )
            .await;
            assert_eq!(result.is_ok(), status == 200);
            if let Err(error) = result {
                assert!(!error.to_string().contains("synthetic-secret"));
            }
            server.await.unwrap();
        }
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            check(
                &reqwest::Client::new(),
                "http://127.0.0.1:1",
                "synthetic-token",
                cancellation
            )
            .await,
            Err(AppError::Cancelled)
        ));
    }
}

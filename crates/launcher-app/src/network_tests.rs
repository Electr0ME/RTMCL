use super::*;
use crate::tasks::{TaskHandle, TaskState};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Reply {
    status: u16,
    headers: String,
    body: &'static str,
    declared: usize,
}

impl Reply {
    fn ok(body: &'static str) -> Self {
        Self {
            status: 200,
            headers: String::new(),
            body,
            declared: body.len(),
        }
    }
}

async fn server(replies: Vec<Reply>) -> (Url, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    tokio::spawn(async move {
        for reply in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                let n = socket.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
                if bytes.windows(4).any(|b| b == b"\r\n\r\n") {
                    break;
                }
            }
            captured
                .lock()
                .await
                .push(String::from_utf8_lossy(&bytes).into_owned());
            let response = format!(
                "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                reply.status, reply.declared, reply.headers, reply.body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (format!("http://{address}/file").parse().unwrap(), requests)
}

fn network(root: &Path, retries: u32) -> (Network, TaskHandle) {
    let (handle, task) = TaskHandle::new(None);
    task.start().unwrap();
    let mut config = AppConfig::default();
    config.downloads.retries_per_source = retries;
    (
        Network::new(AppPaths::discover(Some(root)).unwrap(), config, task).unwrap(),
        handle,
    )
}

#[test]
fn maps_known_origins_and_preserves_encoded_paths_and_queries() {
    let mut config = AppConfig::default();
    config.downloads.sources = vec![SourceId::Bmclapi];
    config.downloads.mode = crate::config::SourceMode::Strict;
    for (url, expected) in [
        (
            "https://resources.download.minecraft.net/ab/cd",
            "/assets/ab/cd",
        ),
        (
            "https://libraries.minecraft.net/org/a%20b.jar?x=1",
            "/maven/org/a%20b.jar?x=1",
        ),
        (
            "https://meta.fabricmc.net/v2/versions/loader",
            "/fabric-meta/v2/versions/loader",
        ),
        (MANIFEST_URL, "/mc/game/version_manifest_v2.json"),
    ] {
        let routed = candidates(&config, &resource(url, None, None).unwrap()).unwrap();
        assert!(
            routed[0].url.as_str().ends_with(expected),
            "{}",
            routed[0].url
        );
    }
    assert!(
        candidates(
            &config,
            &resource("https://example.org/a", None, None).unwrap()
        )
        .is_err()
    );
    assert!(
        candidates(
            &config,
            &resource("https://libraries.minecraft.net.attacker.org/a", None, None).unwrap()
        )
        .is_err()
    );
}

#[tokio::test]
async fn verifies_then_reuses_cache_and_redownloads_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let (network, _) = network(temp.path(), 0);
    let (url, requests) = server(vec![Reply::ok("hello"), Reply::ok("hello")]).await;
    let request = resource(url.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap();
    let path = temp.path().join("file");
    network.download(&request, &path).await.unwrap();
    network.download(&request, &path).await.unwrap();
    assert_eq!(requests.lock().await.len(), 1);
    tokio::fs::write(&path, b"xxxxx").await.unwrap();
    network.download(&request, &path).await.unwrap();
    assert_eq!(requests.lock().await.len(), 2);
    assert_eq!(tokio::fs::read(path).await.unwrap(), b"hello");
}

#[tokio::test]
async fn resumes_same_source_with_validated_206() {
    let temp = tempfile::tempdir().unwrap();
    let (network, _) = network(temp.path(), 1);
    let (url, requests) = server(vec![
        Reply {
            declared: 5,
            ..Reply::ok("he")
        },
        Reply {
            status: 206,
            headers: "Content-Range: bytes 2-4/5\r\n".into(),
            declared: 3,
            body: "llo",
        },
    ])
    .await;
    let path = temp.path().join("file");
    network
        .download(
            &resource(url.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap(),
            &path,
        )
        .await
        .unwrap();
    assert!(
        requests.lock().await[1]
            .to_ascii_lowercase()
            .contains("range: bytes=2-")
    );
    assert_eq!(tokio::fs::read(path).await.unwrap(), b"hello");
}

#[tokio::test]
async fn full_200_replaces_partial_and_416_restarts() {
    for ranged in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (network, _) = network(temp.path(), 2);
        let mut replies = vec![Reply {
            declared: 5,
            ..Reply::ok("he")
        }];
        if ranged {
            replies.push(Reply {
                status: 416,
                ..Reply::ok("")
            });
        }
        replies.push(Reply::ok("hello"));
        let (url, _) = server(replies).await;
        let path = temp.path().join("file");
        network
            .download(
                &resource(url.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap(),
                &path,
            )
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"hello");
    }
}

#[tokio::test]
async fn bad_hash_never_commits_and_source_switch_does_not_resume() {
    let temp = tempfile::tempdir().unwrap();
    let (network, _) = network(temp.path(), 0);
    let (bad, _) = server(vec![Reply::ok("wrong")]).await;
    let (good, requests) = server(vec![Reply::ok("hello")]).await;
    let sources = [
        SourceCandidate {
            source: SourceId::Bmclapi,
            url: bad,
        },
        SourceCandidate {
            source: SourceId::Official,
            url: good.clone(),
        },
    ];
    let path = temp.path().join("file");
    let request = resource(good.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap();
    let record = network
        .download_candidates(&request, &path, &sources)
        .await
        .unwrap();
    assert_eq!(record.source, SourceId::Official);
    assert!(
        !requests.lock().await[0]
            .to_ascii_lowercase()
            .contains("range:")
    );
    let corrupted = temp.path().join("bad-file");
    let (bad, _) = server(vec![Reply::ok("wrong")]).await;
    assert!(
        network
            .download(
                &resource(bad.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap(),
                &corrupted
            )
            .await
            .is_err()
    );
    assert!(!corrupted.exists());
}

#[tokio::test]
async fn invalid_range_and_rate_limit_are_bounded_and_cancel_is_not_success() {
    let temp = tempfile::tempdir().unwrap();
    let (network, handle) = network(temp.path(), 1);
    let (url, _) = server(vec![
        Reply {
            status: 429,
            headers: "Retry-After: 0\r\n".into(),
            ..Reply::ok("")
        },
        Reply {
            status: 206,
            headers: "Content-Range: bytes 2-6/7\r\n".into(),
            ..Reply::ok("hello")
        },
    ])
    .await;
    let path = temp.path().join("file");
    assert!(
        network
            .download(
                &resource(url.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap(),
                &path
            )
            .await
            .is_err()
    );
    assert!(!path.exists());
    handle.request_cancel();
    assert!(matches!(
        network
            .download(&resource(url.as_str(), None, None).unwrap(), &path)
            .await,
        Err(AppError::Cancelled)
    ));
    assert_eq!(handle.snapshot().state, TaskState::Cancelling);
}

#[tokio::test]
async fn unreachable_official_candidate_falls_back_to_mirror() {
    let temp = tempfile::tempdir().unwrap();
    let (network, _) = network(temp.path(), 0);
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let official: Url = format!("http://{}/file", closed.local_addr().unwrap())
        .parse()
        .unwrap();
    drop(closed);
    let (mirror, _) = server(vec![Reply::ok("hello")]).await;
    let sources = [
        SourceCandidate {
            source: SourceId::Official,
            url: official.clone(),
        },
        SourceCandidate {
            source: SourceId::Bmclapi,
            url: mirror,
        },
    ];
    let record = network
        .download_candidates(
            &resource(official.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap(),
            &temp.path().join("file"),
            &sources,
        )
        .await
        .unwrap();
    assert_eq!(record.source, SourceId::Bmclapi);
}

#[tokio::test]
async fn follows_redirect_then_checks_payload_identity() {
    let temp = tempfile::tempdir().unwrap();
    let (network, _) = network(temp.path(), 0);
    let (url, requests) = server(vec![
        Reply {
            status: 302,
            headers: "Location: /redirected\r\n".into(),
            ..Reply::ok("")
        },
        Reply::ok("hello"),
    ])
    .await;
    let path = temp.path().join("file");
    network
        .download(
            &resource(url.as_str(), Some(&hex_sha1(b"hello")), Some(5)).unwrap(),
            &path,
        )
        .await
        .unwrap();
    assert!(requests.lock().await[1].starts_with("GET /redirected "));
    assert_eq!(tokio::fs::read(path).await.unwrap(), b"hello");
}

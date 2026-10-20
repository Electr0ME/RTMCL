//! Exercise the real Lighty adapter against a complete, small mirror fixture.
use std::{collections::HashMap, sync::Arc, time::Duration};

use launcher_app::{
    AppPaths, LauncherService,
    config::{AppConfig, SourceMode},
    domain::{InstallState, Loader},
    downloads::SourceId,
    network::Network,
    p0,
    tasks::TaskHandle,
};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha1::digest(bytes))
}

fn descriptor(url: &str, body: &[u8]) -> Value {
    json!({"url": url, "sha1": hash(body), "size": body.len()})
}

#[tokio::test]
async fn lighty_installs_vanilla_and_fabric_through_strict_mirror() {
    let client = b"fixture client";
    let library = b"fixture library";
    let asset = b"fixture asset";
    let logging = b"<Configuration/>";
    let mut native = vec![0_u8; 22]; // Valid empty ZIP, not an executable native library.
    native[..4].copy_from_slice(b"PK\x05\x06");
    let os = std::env::consts::OS;
    let other_os = if os == "windows" { "linux" } else { "windows" };
    let asset_path = format!("/assets/{}/{}", &hash(asset)[..2], hash(asset));
    let index = serde_json::to_vec(&json!({"objects": {
        "tiny.txt": {"hash": hash(asset), "size": asset.len()}
    }}))
    .unwrap();
    let mut artifact = descriptor(
        "https://libraries.minecraft.net/example/lib/1/lib-1.jar",
        library,
    );
    artifact["path"] = json!("example/lib/1/lib-1.jar");
    let mut native_artifact = descriptor(
        "https://libraries.minecraft.net/example/native/1/native.jar",
        &native,
    );
    native_artifact["path"] = json!("example/native/1/native.jar");
    let mut logging_file = descriptor("https://piston-data.mojang.com/fixture.xml", logging);
    logging_file["id"] = json!("fixture.xml");
    let mut index_entry = descriptor("https://piston-meta.mojang.com/fixture-index.json", &index);
    index_entry["id"] = json!("fixture");
    let version = serde_json::to_vec(&json!({
        "id": "1.21.1", "mainClass": "fixture.Main", "type": "release",
        "releaseTime": "2024-08-08T12:24:45Z", "time": "2024-08-08T12:24:45Z",
        "assetIndex": index_entry, "assets": "fixture",
        "javaVersion": {"component": "java-runtime-delta", "majorVersion": 21},
        "downloads": {"client": descriptor("https://piston-data.mojang.com/fixture-client.jar", client)},
        "arguments": {"game": [], "jvm": []},
        "libraries": [
            {"name": "example:lib:1", "downloads": {"artifact": artifact.clone()}},
            {"name": format!("example:native:1:natives-{os}"), "downloads": {"artifact": native_artifact}},
            {"name": "example:excluded:1", "downloads": {"artifact": {
                "url": "https://libraries.minecraft.net/should-not-download", "path": "excluded.jar",
                "sha1": hash(library), "size": library.len()
            }}, "rules": [{"action": "allow", "os": {"name": other_os}}]}
        ],
        "logging": {"client": {"argument": "-Dlog4j.configurationFile=${path}",
            "type": "log4j2-xml", "file": logging_file}}
    }))
    .unwrap();
    let version_path = format!("/v1/packages/{}/1.21.1.json", hash(&version));
    let manifest = json!({
        "latest": {"release": "1.21.1", "snapshot": "1.21.1"},
        "versions": [{"id": "1.21.1", "type": "release",
            "url": format!("https://piston-meta.mojang.com{version_path}"), "sha1": hash(&version),
            "time": "2024-08-08T12:24:45Z", "releaseTime": "2024-08-08T12:24:45Z", "complianceLevel": 1}]
    });
    let fabric_path = "/fabric-meta/v2/versions/loader/1.21.1/0.16.10/profile/json";
    let fabric_library = "/maven/net/fabricmc/fixture-loader/0.16.10/fixture-loader-0.16.10.jar";
    let fabric = json!({
        "arguments": {"game": [], "jvm": []}, "id": "fabric-fixture", "inheritsFrom": "1.21.1",
        "libraries": [{"name": "net.fabricmc:fixture-loader:0.16.10", "url": "https://maven.fabricmc.net/"}],
        "mainClass": "fixture.FabricMain", "releaseTime": "2024-08-08T12:24:45Z",
        "time": "2024-08-08T12:24:45Z", "type": "release"
    });
    let routes = HashMap::<String, Vec<u8>>::from([
        (
            "/mc/game/version_manifest_v2.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        ),
        (version_path.clone(), version),
        ("/fixture-index.json".into(), index),
        (asset_path.clone(), asset.to_vec()),
        ("/maven/example/lib/1/lib-1.jar".into(), library.to_vec()),
        ("/maven/example/native/1/native.jar".into(), native),
        ("/fixture-client.jar".into(), client.to_vec()),
        ("/fixture.xml".into(), logging.to_vec()),
        (fabric_path.into(), serde_json::to_vec(&fabric).unwrap()),
        (fabric_library.into(), library.to_vec()),
        (format!("{fabric_library}.sha1"), hash(library).into_bytes()),
    ]);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let server = tokio::spawn(async move {
        loop {
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
            let request = String::from_utf8(bytes).unwrap();
            let path = request.split_whitespace().nth(1).unwrap().to_owned();
            captured.lock().await.push(path.clone());
            let (status, body) = routes
                .get(&path)
                .map_or((404, &b"missing"[..]), |body| (200, body.as_slice()));
            let header = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let service =
        LauncherService::new(AppPaths::discover(Some(&temp.path().join("安装 空格"))).unwrap());
    let mut config = AppConfig::default();
    config.downloads.mode = SourceMode::Strict;
    config.downloads.sources = vec![SourceId::Bmclapi];
    config.downloads.retries_per_source = 0;
    config.mirrors.bmclapi.base_url = format!("http://{address}").parse().unwrap();
    for loader in [
        Loader::Vanilla,
        Loader::Fabric {
            version: "0.16.10".into(),
        },
    ] {
        let instance = service
            .create_instance("mirror fixture".into(), "1.21.1".into(), loader)
            .unwrap();
        let (_handle, task) = TaskHandle::new(Some(instance.id));
        task.start().unwrap();
        let network = Network::new(service.paths().clone(), config.clone(), task.clone()).unwrap();
        tokio::time::timeout(
            Duration::from_secs(20),
            p0::install(service.clone(), instance.id, network, task),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            service.instance(instance.id).unwrap().install_state,
            InstallState::Ready
        );
        let dir = service.paths().instance_dir(instance.id);
        let record: Value =
            serde_json::from_slice(&std::fs::read(dir.join("installation.json")).unwrap()).unwrap();
        let expected_main = if matches!(instance.loader, Loader::Vanilla) {
            "fixture.Main"
        } else {
            "fixture.FabricMain"
        };
        assert_eq!(
            record["metadata"]["main_class"]["main_class"],
            expected_main
        );
        assert_eq!(
            std::fs::read(dir.join("minecraft/assets/log_configs/fixture.xml")).unwrap(),
            logging
        );
        assert!(dir.join("minecraft/natives").is_dir());
    }
    let requests = requests.lock().await;
    for required in [
        "/mc/game/version_manifest_v2.json",
        &version_path,
        "/fixture-index.json",
        &asset_path,
        "/maven/example/lib/1/lib-1.jar",
        "/maven/example/native/1/native.jar",
        "/fixture-client.jar",
        "/fixture.xml",
        fabric_path,
        fabric_library,
        &format!("{fabric_library}.sha1"),
    ] {
        assert!(
            requests.iter().any(|path| path == required),
            "missing route: {required}"
        );
    }
    assert!(
        !requests
            .iter()
            .any(|path| path.contains("should-not-download"))
    );
    server.abort();
}

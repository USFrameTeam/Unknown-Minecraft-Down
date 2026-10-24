//! verify_installation 集成测试：在临时目录"装"出一个迷你 1.20.1，
//! 然后校验完整性，再人为删除/损坏文件验证报告准确性。
//! 全部资源来自本地回环测试服务器，不依赖外网。

use std::sync::Arc;

use serde_json::json;
use sha1::{Digest, Sha1};
use tokio_util::sync::CancellationToken;

use mc_dl_engine::downloader::{build_http_client, download_file, test_support as ts};
use mc_dl_engine::mirror::Mirror;
use mc_dl_engine::verifier::verify_installation;

fn sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn single_file_server(body: Vec<u8>) -> impl std::future::Future<Output = std::net::SocketAddr> {
    async move {
        ts::spawn(
            body,
            ts::ServerOpts {
                support_range: true,
                truncate_after: None,
                seen_ranges: Arc::new(std::sync::Mutex::new(vec![])),
            },
        )
        .await
        .addr
    }
}

async fn download_to(server: std::net::SocketAddr, body: &[u8], dest: &std::path::Path) {
    let client = build_http_client().unwrap();
    download_file(
        &client,
        &format!("http://{server}/x"),
        dest,
        Some(&sha1_hex(body)),
        Some(body.len() as u64),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn verify_reports_missing_and_corrupted_accurately() {
    let client_jar = b"fake-client-jar-bytes".to_vec();
    let lib = b"fake-library-bytes".to_vec();
    let asset = b"asset-data-0001".to_vec();
    let asset_hash = sha1_hex(&asset);

    // 本地测试服务器（每个服务一种内容，任意路径返回同一 body）
    let s_jar = single_file_server(client_jar.clone()).await;
    let s_lib = single_file_server(lib.clone()).await;
    let s_asset = single_file_server(asset.clone()).await;
    let index_json = json!({
        "objects": {
            "minecraft/sounds/test.ogg": { "hash": asset_hash, "size": asset.len() as u64 }
        }
    });
    let index_body = serde_json::to_vec(&index_json).unwrap();
    let s_index = single_file_server(index_body.clone()).await;

    // 版本 JSON：URL 指向本地测试服务器
    let version_json = json!({
        "id": "t1.20.1",
        "type": "release",
        "mainClass": "x.Y",
        "assetIndex": {
            "id": "t8",
            "url": format!("http://{s_index}/index.json"),
            "size": index_body.len() as u64,
            "totalSize": asset.len() as u64
        },
        "downloads": {
            "client": {
                "url": format!("http://{s_jar}/client.jar"),
                "sha1": sha1_hex(&client_jar),
                "size": client_jar.len() as u64
            }
        },
        "libraries": [
            {
                "name": "com.test:lib:1.0",
                "downloads": {
                    "artifact": {
                        "path": "com/test/lib/1.0/lib-1.0.jar",
                        "url": format!("http://{s_lib}/lib.jar"),
                        "sha1": sha1_hex(&lib),
                        "size": lib.len() as u64
                    }
                }
            }
        ]
    });

    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().to_path_buf();

    // "安装"：版本 JSON 直接落盘，其余文件经下载器写入（兼测 downloader）
    std::fs::create_dir_all(game.join("versions/t1.20.1")).unwrap();
    std::fs::write(
        game.join("versions/t1.20.1/t1.20.1.json"),
        serde_json::to_vec(&version_json).unwrap(),
    )
    .unwrap();
    download_to(s_jar, &client_jar, &game.join("versions/t1.20.1/t1.20.1.jar")).await;
    download_to(s_lib, &lib, &game.join("libraries/com/test/lib/1.0/lib-1.0.jar")).await;
    download_to(s_index, &index_body, &game.join("assets/indexes/t8.json")).await;
    download_to(
        s_asset,
        &asset,
        &game.join(format!("assets/objects/{}/{}", &asset_hash[..2], asset_hash)),
    )
    .await;

    // 完整安装 → 校验通过（无网络依赖：所有 JSON 本地齐全）
    let report = verify_installation(Mirror::Official, &game, "t1.20.1")
        .await
        .unwrap();
    assert!(
        report.ok,
        "完整安装应通过校验: missing={:?} corrupted={:?}",
        report.missing, report.corrupted
    );
    assert_eq!(report.checked_files, 4);
    // jar + lib + 资产本体 + 资产索引文件本身
    assert_eq!(
        report.total_bytes,
        (client_jar.len() + lib.len() + asset.len() + index_body.len()) as u64
    );

    // 人为删除一个 assets 文件 → verify 准确报告缺失（验收标准之一）
    std::fs::remove_file(game.join(format!("assets/objects/{}/{}", &asset_hash[..2], asset_hash)))
        .unwrap();
    let report = verify_installation(Mirror::Official, &game, "t1.20.1")
        .await
        .unwrap();
    assert!(!report.ok);
    assert_eq!(report.missing.len(), 1);
    assert!(report.missing[0].path.starts_with("assets/objects/"));

    // 损坏客户端 jar → verify 准确报告损坏
    std::fs::write(game.join("versions/t1.20.1/t1.20.1.jar"), b"corrupted!!").unwrap();
    let report = verify_installation(Mirror::Official, &game, "t1.20.1")
        .await
        .unwrap();
    assert!(!report.ok);
    assert_eq!(report.corrupted.len(), 1);
    assert_eq!(report.corrupted[0].path, "versions/t1.20.1/t1.20.1.jar");
    assert_ne!(report.corrupted[0].actual_sha1, report.corrupted[0].expected_sha1);
}

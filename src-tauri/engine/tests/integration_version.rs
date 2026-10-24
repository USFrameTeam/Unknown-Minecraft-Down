//! inheritsFrom 继承链离线解析测试（Forge/Fabric 风格），全部本地 JSON，不联网。

use std::collections::HashMap;

use serde_json::json;

use mc_dl_engine::mirror::Mirror;
use mc_dl_engine::version_json::{OsContext, OsName, VersionJson};
use mc_dl_engine::{downloader, version_json};

fn parent_json() -> serde_json::Value {
    json!({
        "id": "1.20.1",
        "mainClass": "net.minecraft.client.main.Main",
        "releaseTime": "2023-06-12T13:25:51+00:00",
        "time": "2023-06-12T13:25:51+00:00",
        "type": "release",
        "arguments": { "game": [], "jvm": [] },
        "assetIndex": {
            "id": "8", "sha1": "idx_sha_40charsxxxxxxxxxxxxxxxxxxxxxx",
            "size": 100, "totalSize": 400,
            "url": "https://piston-meta.mojang.com/v1/packages/idx/8.json"
        },
        "assets": "8",
        "downloads": {
            "client": {
                "sha1": "client_sha_40charsxxxxxxxxxxxxxxxxxxxxxxx",
                "size": 23_000,
                "url": "https://piston-data.mojang.com/v1/objects/abcd/client.jar"
            }
        },
        "libraries": [
            {
                "name": "com.mojang:brigadier:1.0.18",
                "downloads": {
                    "artifact": {
                        "path": "com/mojang/brigadier/brigadier/1.0.18/brigadier-1.0.18.jar",
                        "sha1": "aa1", "size": 10,
                        "url": "https://libraries.minecraft.net/com/mojang/brigadier/brigadier/1.0.18/brigadier-1.0.18.jar"
                    }
                }
            },
            {
                "name": "org.lwjgl:lwjgl:3.3.1",
                "rules": [ { "action": "allow", "os": { "name": "windows" } } ],
                "downloads": {
                    "artifact": {
                        "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
                        "sha1": "aa2", "size": 20,
                        "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar"
                    },
                    "classifiers": {
                        "natives-windows": {
                            "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar",
                            "sha1": "aa3", "size": 30,
                            "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar"
                        }
                    }
                },
                "natives": { "windows": "natives-windows" },
                "extract": { "exclude": ["META-INF/"] }
            }
        ]
    })
}

fn child_json() -> serde_json::Value {
    json!({
        "id": "fabric-loader-0.16.9-1.20.1",
        "inheritsFrom": "1.20.1",
        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "releaseTime": "2024-01-01T00:00:00+00:00",
        "time": "2024-01-01T00:00:00+00:00",
        "type": "release",
        "libraries": [
            {
                // 覆盖父版本同名库（子版本优先）
                "name": "com.mojang:brigadier:1.0.18",
                "downloads": {
                    "artifact": {
                        "path": "com/mojang/brigadier/brigadier/1.0.18/brigadier-1.0.18.jar",
                        "sha1": "OVER-RIDDEN", "size": 10,
                        "url": "https://libraries.minecraft.net/com/mojang/brigadier/brigadier/1.0.18/brigadier-1.0.18.jar"
                    }
                }
            },
            {
                // 子版本新增库（fabric maven）
                "name": "net.fabricmc:fabric-loader:0.16.9",
                "url": "https://maven.fabricmc.net/",
                "rules": [ { "action": "allow" } ]
            }
        ]
    })
}

async fn setup_chain_dir(dir: &std::path::Path) {
    let p = dir.join("versions/1.20.1");
    tokio::fs::create_dir_all(&p).await.unwrap();
    tokio::fs::write(
        p.join("1.20.1.json"),
        serde_json::to_vec_pretty(&parent_json()).unwrap(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn resolves_inherits_from_offline_and_merges() {
    let dir = tempfile::tempdir().unwrap();
    setup_chain_dir(dir.path()).await;

    let client = downloader::build_http_client().unwrap();
    let manifest = version_json::ManifestHandle::new(&client, Mirror::Official);
    let child: VersionJson = serde_json::from_value(child_json()).unwrap();
    let entry = version_json::ChainEntry {
        json: child,
        raw: Some(serde_json::to_string(&child_json()).unwrap()),
        source_url: None,
        manifest_sha1: None,
    };

    let chain =
        version_json::resolve_version(&client, Mirror::Official, dir.path(), entry, &manifest)
            .await
            .unwrap();

    // 链：根(1.20.1) → 子(fabric)
    assert_eq!(chain.entries.len(), 2);
    assert_eq!(chain.entries[0].json.id, "1.20.1");
    assert_eq!(chain.entries[1].json.id, "fabric-loader-0.16.9-1.20.1");
    // jar 归属根版本
    assert_eq!(chain.jar_id, "1.20.1");
    assert!(chain.client_jar.is_some());
    // assetIndex 继承自父
    assert_eq!(chain.asset_index.id, "8");
    // mainClass 取子版本
    assert!(chain.entries[1].json.main_class.as_deref().unwrap().contains("KnotClient"));

    // libraries 合并：父 2 个 + 子 1 个新增，同名覆盖
    assert_eq!(chain.libraries.len(), 3);
    let brigadier = chain
        .libraries
        .iter()
        .find(|l| l.name.as_deref() == Some("com.mojang:brigadier:1.0.18"))
        .unwrap();
    assert_eq!(
        brigadier.downloads.as_ref().unwrap().artifact.as_ref().unwrap().sha1.as_deref(),
        Some("OVER-RIDDEN"),
        "同名库必须被子版本覆盖"
    );

    // 由合并结果构建下载计划（linux 上下文：父的 windows natives 库应被规则剔除）
    let ctx = OsContext { os: OsName::Linux, arch: "x64", features: HashMap::new() };
    let plan = mc_dl_engine::plan::build_plan(&chain, Mirror::Official, &ctx).unwrap();
    assert_eq!(plan.version_id, "fabric-loader-0.16.9-1.20.1");
    assert_eq!(plan.chain_ids, vec!["1.20.1", "fabric-loader-0.16.9-1.20.1"]);
    // client jar(1) + brigadier(1) + fabric-loader(1) + asset index(1) = 4
    assert_eq!(plan.jobs.len(), 4);
    let jar = plan.jobs.iter().find(|j| j.dest.ends_with(".jar") && j.dest.contains("versions/")).unwrap();
    assert_eq!(jar.dest, "versions/1.20.1/1.20.1.jar", "继承版本的客户端 jar 放在父版本目录");
    assert!(plan.jobs.iter().all(|j| !j.dest.contains("lwjgl")));
    // fabric 库应走 fabric maven 且映射时不改域名
    let fabric = plan.jobs.iter().find(|j| j.dest.contains("fabric-loader")).unwrap();
    assert_eq!(
        fabric.url,
        "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
    );
}

#[tokio::test]
async fn detects_inherits_from_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let a = json!({ "id": "a", "inheritsFrom": "b", "libraries": [] });
    let b = json!({ "id": "b", "inheritsFrom": "a", "libraries": [] });
    for (id, v) in [("a", &a), ("b", &b)] {
        let p = dir.path().join(format!("versions/{id}"));
        tokio::fs::create_dir_all(&p).await.unwrap();
        tokio::fs::write(p.join(format!("{id}.json")), serde_json::to_vec(&v).unwrap())
            .await
            .unwrap();
    }
    let client = downloader::build_http_client().unwrap();
    let manifest = version_json::ManifestHandle::new(&client, Mirror::Official);
    let entry = version_json::ChainEntry {
        json: serde_json::from_value(a).unwrap(),
        raw: None,
        source_url: None,
        manifest_sha1: None,
    };
    let err = version_json::resolve_version(&client, Mirror::Official, dir.path(), entry, &manifest)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("环"), "{err}");
}

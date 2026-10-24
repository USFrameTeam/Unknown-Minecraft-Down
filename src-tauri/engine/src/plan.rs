//! 下载计划构建：把解析后的继承链转成“待下载文件任务列表”。
//!
//! 目录布局（与 PCL/HMCL/BakaXL 兼容的标准 .minecraft 结构）：
//! - versions/<id>/<id>.json   每个链上版本一份（启动器靠它自己解析 inheritsFrom）
//! - versions/<jar_id>/<jar_id>.jar  客户端 jar（继承版本共用父版本 jar）
//! - versions/<id>/natives/    natives 解压目标
//! - libraries/<maven path>
//! - assets/indexes/<index id>.json
//! - assets/objects/<hash 前两位>/<hash>
//! - assets/virtual/<index id>/...  legacy 索引的虚拟资源拷贝

use serde::{Deserialize, Serialize};

use crate::game_dir::safe_join;
use crate::mirror::Mirror;
use crate::version_json::{
    evaluate_rules, library_artifact, natives_classifier, OsContext, ResolvedChain,
};
use crate::{EngineError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    ClientJar,
    Library,
    NativesJar,
    AssetIndex,
    Asset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileJob {
    pub url: String,
    /// 相对 game_dir 的目标路径，`/` 分隔；写入前会再过 safe_join 校验。
    pub dest: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeExtract {
    /// 已下载的 natives jar（相对 game_dir）。
    pub jar: String,
    /// 解压目标目录（相对 game_dir），如 versions/<id>/natives。
    pub dest_dir: String,
    #[serde(default)]
    pub excludes: Vec<String>,
}

/// legacy 索引虚拟资源的拷贝任务（下载完成后执行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCopy {
    /// 源：assets/objects/<xx>/<hash>
    pub from_rel: String,
    /// 目标：assets/virtual/<index id>/<资源名> 或 resources/<资源名>
    pub to_rel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub version_id: String,
    pub chain_ids: Vec<String>,
    pub jobs: Vec<FileJob>,
    pub natives: Vec<NativeExtract>,
    pub virtual_copies: Vec<VirtualCopy>,
    /// 已知大小的总和（资产部分在索引下载后追加）。
    pub total_bytes: u64,
}

/// 从继承链构建下载计划。
pub fn build_plan(chain: &ResolvedChain, mirror: Mirror, ctx: &OsContext) -> Result<Plan> {
    let mut jobs: Vec<FileJob> = Vec::new();

    // 1) 客户端 jar
    let client_jar = chain.client_jar.as_ref().ok_or_else(|| {
        EngineError::Custom(format!(
            "无法确定客户端 jar（链上 {} 缺少 downloads.client）",
            chain.jar_id
        ))
    })?;
    jobs.push(FileJob {
        url: mirror.map_url(&client_jar.url)?,
        dest: format!("versions/{}/{}.jar", chain.jar_id, chain.jar_id),
        sha1: client_jar.sha1.clone(),
        size: client_jar.size,
        kind: FileKind::ClientJar,
    });

    // 2) libraries（含 natives 分类器 jar）
    let natives_root = format!("versions/{}/natives", chain.requested_id);
    let mut natives = Vec::new();
    for lib in &chain.libraries {
        if !evaluate_rules(lib.rules.as_deref(), ctx) {
            continue;
        }
        // 主 artifact
        if let Some(art) = library_artifact(lib, ctx, None)? {
            if let Some(job) = artifact_job(&art, mirror, FileKind::Library)? {
                jobs.push(job);
            }
        }
        // natives jar + 解压计划
        if let Some(classifier) = natives_classifier(lib, ctx) {
            if let Some(art) = library_artifact(lib, ctx, Some(&classifier))? {
                if let Some(job) = artifact_job(&art, mirror, FileKind::NativesJar)? {
                    natives.push(NativeExtract {
                        jar: job.dest.clone(),
                        dest_dir: natives_root.clone(),
                        excludes: lib
                            .extract
                            .as_ref()
                            .and_then(|e| e.exclude.clone())
                            .unwrap_or_default(),
                    });
                    jobs.push(job);
                }
            }
        }
    }

    // 3) asset index
    let idx = &chain.asset_index;
    jobs.push(FileJob {
        url: mirror.map_url(&idx.url)?,
        dest: format!("assets/indexes/{}.json", idx.id),
        sha1: idx.sha1.clone(),
        size: idx.size,
        kind: FileKind::AssetIndex,
    });

    let total_bytes = jobs.iter().filter_map(|j| j.size).sum::<u64>() + idx.total_size.unwrap_or(0);

    Ok(Plan {
        version_id: chain.requested_id.clone(),
        chain_ids: chain.entries.iter().map(|e| e.json.id.clone()).collect(),
        jobs,
        natives,
        virtual_copies: Vec::new(),
        total_bytes,
    })
}

fn artifact_job(
    art: &crate::version_json::Artifact,
    mirror: Mirror,
    kind: FileKind,
) -> Result<Option<FileJob>> {
    if art.url.is_empty() {
        return Ok(None);
    }
    let rel = match &art.path {
        Some(p) => p.clone(),
        None => return Ok(None),
    };
    // libraries 的 artifact.path 是相对 libraries/ 目录的仓库路径
    let dest = match kind {
        FileKind::Library | FileKind::NativesJar => format!("libraries/{rel}"),
        _ => rel,
    };
    Ok(Some(FileJob {
        url: mirror.map_url(&art.url)?,
        dest,
        sha1: art.sha1.clone(),
        size: art.size,
        kind,
    }))
}

// ---------------------------------------------------------------------------
// asset index 展开
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexContent {
    /// BTreeMap 保证展开顺序稳定（可复现的任务顺序）。
    #[serde(default)]
    pub objects: std::collections::BTreeMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    pub is_virtual: Option<bool>,
    #[serde(default, rename = "map_to_resources")]
    pub map_to_resources: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

const RESOURCES_HOST: &str = "https://resources.download.minecraft.net";

/// 展开资产索引为一批 Asset 下载任务（hash 即 SHA-1 与文件路径）。
/// 返回 (任务列表, 资产总字节, 虚拟拷贝列表)。
pub fn expand_asset_jobs(
    index_json: &str,
    index_id: &str,
    mirror: Mirror,
) -> Result<(Vec<FileJob>, u64, Vec<VirtualCopy>)> {
    let idx: AssetIndexContent = serde_json::from_str(index_json)?;
    let mut jobs = Vec::with_capacity(idx.objects.len());
    let mut copies = Vec::new();
    let mut total = 0u64;
    for (name, obj) in &idx.objects {
        let hash = obj.hash.to_lowercase();
        if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(EngineError::Custom(format!(
                "资产索引中的 hash 非法（{name}: {}）",
                obj.hash
            )));
        }
        let rel = format!("assets/objects/{}/{}", &hash[..2], hash);
        jobs.push(FileJob {
            url: mirror.map_url(&format!("{RESOURCES_HOST}/{}/{}", &hash[..2], hash))?,
            dest: rel.clone(),
            sha1: Some(hash.clone()),
            size: Some(obj.size),
            kind: FileKind::Asset,
        });
        total += obj.size;
        // legacy 索引：需要把对象按资源名拷贝到虚拟目录 / resources/
        let name_rel = name.replace('\\', "/");
        if name_rel.split('/').any(|seg| seg == ".." || seg.is_empty())
            || name_rel.starts_with('/')
            || name_rel.contains('\\')
        {
            return Err(EngineError::PathTraversal(name.clone()));
        }
        if idx.is_virtual == Some(true) {
            copies.push(VirtualCopy {
                from_rel: rel.clone(),
                to_rel: format!("assets/virtual/{index_id}/{name_rel}"),
            });
        }
        if idx.map_to_resources == Some(true) {
            copies.push(VirtualCopy {
                from_rel: rel,
                to_rel: format!("resources/{name_rel}"),
            });
        }
    }
    Ok((jobs, total, copies))
}

/// 校验计划中的所有目标路径都不会写出 game_dir（防御性兜底，plan 构建即失败）。
pub fn validate_plan_paths(plan: &Plan, game_dir: &std::path::Path) -> Result<()> {
    for job in &plan.jobs {
        safe_join(game_dir, &job.dest)?;
    }
    for ne in &plan.natives {
        safe_join(game_dir, &ne.jar)?;
        safe_join(game_dir, &ne.dest_dir)?;
    }
    for c in &plan.virtual_copies {
        safe_join(game_dir, &c.from_rel)?;
        safe_join(game_dir, &c.to_rel)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version_json::{
        Artifact, AssetIndexMeta, ChainEntry, Library, LibraryDownloads, VersionDownloads,
        VersionJson,
    };
    use std::collections::HashMap;

    fn vanilla_chain() -> ResolvedChain {
        let json = VersionJson {
            id: "1.20.1".into(),
            downloads: Some(VersionDownloads {
                client: Some(Artifact {
                    path: None,
                    url: "https://piston-data.mojang.com/v1/objects/abc/client.jar".into(),
                    sha1: Some("clientsha1".into()),
                    size: Some(1_000),
                }),
                extra: HashMap::new(),
            }),
            asset_index: Some(AssetIndexMeta {
                id: "8".into(),
                url: "https://piston-meta.mojang.com/v1/packages/idx/8.json".into(),
                sha1: Some("idxsha".into()),
                size: Some(100),
                total_size: Some(500),
            }),
            libraries: vec![
                Library {
                    name: Some("com.mojang:brigadier:1.0.18".into()),
                    downloads: Some(LibraryDownloads {
                        artifact: Some(Artifact {
                            path: Some("com/mojang/brigadier/brigadier/1.0.18/brigadier-1.0.18.jar".into()),
                            url: "https://libraries.minecraft.net/com/mojang/brigadier/brigadier/1.0.18/brigadier-1.0.18.jar".into(),
                            sha1: Some("b1".into()),
                            size: Some(10),
                        }),
                        classifiers: None,
                    }),
                    ..Default::default()
                },
                // natives 库（仅 windows 命中规则）
                Library {
                    name: Some("org.lwjgl:lwjgl:3.3.1".into()),
                    rules: Some(vec![crate::version_json::Rule {
                        action: "allow".into(),
                        os: Some(crate::version_json::OsRule {
                            name: Some("windows".into()),
                            arch: None,
                        }),
                        features: None,
                    }]),
                    downloads: Some(LibraryDownloads {
                        artifact: Some(Artifact {
                            path: Some("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar".into()),
                            url: "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar".into(),
                            sha1: Some("l1".into()),
                            size: Some(20),
                        }),
                        classifiers: Some(HashMap::from([(
                            "natives-windows".into(),
                            Artifact {
                                path: Some("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar".into()),
                                url: "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar".into(),
                                sha1: Some("l2".into()),
                                size: Some(30),
                            },
                        )])),
                    }),
                    natives: Some(HashMap::from([("windows".into(), "natives-windows".into())])),
                    extract: Some(crate::version_json::ExtractRules {
                        exclude: Some(vec!["META-INF/".into()]),
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        ResolvedChain {
            entries: vec![ChainEntry {
                json: json.clone(),
                raw: None,
                source_url: None,
                manifest_sha1: None,
            }],
            requested_id: "1.20.1".into(),
            jar_id: "1.20.1".into(),
            client_jar: json.downloads.as_ref().unwrap().client.clone(),
            asset_index: json.asset_index.clone().unwrap(),
            libraries: json.libraries.clone(),
        }
    }

    #[test]
    fn plan_skips_other_os_libraries_on_linux() {
        let chain = vanilla_chain();
        let ctx = OsContext {
            os: crate::version_json::OsName::Linux,
            arch: "x64",
            features: HashMap::new(),
        };
        let plan = build_plan(&chain, Mirror::Official, &ctx).unwrap();
        // client jar + brigadier + asset index；lwjgl 整库规则拒绝（linux 不允许）→ jar 与 natives 均不出现
        assert_eq!(plan.jobs.len(), 3);
        assert!(plan.natives.is_empty());
        assert!(plan.jobs.iter().all(|j| !j.dest.contains("lwjgl")));
        let jar_job = plan
            .jobs
            .iter()
            .find(|j| j.kind == FileKind::ClientJar)
            .unwrap();
        assert_eq!(jar_job.dest, "versions/1.20.1/1.20.1.jar");
        assert_eq!(
            jar_job.url,
            "https://piston-data.mojang.com/v1/objects/abc/client.jar"
        );
    }

    #[test]
    fn plan_on_windows_includes_natives_and_extract_plan() {
        let chain = vanilla_chain();
        let ctx = OsContext {
            os: crate::version_json::OsName::Windows,
            arch: "x64",
            features: HashMap::new(),
        };
        let plan = build_plan(&chain, Mirror::Official, &ctx).unwrap();
        let natives_jobs = plan
            .jobs
            .iter()
            .filter(|j| j.kind == FileKind::NativesJar)
            .count();
        assert_eq!(natives_jobs, 1);
        assert_eq!(plan.natives.len(), 1);
        assert_eq!(plan.natives[0].dest_dir, "versions/1.20.1/natives");
        assert_eq!(plan.natives[0].excludes, vec!["META-INF/".to_string()]);
    }

    #[test]
    fn plan_total_bytes_sums_known_sizes() {
        let chain = vanilla_chain();
        let ctx = OsContext {
            os: crate::version_json::OsName::Linux,
            arch: "x64",
            features: HashMap::new(),
        };
        let plan = build_plan(&chain, Mirror::Official, &ctx).unwrap();
        // client 1000 + brigadier 10 + index 100 + assets 500
        assert_eq!(plan.total_bytes, 1_610);
    }

    #[test]
    fn expand_assets_builds_objects_and_virtual_copies() {
        let index = r#"{
            "objects": {
                "minecraft/sounds/click.ogg": { "hash": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "size": 10 },
                "lang/en_us.lang": { "hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "size": 20 }
            },
            "virtual": true
        }"#;
        let (jobs, total, copies) = expand_asset_jobs(index, "legacy", Mirror::Official).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(total, 30);
        // BTreeMap 顺序：lang < minecraft；大写 hash 归一化为小写路径
        assert_eq!(
            jobs[0].dest,
            "assets/objects/bb/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(
            jobs[1].dest,
            "assets/objects/aa/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            jobs[1].url,
            "https://resources.download.minecraft.net/aa/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(copies.len(), 2);
        assert_eq!(
            copies[1].to_rel,
            "assets/virtual/legacy/minecraft/sounds/click.ogg"
        );
    }

    #[test]
    fn expand_assets_rejects_bad_hash() {
        let index = r#"{ "objects": { "x": { "hash": "zz", "size": 1 } } }"#;
        assert!(expand_asset_jobs(index, "8", Mirror::Official).is_err());
    }

    #[test]
    fn expand_assets_rejects_path_traversal_names() {
        let index = r#"{ "objects": { "../evil": { "hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size": 1 } }, "virtual": true }"#;
        assert!(expand_asset_jobs(index, "legacy", Mirror::Official).is_err());
    }
}

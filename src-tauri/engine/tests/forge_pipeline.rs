//! 真实 Forge 安装管线验证（需要网络 + Java，默认 #[ignore]）。
//!
//! 流程与 runner 的 Forge 阶段一致：
//! 原版 version.json + 客户端 jar → 安装器 → profile 库下载 → 内嵌产物提取 →
//! processors 执行 → PATCHED jar 的 SHA-1 必须与 install_profile 中声明的
//! PATCHED_SHA 一致（即与官方安装器产物逐字节一致）。

use std::path::Path;

use mc_dl_engine::downloader::{build_http_client, download_file};
use mc_dl_engine::forge::{
    ensure_launcher_profiles_stub, extract_embedded_maven, fetch_forge_builds,
    fetch_forge_version_entry, locate_java, profile_library_jobs, run_client_processors,
    ProcessorCtx,
};
use mc_dl_engine::game_dir::ensure_game_skeleton;
use mc_dl_engine::manifest::fetch_manifest;
use mc_dl_engine::mirror::Mirror;
use mc_dl_engine::version_json::{fetch_version_json_by_summary, resolve_version, ManifestHandle};
use tokio_util::sync::CancellationToken;

const MC_VERSION: &str = "1.20.1";
const FORGE_VERSION: &str = "47.3.0";

#[tokio::test]
#[ignore = "需要网络与本地 Java；真实下载 ~70MB 并执行官方安装器 processors"]
async fn forge_modern_pipeline_produces_official_artifacts() {
    let cancel = CancellationToken::new();
    let client = build_http_client().unwrap();
    // BMCLAPI：本测试环境官方源较慢，镜像优先（产物与官方一致，SHA-1 终检）
    let mirror = Mirror::Bmclapi;

    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().to_path_buf();
    ensure_game_skeleton(&game).unwrap();

    // ---- 1) 原版 1.20.1：version.json + 客户端 jar（不下载 assets，processors 不需要）----
    let manifest = fetch_manifest(&client, mirror).await.unwrap();
    let summary = manifest.find(MC_VERSION).unwrap().clone();
    let vanilla_entry = fetch_version_json_by_summary(&client, mirror, &summary)
        .await
        .unwrap();
    let vanilla_json_text = vanilla_entry.raw.clone().unwrap();
    let handle = ManifestHandle::new(&client, mirror);
    let chain = resolve_version(&client, mirror, &game, vanilla_entry, &handle)
        .await
        .unwrap();
    let vdir = game.join(format!("versions/{MC_VERSION}"));
    tokio::fs::create_dir_all(&vdir).await.unwrap();
    tokio::fs::write(vdir.join(format!("{MC_VERSION}.json")), vanilla_json_text)
        .await
        .unwrap();
    let client_jar = chain.client_jar.as_ref().unwrap();
    download_file(
        &client,
        &mirror.map_url(&client_jar.url).unwrap(),
        &vdir.join(format!("{MC_VERSION}.jar")),
        client_jar.sha1.as_deref(),
        client_jar.size,
        &cancel,
        &|_| {},
    )
    .await
    .unwrap();

    // ---- 2) 安装器 → profile + 内嵌 version.json ----
    let builds = fetch_forge_builds(&client, MC_VERSION).await.unwrap();
    let build = builds
        .iter()
        .find(|b| b.version == FORGE_VERSION)
        .expect("构建列表应包含 47.3.0")
        .clone();
    let (forge_entry, profile, installer) = fetch_forge_version_entry(
        &client,
        mirror,
        &build.artifact_id(),
        build.installer_sha1(),
        &game,
        &cancel,
    )
    .await
    .unwrap();
    let forge_id = forge_entry.json.id.clone();
    let fdir = game.join(format!("versions/{forge_id}"));
    tokio::fs::create_dir_all(&fdir).await.unwrap();
    tokio::fs::write(
        fdir.join(format!("{forge_id}.json")),
        forge_entry.raw.clone().unwrap(),
    )
    .await
    .unwrap();

    // ---- 3) profile 声明的库下载到 libraries/ ----
    let jobs = profile_library_jobs(&profile, mirror, "https://maven.minecraftforge.net/").unwrap();
    assert!(!jobs.is_empty());
    for job in &jobs {
        let dest = game.join(&job.dest);
        download_file(
            &client,
            &job.url,
            &dest,
            job.sha1.as_deref(),
            job.size,
            &cancel,
            &|_| {},
        )
        .await
        .unwrap_or_else(|e| panic!("下载 {} 失败: {e}", job.url));
    }

    // ---- 4) 内嵌 maven 产物提取 ----
    // 现代（1.13+）安装器产物在 data/ 中、库全部有下载 url，maven/ 提取数可为 0；
    // 旧版（≤1.12.2）的 universal 只内嵌在 maven/ 中（url 为空），靠此步提取。
    ensure_launcher_profiles_stub(&game).await.unwrap();
    let _extracted = extract_embedded_maven(&installer, &profile, &game.join("libraries")).unwrap();
    let aid = build.artifact_id();
    let universal = game.join(format!(
        "libraries/net/minecraftforge/forge/{aid}/forge-{aid}-universal.jar"
    ));
    assert!(universal.exists(), "universal.jar 应就位（下载或提取）");

    // ---- 5) processors ----
    assert!(profile.has_client_processors());
    let java = locate_java(None).expect("测试机需要 Java");
    let work_dir = installer.parent().unwrap().join("work");
    let ctx = ProcessorCtx {
        game_dir: &game,
        installer: &installer,
        work_dir: &work_dir,
        data: &profile.data,
        mc_version: MC_VERSION,
    };
    run_client_processors(&java, &profile, &ctx, &cancel, &|_| {})
        .await
        .expect("processors 应全部成功");

    // ---- 6) PATCHED jar 与官方声明的 SHA-1 逐字节一致 ----
    let patched_path = mc_dl_engine::forge::resolve_token("{PATCHED}", &ctx).unwrap();
    let expect_sha = profile
        .data
        .get("PATCHED_SHA")
        .and_then(|v| {
            v.client
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .map(str::to_string)
        })
        .expect("PATCHED_SHA 应存在");
    let actual = mc_dl_engine::downloader::hash_file(Path::new(&patched_path))
        .await
        .unwrap();
    assert_eq!(actual, expect_sha, "自研管线的补丁产物必须与官方安装器一致");
}

#[tokio::test]
#[ignore = "需要网络：NeoForge 版本列表 + 安装器解析（约 7MB，不执行 processors）"]
async fn neoforge_installer_parses() {
    use mc_dl_engine::forge::{
        fetch_neoforge_version_entry, fetch_neoforge_versions, profile_library_jobs,
    };
    let client = build_http_client().unwrap();
    let mirror = Mirror::Bmclapi;
    let cancel = CancellationToken::new();

    let vs = fetch_neoforge_versions(&client, mirror, "1.20.4").await.unwrap();
    assert!(!vs.is_empty(), "1.20.4 应有稳定 NeoForge 版本");
    let latest = vs[0].clone();

    let tmp = tempfile::tempdir().unwrap();
    let (entry, profile, installer) =
        fetch_neoforge_version_entry(&client, mirror, &latest, tmp.path(), &cancel)
            .await
            .unwrap();
    assert_eq!(entry.json.id, format!("neoforge-{latest}"));
    assert_eq!(entry.json.inherits_from.as_deref(), Some("1.20.4"));
    assert!(profile.has_client_processors(), "20.4 应含 client processors");
    assert!(installer.exists());

    let jobs = profile_library_jobs(&profile, mirror, mirror.neoforge_maven_base()).unwrap();
    assert!(jobs.len() > 100, "profile 库应超过 100 项，实际 {}", jobs.len());
    assert!(jobs.iter().all(|j| j.url.starts_with("https://")));
}

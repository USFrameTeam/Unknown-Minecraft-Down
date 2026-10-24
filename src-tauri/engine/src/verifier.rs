//! 安装校验：重建下载计划并逐文件核对 SHA-1，报告缺失/损坏清单。
//! 本地版本 JSON 齐全时可完全离线校验。

use serde::{Deserialize, Serialize};

use crate::downloader::{build_http_client, hash_file};
use crate::game_dir::{safe_join, PathGuard};
use crate::mirror::Mirror;
use crate::plan::{expand_asset_jobs, FileJob};
use crate::version_json::{load_or_fetch_version_json, resolve_version, ManifestHandle, OsContext};
use crate::{EngineError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportEntry {
    pub path: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub expected_sha1: Option<String>,
    #[serde(default)]
    pub actual_sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub version_id: String,
    pub game_dir: String,
    pub ok: bool,
    pub checked_files: u64,
    pub missing: Vec<ReportEntry>,
    pub corrupted: Vec<ReportEntry>,
    pub total_bytes: u64,
}

/// 校验某个版本的安装完整性。返回的报告由前端以表格展示。
pub async fn verify_installation(
    mirror: Mirror,
    game_dir: &std::path::Path,
    version_id: &str,
) -> Result<VerificationReport> {
    let client = build_http_client()?;
    let manifest = ManifestHandle::new(&client, mirror);
    let game_dir = game_dir.to_path_buf();

    let entry =
        load_or_fetch_version_json(&client, mirror, &game_dir, version_id, &manifest).await?;
    let chain = resolve_version(&client, mirror, &game_dir, entry, &manifest).await?;

    let ctx = OsContext::detect();
    let mut plan = crate::plan::build_plan(&chain, mirror, &ctx)?;

    // 资产任务：索引文件存在才展开；否则把索引本身记为缺失
    let idx_id = chain.asset_index.id.clone();
    let index_path = safe_join(&game_dir, &format!("assets/indexes/{idx_id}.json"))?;
    let mut extra_missing = Vec::new();
    match tokio::fs::read_to_string(&index_path).await {
        Ok(text) => {
            let (asset_jobs, _total, _copies) = expand_asset_jobs(&text, &idx_id, mirror)?;
            plan.jobs.extend(asset_jobs);
        }
        Err(_) => extra_missing.push(ReportEntry {
            path: format!("assets/indexes/{idx_id}.json"),
            url: Some(chain.asset_index.url.clone()),
            expected_sha1: chain.asset_index.sha1.clone(),
            actual_sha1: None,
            size: None,
        }),
    }

    let guard = std::sync::Arc::new(PathGuard::new());
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    let mut handles = Vec::new();
    for job in plan.jobs {
        let Ok(permit) = sem.clone().acquire_owned().await else {
            break;
        };
        let gd = game_dir.clone();
        let guard = guard.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            check_one(&gd, &guard, &job).await
        }));
    }

    let mut missing = extra_missing;
    let mut corrupted = Vec::new();
    let mut checked: u64 = 0;
    let mut total_bytes: u64 = 0;
    for h in handles {
        let outcome = h
            .await
            .map_err(|e| EngineError::Custom(format!("校验线程崩溃: {e}")))?;
        match outcome {
            CheckOutcome::Ok(size) => {
                checked += 1;
                total_bytes += size;
            }
            CheckOutcome::Missing(entry) => missing.push(entry),
            CheckOutcome::Corrupted(entry) => corrupted.push(entry),
        }
    }

    Ok(VerificationReport {
        version_id: chain.requested_id,
        game_dir: game_dir.display().to_string(),
        ok: missing.is_empty() && corrupted.is_empty(),
        checked_files: checked,
        missing,
        corrupted,
        total_bytes,
    })
}

enum CheckOutcome {
    Ok(u64),
    Missing(ReportEntry),
    Corrupted(ReportEntry),
}

async fn check_one(game_dir: &std::path::Path, guard: &PathGuard, job: &FileJob) -> CheckOutcome {
    let entry = |actual: Option<String>| ReportEntry {
        path: job.dest.clone(),
        url: Some(job.url.clone()),
        expected_sha1: job.sha1.clone(),
        actual_sha1: actual,
        size: job.size,
    };

    let path = match safe_join(game_dir, &job.dest) {
        Ok(p) => p,
        Err(_) => return CheckOutcome::Corrupted(entry(None)),
    };
    if guard.ensure_within(game_dir, &path).is_err() {
        return CheckOutcome::Corrupted(entry(None));
    }
    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => return CheckOutcome::Missing(entry(None)),
    };
    let actual = match hash_file(&path).await {
        Ok(a) => a,
        Err(e) => {
            return CheckOutcome::Corrupted(entry(Some(e.to_string())));
        }
    };
    match &job.sha1 {
        Some(expect) if actual != expect.to_lowercase() => {
            CheckOutcome::Corrupted(entry(Some(actual)))
        }
        _ => CheckOutcome::Ok(meta.len()),
    }
}

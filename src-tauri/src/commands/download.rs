//! 下载相关 Tauri commands：版本列表、任务生命周期、安装校验。
//!
//! 事件模型：引擎快照 → entry 状态 + `download://progress` 事件 + 节流持久化。
//! 取消/暂停：通过任务自己的 CancellationToken 通知引擎，暂停时由转发器
//! 把引擎的 Canceled 映射为 Paused。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, State};

use mc_dl_engine::downloader::build_http_client;
use mc_dl_engine::forge::ForgeBuild;
use mc_dl_engine::manifest::{fetch_manifest, VersionSummary};
use mc_dl_engine::runner::{
    fetch_fabric_loaders, run_task, FabricLoaderSummary, TaskSnapshot, TaskSpec, TaskStatus,
    VersionSource, EVENT_PROGRESS,
};
use mc_dl_engine::verifier::{verify_installation as engine_verify, VerificationReport};
use mc_dl_engine::Mirror;

use crate::state::{AppState, TaskEntry};
use crate::AppError;

/// Forge 构建摘要（前端展示与任务参数用）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeBuildSummary {
    pub mc_version: String,
    pub forge_version: String,
    pub branch: Option<String>,
    pub artifact_id: String,
    pub version_id: String,
    pub build: i64,
    pub modified: String,
}

impl From<ForgeBuild> for ForgeBuildSummary {
    fn from(b: ForgeBuild) -> Self {
        Self {
            mc_version: b.mcversion.clone(),
            forge_version: b.version.clone(),
            branch: b.branch.clone(),
            artifact_id: b.artifact_id(),
            version_id: b.version_id(),
            build: b.build,
            modified: b.modified,
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `list_versions(mirror) -> Vec<VersionSummary>`（带 5 分钟缓存）。
#[tauri::command]
pub async fn list_versions(
    state: State<'_, AppState>,
    mirror: Mirror,
) -> Result<Vec<VersionSummary>, AppError> {
    if let Some((at, cached)) = state.manifest_cache.lock().unwrap().clone() {
        if at.elapsed() < Duration::from_secs(300) {
            return Ok(cached);
        }
    }
    let client = build_http_client()?;
    let manifest = fetch_manifest(&client, mirror).await?;
    let versions = manifest.versions;
    *state.manifest_cache.lock().unwrap() = Some((Instant::now(), versions.clone()));
    Ok(versions)
}

/// Fabric Loader 版本列表（官方 meta.fabricmc.net）。
#[tauri::command]
pub async fn list_fabric_loaders(
    mc_version: String,
) -> Result<Vec<FabricLoaderSummary>, AppError> {
    let client = build_http_client()?;
    Ok(fetch_fabric_loaders(&client, &mc_version).await?)
}

/// Forge 构建列表（BMCLAPI，含安装器 SHA-1）。
#[tauri::command]
pub async fn list_forge_builds(
    mc_version: String,
) -> Result<Vec<ForgeBuildSummary>, AppError> {
    let client = build_http_client()?;
    let builds = mc_dl_engine::forge::fetch_forge_builds(&client, &mc_version).await?;
    Ok(builds.into_iter().map(Into::into).collect())
}

/// `start_download(...) -> DownloadTaskId`
#[tauri::command]
pub async fn start_download(
    app: AppHandle,
    state: State<'_, AppState>,
    source: VersionSource,
    game_dir: String,
    mirror: Mirror,
    concurrency: u32,
    java_path: Option<String>,
) -> Result<String, AppError> {
    let version_id = source.display_id();
    let task_id = format!("{}-{}", version_id, now_ms());
    let spec = TaskSpec {
        version_id: version_id.clone(),
        source,
        game_dir,
        mirror,
        concurrency: concurrency.clamp(1, 32),
        java_path,
    };
    let entry = register_task(&state, spec, task_id.clone())?;
    run_entry(&app, entry);
    Ok(task_id)
}

/// 创建任务条目（初始状态 Queued），供 start_download 与启动恢复共用。
pub fn register_task(
    state: &AppState,
    spec: TaskSpec,
    task_id: String,
) -> Result<Arc<TaskEntry>, AppError> {
    let entry = Arc::new(TaskEntry {
        task_id: task_id.clone(),
        spec: spec.clone(),
        cancel: tokio_util::sync::CancellationToken::new(),
        pause_requested: std::sync::atomic::AtomicBool::new(false),
        snapshot: std::sync::RwLock::new(TaskSnapshot {
            task_id: task_id.clone(),
            version_id: spec.version_id.clone(),
            game_dir: spec.game_dir.clone(),
            mirror: spec.mirror,
            status: TaskStatus::Queued,
            stage: None,
            downloaded_bytes: 0,
            total_bytes: 0,
            speed_bps: 0,
            current_file: None,
            files_done: 0,
            files_total: 0,
            error: None,
            created_at: now_ms(),
            finished_at: None,
        }),
    });
    state.tasks.lock().unwrap().insert(task_id, entry.clone());
    Ok(entry)
}

/// 启动引擎并接管事件转发。可在 command 线程或启动恢复时调用。
pub fn run_entry(app: &AppHandle, entry: Arc<TaskEntry>) {
    {
        let mut snap = entry.snapshot.write().unwrap();
        snap.status = TaskStatus::Running;
        snap.error = None;
        snap.finished_at = None;
    }
    entry.pause_requested.store(false, Ordering::SeqCst);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TaskSnapshot>(256);
    let cancel = entry.cancel.clone();
    let spec = entry.spec.clone();
    let task_id = entry.task_id.clone();
    tauri::async_runtime::spawn(run_task(spec, task_id, cancel, tx));

    let app = app.clone();
    let entry = entry.clone();
    tauri::async_runtime::spawn(async move {
        let persist_every = Duration::from_secs(1);
        let mut last_persist = Instant::now() - persist_every;
        while let Some(mut snap) = rx.recv().await {
            // 暂停请求：把引擎的 Canceled 映射为 Paused
            if snap.status == TaskStatus::Canceled
                && entry.pause_requested.load(Ordering::SeqCst)
            {
                snap.status = TaskStatus::Paused;
            }
            *entry.snapshot.write().unwrap() = snap.clone();
            let _ = app.emit(EVENT_PROGRESS, &snap);
            let is_final = matches!(
                snap.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Canceled
            );
            if is_final || last_persist.elapsed() >= persist_every {
                app.state::<AppState>().persist_tasks();
                last_persist = Instant::now();
            }
            if is_final {
                break;
            }
        }
        // 引擎已收尾（或转发端提前退出），确保磁盘状态一致
        app.state::<AppState>().persist_tasks();
    });
}

#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>, task_id: String) -> Result<(), AppError> {
    let entry = get_entry(&state, &task_id)?;
    entry.pause_requested.store(false, Ordering::SeqCst);
    entry.cancel.cancel();
    Ok(())
}

#[tauri::command]
pub async fn pause_download(state: State<'_, AppState>, task_id: String) -> Result<(), AppError> {
    let entry = get_entry(&state, &task_id)?;
    entry.pause_requested.store(true, Ordering::SeqCst);
    entry.cancel.cancel();
    Ok(())
}

#[tauri::command]
pub async fn resume_download(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), AppError> {
    let entry = get_entry(&state, &task_id)?;
    if !entry.is_final() {
        return Ok(()); // 正在运行，无需恢复
    }
    run_entry(&app, entry);
    Ok(())
}

#[tauri::command]
pub fn get_download_state(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<TaskSnapshot, AppError> {
    Ok(get_entry(&state, &task_id)?.snapshot.read().unwrap().clone())
}

#[tauri::command]
pub fn list_download_tasks(state: State<'_, AppState>) -> Result<Vec<TaskSnapshot>, AppError> {
    let tasks = state.tasks.lock().unwrap();
    let mut snaps: Vec<TaskSnapshot> = tasks
        .values()
        .map(|e| e.snapshot.read().unwrap().clone())
        .collect();
    snaps.sort_by_key(|s| s.created_at);
    Ok(snaps)
}

#[tauri::command]
pub fn clear_finished_tasks(state: State<'_, AppState>) -> Result<(), AppError> {
    let mut tasks = state.tasks.lock().unwrap();
    let finished: Vec<String> = tasks
        .iter()
        .filter(|(_, e)| e.is_final())
        .map(|(k, _)| k.clone())
        .collect();
    for k in finished {
        tasks.remove(&k);
    }
    drop(tasks);
    state.persist_tasks();
    Ok(())
}

/// `verify_installation(version_id, game_dir) -> VerificationReport`
#[tauri::command]
pub async fn verify_installation(
    version_id: String,
    game_dir: String,
    mirror: Mirror,
) -> Result<VerificationReport, AppError> {
    Ok(engine_verify(mirror, std::path::Path::new(&game_dir), &version_id).await?)
}

fn get_entry(state: &AppState, task_id: &str) -> Result<Arc<TaskEntry>, AppError> {
    state
        .tasks
        .lock()
        .unwrap()
        .get(task_id)
        .cloned()
        .ok_or_else(|| AppError::new("task_not_found", format!("任务不存在: {task_id}")))
}

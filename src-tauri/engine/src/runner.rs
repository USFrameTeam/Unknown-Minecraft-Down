//! 下载任务编排：阶段推进、并发控制、取消、进度快照、natives 解压与虚拟资源拷贝。
//!
//! 阶段顺序：manifest → version_json（解析继承链）→ client_jar/libraries（阶段 A
//! 文件任务）→ assets（索引展开后阶段 B）→ natives 解压/legacy 拷贝 → finished。
//! 进度快照每 500ms 经由 mpsc 通道发出，阶段切换立即发一次。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::downloader::{build_http_client, download_file};
use crate::game_dir::{ensure_game_skeleton, safe_join, PathGuard};
use crate::mirror::Mirror;
use crate::plan::{expand_asset_jobs, validate_plan_paths, FileJob};
use crate::version_json::{
    load_or_fetch_version_json, resolve_version, ChainEntry, ManifestHandle, OsContext,
    VersionJson,
};
use crate::{EngineError, Result};

// ---------------------------------------------------------------------------
// 对外类型（Tauri 应用层直接序列化给前端）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Manifest,
    VersionJson,
    Libraries,
    Assets,
    Forge,
    Verifying,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub task_id: String,
    pub version_id: String,
    pub game_dir: String,
    pub mirror: Mirror,
    pub status: TaskStatus,
    #[serde(default)]
    pub stage: Option<Stage>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bps: u64,
    #[serde(default)]
    pub current_file: Option<String>,
    pub files_done: u64,
    pub files_total: u64,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub finished_at: Option<u64>,
}

/// 版本来源：原版（清单 id）、Fabric Loader（Fabric Meta）或 Forge（官方安装器）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VersionSource {
    Vanilla { id: String },
    Fabric { mc_version: String, loader_version: String },
    Forge {
        mc_version: String,
        forge_version: String,
        #[serde(default)]
        branch: Option<String>,
    },
}

impl VersionSource {
    pub fn display_id(&self) -> String {
        match self {
            VersionSource::Vanilla { id } => id.clone(),
            VersionSource::Fabric {
                mc_version,
                loader_version,
            } => format!("fabric-loader-{loader_version}-{mc_version}"),
            VersionSource::Forge {
                mc_version,
                forge_version,
                branch,
            } => match branch {
                Some(b) if !b.is_empty() => {
                    format!("{mc_version}-forge-{forge_version}-{b}")
                }
                _ => format!("{mc_version}-forge-{forge_version}"),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSpec {
    pub version_id: String,
    pub source: VersionSource,
    pub game_dir: String,
    pub mirror: Mirror,
    /// 并发数 1..=32
    pub concurrency: u32,
    /// Forge processors 使用的 Java 路径（None = 自动探测）
    #[serde(default)]
    pub java_path: Option<String>,
}

pub const EVENT_PROGRESS: &str = "download://progress";

/// Fabric 官方 Meta：返回带 inheritsFrom 的 profile JSON。
/// BMCLAPI 不镜像 Fabric Meta，但 meta.fabricmc.net 是官方 HTTPS 源。
pub async fn fetch_fabric_profile(
    client: &reqwest::Client,
    mc_version: &str,
    loader_version: &str,
) -> Result<ChainEntry> {
    let url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{mc_version}/{loader_version}/profile/json"
    );
    let resp = client.get(&url).send().await?.error_for_status()?;
    let text = resp.text().await?;
    let json: VersionJson = serde_json::from_str(&text)?;
    Ok(ChainEntry {
        json,
        raw: Some(text),
        source_url: Some(url),
        manifest_sha1: None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricLoaderSummary {
    pub loader: FabricLoaderRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricLoaderRef {
    pub version: String,
    pub stable: bool,
}

/// 列出某个 MC 版本可用的 Fabric Loader 版本（官方 Meta v2）。
pub async fn fetch_fabric_loaders(
    client: &reqwest::Client,
    mc_version: &str,
) -> Result<Vec<FabricLoaderSummary>> {
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc_version}");
    let resp = client.get(&url).send().await?.error_for_status()?;
    Ok(resp.json::<Vec<FabricLoaderSummary>>().await?)
}

// ---------------------------------------------------------------------------
// 运行时共享状态
// ---------------------------------------------------------------------------

struct SpeedWindow {
    last_bytes: u64,
    last_time: Instant,
    bps: f64,
}

pub struct Runtime {
    cancel: CancellationToken,
    downloaded: AtomicU64,
    files_done: AtomicU64,
    files_total: AtomicU64,
    total_bytes: AtomicU64,
    stage: Mutex<Option<Stage>>,
    current_file: Mutex<Option<String>>,
    speed: Mutex<SpeedWindow>,
    paths: PathGuard,
}

impl Runtime {
    fn new(cancel: CancellationToken) -> Self {
        Self {
            cancel,
            downloaded: AtomicU64::new(0),
            files_done: AtomicU64::new(0),
            files_total: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            stage: Mutex::new(None),
            current_file: Mutex::new(None),
            speed: Mutex::new(SpeedWindow {
                last_bytes: 0,
                last_time: Instant::now(),
                bps: 0.0,
            }),
            paths: PathGuard::new(),
        }
    }

    fn set_stage(&self, s: Stage) {
        *self.stage.lock().unwrap() = Some(s);
    }

    pub(crate) fn set_current_file(&self, f: &str) {
        *self.current_file.lock().unwrap() = Some(f.to_string());
    }

    fn snapshot(
        &self,
        base: &SnapshotBase,
        status: TaskStatus,
        error: Option<String>,
    ) -> TaskSnapshot {
        let now_bytes = self.downloaded.load(Ordering::Relaxed);
        let speed_bps = {
            let mut w = self.speed.lock().unwrap();
            let t = Instant::now();
            let dt = t.duration_since(w.last_time).as_secs_f64().max(0.2);
            let raw = ((now_bytes as f64) - (w.last_bytes as f64)).max(0.0) / dt;
            // 指数平滑，避免速度数字跳动
            w.bps = if w.bps == 0.0 { raw } else { 0.6 * w.bps + 0.4 * raw };
            w.last_time = t;
            w.last_bytes = now_bytes;
            w.bps as u64
        };
        TaskSnapshot {
            task_id: base.task_id.clone(),
            version_id: base.version_id.clone(),
            game_dir: base.game_dir.clone(),
            mirror: base.mirror,
            status,
            stage: *self.stage.lock().unwrap(),
            downloaded_bytes: now_bytes,
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            speed_bps,
            current_file: self.current_file.lock().unwrap().clone(),
            files_done: self.files_done.load(Ordering::Relaxed),
            files_total: self.files_total.load(Ordering::Relaxed),
            error,
            created_at: base.created_at,
            finished_at: matches!(
                status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Canceled
            )
            .then(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            }),
        }
    }
}

#[derive(Clone)]
struct SnapshotBase {
    task_id: String,
    version_id: String,
    game_dir: String,
    mirror: Mirror,
    created_at: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 任务主流程
// ---------------------------------------------------------------------------

/// 运行一个下载任务，过程中通过 `events_tx` 广播快照。
/// `cancel` 由调用方持有（UI 的取消/暂停都通过它触发）。
/// 返回 Ok 表示完成；`Err(Canceled)` 表示被取消；其他错误为失败原因。
pub async fn run_task(
    spec: TaskSpec,
    task_id: String,
    cancel: CancellationToken,
    events_tx: mpsc::Sender<TaskSnapshot>,
) -> Result<()> {
    let base = SnapshotBase {
        task_id,
        version_id: spec.version_id.clone(),
        game_dir: spec.game_dir.clone(),
        mirror: spec.mirror,
        created_at: now_ms(),
    };
    let rt = Arc::new(Runtime::new(cancel));
    rt.total_bytes.store(0, Ordering::Relaxed);

    let done = CancellationToken::new();
    let ticker = {
        let rt = rt.clone();
        let base = base.clone();
        let done = done.clone();
        let tx = events_tx.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(500));
            loop {
                tokio::select! {
                    _ = done.cancelled() => break,
                    _ = iv.tick() => {
                        if tx.send(rt.snapshot(&base, TaskStatus::Running, None)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
    };

    let result = run_inner(&spec, &base, &rt, &events_tx).await;
    done.cancel();
    let _ = ticker.await;

    let final_snap = match &result {
        Ok(()) => rt.snapshot(&base, TaskStatus::Completed, None),
        Err(EngineError::Canceled) => rt.snapshot(&base, TaskStatus::Canceled, None),
        Err(e) => rt.snapshot(&base, TaskStatus::Failed, Some(e.to_string())),
    };
    let _ = events_tx.send(final_snap).await;
    result
}

async fn run_inner(
    spec: &TaskSpec,
    base: &SnapshotBase,
    rt: &Arc<Runtime>,
    events_tx: &mpsc::Sender<TaskSnapshot>,
) -> Result<()> {
    let game_dir = PathBuf::from(&spec.game_dir);
    let cancel = &rt.cancel;
    let client = Arc::new(build_http_client()?);
    ensure_game_skeleton(&game_dir)?;

    // ---- 阶段 1：manifest ----
    rt.set_stage(Stage::Manifest);
    let _ = events_tx
        .send(rt.snapshot(base, TaskStatus::Running, None))
        .await;
    cancel_err(cancel)?;
    let manifest_handle = ManifestHandle::new(&client, spec.mirror);

    // ---- 阶段 2：version_json（含 inheritsFrom 递归）----
    rt.set_stage(Stage::VersionJson);
    let _ = events_tx
        .send(rt.snapshot(base, TaskStatus::Running, None))
        .await;
    let mut forge_hold: Option<(crate::forge::InstallProfile, std::path::PathBuf)> = None;
    let entry = match &spec.source {
        VersionSource::Vanilla { id } => {
            load_or_fetch_version_json(&client, spec.mirror, &game_dir, id, &manifest_handle)
                .await?
        }
        VersionSource::Fabric {
            mc_version,
            loader_version,
        } => fetch_fabric_profile(&client, mc_version, loader_version).await?,
        VersionSource::Forge {
            mc_version,
            forge_version,
            branch,
        } => {
            // 构建列表（含安装器 SHA-1）→ 下载安装器 → 解析 profile 与内嵌 version.json
            let builds = crate::forge::fetch_forge_builds(&client, mc_version).await?;
            let build = builds
                .into_iter()
                .find(|b| {
                    &b.version == forge_version && b.branch.as_deref() == branch.as_deref()
                })
                .ok_or_else(|| {
                    EngineError::VersionNotFound(format!(
                        "Forge {mc_version}-{forge_version} 不在构建列表中"
                    ))
                })?;
            let (entry, profile, installer) = crate::forge::fetch_forge_version_entry(
                &client,
                spec.mirror,
                &build.artifact_id(),
                build.installer_sha1(),
                &game_dir,
                cancel,
            )
            .await?;
            forge_hold = Some((profile, installer));
            entry
        }
    };
    let chain = resolve_version(&client, spec.mirror, &game_dir, entry, &manifest_handle).await?;

    // 落盘链上所有版本 JSON（保持与来源字节一致，启动器依赖它们解析继承）
    for e in &chain.entries {
        let Some(raw) = &e.raw else { continue };
        let dest = safe_join(&game_dir, &format!("versions/{}/{}.json", e.json.id, e.json.id))?;
        write_if_changed(&dest, raw.as_bytes()).await?;
    }

    // ---- 构建下载计划 ----
    let ctx = OsContext::detect();
    let mut plan = crate::plan::build_plan(&chain, spec.mirror, &ctx)?;
    // Forge：安装器声明的库（processor 依赖等）并入下载计划
    if let Some((profile, _)) = &forge_hold {
        let extra = crate::forge::profile_library_jobs(profile, spec.mirror, &game_dir)?;
        plan.total_bytes += extra.iter().filter_map(|j| j.size).sum::<u64>();
        plan.jobs.extend(extra);
    }
    validate_plan_paths(&plan, &game_dir)?;
    rt.total_bytes.store(plan.total_bytes, Ordering::Relaxed);
    rt.files_total.store(plan.jobs.len() as u64, Ordering::Relaxed);

    // ---- 阶段 3：阶段 A 文件任务（client jar / libraries / natives jar / 索引）----
    rt.set_stage(Stage::Libraries);
    let _ = events_tx
        .send(rt.snapshot(base, TaskStatus::Running, None))
        .await;
    let phase_a_jobs = std::mem::take(&mut plan.jobs);
    run_jobs(phase_a_jobs, spec, rt, &game_dir, client.clone()).await?;

    // ---- 阶段 4：assets（依赖已下载的索引文件）----
    rt.set_stage(Stage::Assets);
    let _ = events_tx
        .send(rt.snapshot(base, TaskStatus::Running, None))
        .await;
    cancel_err(cancel)?;
    let idx_meta = &chain.asset_index;
    let index_path = safe_join(&game_dir, &format!("assets/indexes/{}.json", idx_meta.id))?;
    let index_text = tokio::fs::read_to_string(&index_path)
        .await
        .map_err(|e| {
            EngineError::Custom(format!("资产索引缺失（{}）: {e}", index_path.display()))
        })?;
    let (asset_jobs, asset_total, copies) =
        expand_asset_jobs(&index_text, &idx_meta.id, spec.mirror)?;
    rt.total_bytes.fetch_add(asset_total, Ordering::Relaxed);
    rt.files_total
        .fetch_add(asset_jobs.len() as u64, Ordering::Relaxed);
    plan.virtual_copies = copies;
    run_jobs(asset_jobs, spec, rt, &game_dir, client.clone()).await?;

    // ---- 阶段 5：Forge（内嵌产物提取 + processors，仅在 Forge 任务时）----
    if let Some((profile, installer)) = forge_hold {
        rt.set_stage(Stage::Forge);
        let _ = events_tx
            .send(rt.snapshot(base, TaskStatus::Running, None))
            .await;
        cancel_err(cancel)?;
        crate::forge::ensure_launcher_profiles_stub(&game_dir).await?;
        let extracted = crate::forge::extract_embedded_maven(
            &installer,
            &profile,
            &game_dir.join("libraries"),
        )?;
        log::info!("Forge 安装器内嵌产物提取 {extracted} 个");
        // 现代 Forge（1.13+）：执行官方安装器的 processors（需 Java）
        if profile.has_client_processors() {
            let java = crate::forge::locate_java(spec.java_path.as_deref()).ok_or_else(|| {
                EngineError::Custom(
                    "未找到可用的 Java（Forge 安装需要）。请在设置页配置 Java 路径。".into(),
                )
            })?;
            if !crate::forge::java_works(&java).await {
                return Err(EngineError::Custom(format!(
                    "Java 无法运行：{}",
                    java.display()
                )));
            }
            let work_dir = installer
                .parent()
                .map(|p| p.join("work"))
                .unwrap_or_else(std::env::temp_dir);
            let pctx = crate::forge::ProcessorCtx {
                game_dir: &game_dir,
                installer: &installer,
                work_dir: &work_dir,
                data: &profile.data,
                mc_version: &profile.minecraft,
            };
            let rt2 = rt.clone();
            crate::forge::run_client_processors(&java, &profile, &pctx, cancel, &|step| {
                rt2.set_current_file(&format!("Forge: {step}"));
            })
            .await?;
        }
    }

    // ---- 阶段 6：natives 解压 + legacy 虚拟拷贝 ----
    rt.set_stage(Stage::Verifying);
    let _ = events_tx
        .send(rt.snapshot(base, TaskStatus::Running, None))
        .await;
    for ne in &plan.natives {
        cancel_err(cancel)?;
        let ne = ne.clone();
        let gd = game_dir.clone();
        tokio::task::spawn_blocking(move || crate::runner::extract_natives(&gd, &ne))
            .await
            .map_err(|e| EngineError::Custom(format!("natives 解压任务失败: {e}")))??;
    }
    for copy in &plan.virtual_copies {
        cancel_err(cancel)?;
        let from = safe_join(&game_dir, &copy.from_rel)?;
        let to = safe_join(&game_dir, &copy.to_rel)?;
        if !to.exists() {
            if let Some(parent) = to.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(&from, &to).await?;
        }
    }

    rt.set_stage(Stage::Finished);
    Ok(())
}

fn cancel_err(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(EngineError::Canceled)
    } else {
        Ok(())
    }
}

async fn write_if_changed(dest: &std::path::Path, content: &[u8]) -> Result<()> {
    if let Ok(existing) = tokio::fs::read(dest).await {
        if existing == content {
            return Ok(());
        }
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = dest.with_extension("json.tmp");
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::rename(&tmp, dest).await?;
    Ok(())
}

/// 并发执行一批文件任务（Semaphore 控制并发数）。
async fn run_jobs(
    jobs: Vec<FileJob>,
    spec: &TaskSpec,
    rt: &Arc<Runtime>,
    game_dir: &std::path::Path,
    client: Arc<reqwest::Client>,
) -> Result<()> {
    let cancel = &rt.cancel;
    let sem = Arc::new(Semaphore::new(spec.concurrency.clamp(1, 32) as usize));
    let game_dir = game_dir.to_path_buf();
    let rt = rt.clone();

    let mut handles = Vec::with_capacity(jobs.len());
    for job in jobs {
        let permit = sem.clone().acquire_owned().await;
        let Ok(permit) = permit else { break };
        let client = client.clone();
        let rt = rt.clone();
        let game_dir = game_dir.clone();
        let cancel = cancel.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            run_file_job(client, job, game_dir, rt, cancel).await
        }));
    }

    let mut first_error: Option<EngineError> = None;
    let mut saw_cancel = false;
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(EngineError::Canceled)) => saw_cancel = true,
            Ok(Err(e)) => {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
            Err(join_err) => {
                if first_error.is_none() {
                    first_error = Some(EngineError::Custom(format!("任务线程崩溃: {join_err}")));
                }
            }
        }
    }
    if saw_cancel || cancel.is_cancelled() {
        return Err(EngineError::Canceled);
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

async fn run_file_job(
    client: Arc<reqwest::Client>,
    job: FileJob,
    game_dir: PathBuf,
    rt: Arc<Runtime>,
    cancel: CancellationToken,
) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(EngineError::Canceled);
    }
    let dest = safe_join(&game_dir, &job.dest)?;
    // 防穿越兜底（含符号链接逃逸检测）
    rt.paths.ensure_within(&game_dir, &dest)?;
    rt.set_current_file(&job.dest);

    let rt2 = rt.clone();
    let progress = move |delta: u64| {
        rt2.downloaded.fetch_add(delta, Ordering::Relaxed);
    };
    let bytes_now = download_file(
        &client,
        &job.url,
        &dest,
        job.sha1.as_deref(),
        job.size,
        &cancel,
        &progress,
    )
    .await?;

    // 修正累计字节：补上“已存在的完整文件/续传前缀”未被 progress 统计的部分
    let full = job.size.unwrap_or_else(|| {
        std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(bytes_now)
    });
    rt.downloaded
        .fetch_add(full.saturating_sub(bytes_now), Ordering::Relaxed);
    rt.files_done.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// natives 解压（防穿越）
// ---------------------------------------------------------------------------

const NATIVE_EXTS: [&str; 4] = [".dll", ".so", ".dylib", ".jnilib"];

/// 从已下载的 natives jar 中提取当前平台的动态库到 dest_dir。
/// 防穿越三重保障：只允许白名单扩展名、只取 entry 的文件名部分（丢弃目录成分）、
/// 最终写入路径经 ensure_within 兜底校验。
pub fn extract_natives(game_dir: &std::path::Path, ne: &crate::plan::NativeExtract) -> Result<()> {
    let jar_path = safe_join(game_dir, &ne.jar)?;
    let dest_root = safe_join(game_dir, &ne.dest_dir)?;
    std::fs::create_dir_all(&dest_root)?;

    let file = std::fs::File::open(&jar_path)
        .map_err(|e| EngineError::Custom(format!("打开 natives jar 失败（{}）: {e}", jar_path.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| EngineError::Custom(format!("读取 natives jar 失败: {e}")))?;
    let guard = PathGuard::new();

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| EngineError::Custom(format!("读取 natives 条目失败: {e}")))?;
        let name = entry.name().to_string();
        if !NATIVE_EXTS.iter().any(|ext| name.ends_with(ext)) {
            continue;
        }
        if ne.excludes.iter().any(|ex| name.starts_with(ex)) {
            continue;
        }
        // 只取文件名，天然丢弃 zip 内的任何目录成分与 ".." 片段
        let Some(file_name) = std::path::Path::new(&name).file_name() else {
            continue;
        };
        let file_name = file_name.to_string_lossy().to_string();
        if file_name.contains("..") {
            continue;
        }
        let out_path = dest_root.join(&file_name);
        guard.ensure_within(game_dir, &out_path)?;
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

//! 全局应用状态：任务表、设置、持久化、启动恢复。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use tauri::Manager;
use tokio_util::sync::CancellationToken;

use mc_dl_engine::manifest::VersionSummary;
use mc_dl_engine::runner::{TaskSnapshot, TaskSpec, TaskStatus};

use crate::settings::{AppSettings, DownloadsFile, TaskRecord};
use crate::AppError;

pub struct TaskEntry {
    pub task_id: String,
    pub spec: TaskSpec,
    pub cancel: CancellationToken,
    /// 暂停时为 true：转发器据此把引擎的 Canceled 映射为 Paused
    pub pause_requested: AtomicBool,
    /// 启动恢复时标记"上次会话仍在运行"，由前端就绪后调用 auto_resume_pending 续跑，
    /// 避免开屏即发起网络风暴拖慢启动
    pub auto_resume: AtomicBool,
    pub snapshot: RwLock<TaskSnapshot>,
}

impl TaskEntry {
    pub fn status(&self) -> TaskStatus {
        self.snapshot.read().unwrap().status
    }

    pub fn is_final(&self) -> bool {
        matches!(
            self.status(),
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Canceled
        )
    }
}

pub struct AppState {
    pub tasks: Mutex<HashMap<String, Arc<TaskEntry>>>,
    pub settings: RwLock<AppSettings>,
    pub data_dir: PathBuf,
    /// version manifest 的轻量缓存（5 分钟），避免版本页反复拉取
    pub manifest_cache: Mutex<Option<(Instant, Vec<VersionSummary>)>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            settings: RwLock::new(AppSettings::default()),
            data_dir,
            manifest_cache: Mutex::new(None),
        }
    }

    // ---- settings ----

    pub fn load_settings(&self) {
        if let Some(mut s) = crate::persistence::read_json::<AppSettings>(&self.settings_path()) {
            s.normalize();
            *self.settings.write().unwrap() = s;
        }
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<(), AppError> {
        *self.settings.write().unwrap() = s.clone();
        crate::persistence::write_json_atomic(&self.settings_path(), s)
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    // ---- tasks ----

    pub fn load_downloads(&self) -> DownloadsFile {
        crate::persistence::read_json::<DownloadsFile>(&self.downloads_path()).unwrap_or_default()
    }

    pub fn persist_tasks(&self) {
        let records: Vec<TaskRecord> = {
            let tasks = self.tasks.lock().unwrap();
            let mut records: Vec<TaskRecord> = tasks
                .values()
                .map(|e| {
                    let snap = e.snapshot.read().unwrap();
                    TaskRecord {
                        task_id: e.task_id.clone(),
                        version_id: e.spec.version_id.clone(),
                        source: e.spec.source.clone(),
                        game_dir: e.spec.game_dir.clone(),
                        mirror: e.spec.mirror,
                        concurrency: e.spec.concurrency,
                        status: snap.status,
                        downloaded_bytes: snap.downloaded_bytes,
                        total_bytes: snap.total_bytes,
                        created_at: snap.created_at,
                    }
                })
                .collect();
            drop(tasks);
            records.sort_by_key(|r| r.created_at);
            records
        };
        let file = DownloadsFile { tasks: records };
        if let Err(e) = crate::persistence::write_json_atomic(&self.downloads_path(), &file) {
            log::error!("持久化下载任务失败: {e}");
        }
    }

    pub fn downloads_path(&self) -> PathBuf {
        self.data_dir.join("downloads.json")
    }

    /// 前端就绪后调用：续跑上次会话仍在运行的任务（每任务回退到 AppHandle）。
    pub fn auto_resume_pending(&self, app: &tauri::AppHandle) -> usize {
        let entries: Vec<Arc<TaskEntry>> = self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|e| {
                e.auto_resume.load(std::sync::atomic::Ordering::SeqCst)
                    && e.status() == TaskStatus::Paused
            })
            .cloned()
            .collect();
        let n = entries.len();
        for e in entries {
            e.auto_resume
                .store(false, std::sync::atomic::Ordering::SeqCst);
            crate::commands::download::run_entry(app, e);
        }
        n
    }

    /// 启动恢复：未结束任务全部重建为 Paused（不立即续跑）；
    /// 上次为 Running/Queued 的任务标记 auto_resume，待前端就绪后由
    /// `auto_resume_pending` 续跑。引擎按磁盘现状推进，进度不归零。
    pub fn bootstrap_from_disk(&self) {
        let file = self.load_downloads();
        for record in file.tasks {
            let java_path = self.settings.read().unwrap().java_path.clone();
            let spec = TaskSpec {
                version_id: record.version_id.clone(),
                source: record.source.clone(),
                game_dir: record.game_dir.clone(),
                mirror: record.mirror,
                concurrency: record.concurrency.clamp(1, 32),
                java_path,
            };
            let was_running = matches!(record.status, TaskStatus::Running | TaskStatus::Queued);
            match crate::commands::download::register_task(self, spec, record.task_id.clone()) {
                Ok(entry) => {
                    {
                        let mut snap = entry.snapshot.write().unwrap();
                        snap.status = TaskStatus::Paused;
                        snap.downloaded_bytes = record.downloaded_bytes;
                        snap.total_bytes = record.total_bytes;
                    }
                    entry
                        .auto_resume
                        .store(was_running, std::sync::atomic::Ordering::SeqCst);
                }
                Err(e) => log::error!("恢复任务 {} 失败: {e}", record.task_id),
            }
        }
        self.persist_tasks();
    }
}

/// 应用启动时调用：初始化数据目录、加载设置、恢复任务队列。
pub fn bootstrap(app: tauri::AppHandle) {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("unknown-minecraft-down"));
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        log::error!("创建数据目录失败: {e}");
    }
    let state = AppState::new(data_dir);
    state.load_settings();
    app.manage(state);
    let state = app.state::<AppState>();
    state.bootstrap_from_disk();
}

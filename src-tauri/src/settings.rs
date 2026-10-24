//! 用户设置与任务记录的持久化模型（app_data_dir 下 settings.json / downloads.json）。

use serde::{Deserialize, Serialize};

use mc_dl_engine::runner::{TaskStatus, VersionSource};
use mc_dl_engine::Mirror;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// 首次启动向导是否完成
    pub onboarded: bool,
    /// 已确认的 .minecraft 目录
    pub game_dir: Option<String>,
    pub mirror: Mirror,
    /// 下载并发数 1..=32
    pub concurrency: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            onboarded: false,
            game_dir: None,
            mirror: Mirror::Official,
            concurrency: 12,
        }
    }
}

/// 持久化的任务记录：重启后据此恢复队列（断点续传由引擎按磁盘状态自动推进）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub task_id: String,
    pub version_id: String,
    pub source: VersionSource,
    pub game_dir: String,
    pub mirror: Mirror,
    pub concurrency: u32,
    pub status: TaskStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadsFile {
    pub tasks: Vec<TaskRecord>,
}

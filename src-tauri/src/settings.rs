//! 用户设置与任务记录的持久化模型（app_data_dir 下 settings.json / downloads.json）。

use serde::{Deserialize, Serialize};

use mc_dl_engine::runner::{TaskStatus, VersionSource};
use mc_dl_engine::Mirror;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// 首次启动向导是否完成
    pub onboarded: bool,
    /// 旧版单目录字段：读取时迁移到 game_dirs，不再写出
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_dir: Option<String>,
    /// 多目录：所有已登记的 .minecraft 目录
    pub game_dirs: Vec<String>,
    /// 当前激活目录（新任务写入这里）
    pub active_game_dir: Option<String>,
    pub mirror: Mirror,
    /// 下载并发数 1..=32
    pub concurrency: u32,
    /// Forge processors 使用的 Java 路径（空 = 自动探测）
    pub java_path: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            onboarded: false,
            game_dir: None,
            game_dirs: Vec::new(),
            active_game_dir: None,
            mirror: Mirror::Official,
            concurrency: 12,
            java_path: None,
        }
    }
}

impl AppSettings {
    /// 单目录字段迁移 + 激活目录一致性修复。
    pub fn normalize(&mut self) {
        if self.game_dirs.is_empty() {
            if let Some(dir) = self.game_dir.clone().filter(|d| !d.is_empty()) {
                self.game_dirs.push(dir);
            }
        }
        self.game_dir = None;
        match &self.active_game_dir {
            Some(active) if !active.is_empty() => {
                if !self.game_dirs.contains(active) {
                    self.active_game_dir = self.game_dirs.first().cloned();
                }
            }
            _ => self.active_game_dir = self.game_dirs.first().cloned(),
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

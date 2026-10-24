//! 设置与路径探测相关 Tauri commands。

use tauri::State;

use mc_dl_engine::game_dir::{
    default_game_dir, detect_game_dir as engine_detect, ensure_game_skeleton,
};

use crate::settings::AppSettings;
use crate::state::AppState;
use crate::AppError;

fn canonical_display(p: std::path::PathBuf) -> String {
    std::fs::canonicalize(&p)
        .unwrap_or(p)
        .display()
        .to_string()
}

/// `detect_game_dir(selected_path) -> PathBuf`：
/// 识别用户选中的是 .minecraft 本身还是启动器父目录。
#[tauri::command]
pub fn detect_game_dir(selected_path: String) -> Result<String, AppError> {
    match engine_detect(std::path::Path::new(&selected_path)) {
        Ok(p) => Ok(canonical_display(p)),
        Err(msg) => Err(AppError::new("invalid_game_dir", msg)),
    }
}

/// 探测平台默认路径（%APPDATA%/.minecraft 或 $HOME/.minecraft）是否存在。
#[tauri::command]
pub fn probe_default_dir() -> Result<Option<String>, AppError> {
    Ok(default_game_dir()
        .filter(|p| p.is_dir())
        .map(|p| canonical_display(p)))
}

/// 把用户指定的空文件夹初始化为新的 .minecraft（versions/assets/libraries）。
/// UI 侧需要二次确认后才调用。
#[tauri::command]
pub fn prepare_game_dir(path: String) -> Result<String, AppError> {
    let dir = std::path::PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(AppError::new(
            "invalid_game_dir",
            format!("路径不是文件夹: {path}"),
        ));
    }
    ensure_game_skeleton(&dir)?;
    Ok(canonical_display(dir))
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, AppError> {
    Ok(state.settings.read().unwrap().clone())
}

/// 自动探测系统 Java（仅用于执行 Forge 安装器的 processors）。
#[tauri::command]
pub fn detect_java() -> Result<Option<String>, AppError> {
    Ok(mc_dl_engine::forge::locate_java(None).map(|p| p.display().to_string()))
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, mut settings: AppSettings) -> Result<(), AppError> {
    settings.normalize();
    settings.concurrency = settings.concurrency.clamp(1, 32);
    for dir in settings.game_dirs.iter() {
        if !std::path::Path::new(dir).is_dir() {
            return Err(AppError::new(
                "invalid_game_dir",
                format!(".minecraft 路径不存在: {dir}"),
            ));
        }
    }
    state.save_settings(&settings)
}

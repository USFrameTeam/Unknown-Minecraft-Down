#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Unknown Minecraft Down —— Minecraft Java 版下载器。
//!
//! 职责边界（硬约束）：
//! - 只做：解析版本清单、下载文件、按标准目录放置并校验；
//! - 不做：启动游戏、生成 Java 启动参数、正版验证、登录、模组/整合包管理；
//! - 全程不执行任何下载得到的可执行文件，且无任何进程启动调用。

mod commands;
mod error;
mod persistence;
mod settings;
mod state;

pub use error::AppError;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            state::bootstrap(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::download::list_versions,
            commands::download::list_fabric_loaders,
            commands::download::list_forge_builds,
            commands::download::start_download,
            commands::download::cancel_download,
            commands::download::pause_download,
            commands::download::resume_download,
            commands::download::get_download_state,
            commands::download::list_download_tasks,
            commands::download::clear_finished_tasks,
            commands::download::verify_installation,
            commands::settings::detect_game_dir,
            commands::settings::probe_default_dir,
            commands::settings::prepare_game_dir,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::detect_java,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

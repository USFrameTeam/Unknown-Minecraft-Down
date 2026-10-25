#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Unknown Minecraft Down —— Minecraft Java 版下载器。
//!
//! 职责边界（硬约束）：
//! - 只做：解析版本清单、下载文件、按标准目录放置并校验；
//! - 不做：启动游戏、生成 Java 启动参数、正版验证、登录、模组/整合包管理；
//! - 进程启动调用仅存在于 engine/src/forge.rs（Forge/NeoForge 官方安装器
//!   processors，产品明确授权的唯一例外），本应用层无任何进程启动调用。

mod commands;
mod error;
mod persistence;
mod settings;
mod state;

pub use error::AppError;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            state::bootstrap(app.handle().clone());

            // 窗口默认隐藏（避免启动白屏闪烁）。显示时机双保障：
            // 1) 前端首帧渲染后 show（capabilities 已补 core:window:allow-show）；
            // 2) 5 秒兜底（Rust 侧直接调用，不经过 IPC）：即使前端脚本
            //    完全失败也强制显示窗口。
            if let Some(win) = app.get_webview_window("main") {
                let handle = win.app_handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.show();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::download::list_versions,
            commands::download::list_fabric_loaders,
            commands::download::list_forge_builds,
            commands::download::list_neoforge_versions,
            commands::download::auto_resume_pending,
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

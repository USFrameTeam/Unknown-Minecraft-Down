//! Unknown Minecraft Down 纯下载引擎。
//!
//! 职责边界（硬约束）：
//! - 只做三件事：解析版本清单、下载文件、按标准目录放置并校验；
//! - 绝不启动 Minecraft、绝不生成启动参数、绝不执行任何下载得到的可执行文件；
//! - 所有网络请求走 HTTPS，下载先写 `.part` 临时文件，SHA-1 校验通过后原子改名。

pub mod downloader;
pub mod forge;
pub mod game_dir;
pub mod manifest;
pub mod mirror;
pub mod plan;
pub mod runner;
pub mod verifier;
pub mod version_json;

pub use mirror::Mirror;

pub type Result<T> = std::result::Result<T, EngineError>;

/// 引擎层结构化错误。实现 `Display` 以便前端展示可读信息。
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("网络请求失败: {0}")]
    Network(String),
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("版本不存在或清单中找不到: {0}")]
    VersionNotFound(String),
    #[error("无效或非 HTTPS 的 URL: {0}")]
    InvalidUrl(String),
    #[error("SHA-1 校验失败: {path}（期望 {expected}，实际 {actual}）")]
    Sha1Mismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("拒绝路径穿越: {0}")]
    PathTraversal(String),
    #[error("无效的 .minecraft 目录: {0}")]
    InvalidGameDir(String),
    #[error("任务已取消")]
    Canceled,
    #[error("{0}")]
    Custom(String),
}

impl From<reqwest::Error> for EngineError {
    fn from(e: reqwest::Error) -> Self {
        EngineError::Network(e.to_string())
    }
}

//! IPC 错误：结构化、可序列化，前端能拿到可读信息。

use serde::Serialize;

use mc_dl_engine::EngineError;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<EngineError> for AppError {
    fn from(e: EngineError) -> Self {
        let code = match &e {
            EngineError::Network(_) => "network",
            EngineError::Io(_) => "io",
            EngineError::Json(_) => "json",
            EngineError::VersionNotFound(_) => "version_not_found",
            EngineError::InvalidUrl(_) => "invalid_url",
            EngineError::Sha1Mismatch { .. } => "sha1_mismatch",
            EngineError::PathTraversal(_) => "path_traversal",
            EngineError::InvalidGameDir(_) => "invalid_game_dir",
            EngineError::Canceled => "canceled",
            EngineError::Custom(_) => "internal",
        };
        AppError::new(code, e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::new("io", e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::new("network", e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::new("json", e.to_string())
    }
}

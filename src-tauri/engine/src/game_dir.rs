//! `.minecraft` 目录探测与路径安全。
//!
//! - `detect_game_dir`：用户选中的可能是 `.minecraft` 本身，也可能是启动器（PCL/HMCL
//!   等）所在父目录，自动向下探测；
//! - `safe_join` / `ensure_within`：下载与 natives 解压写文件的路径防穿越，
//!   拒绝 `..`、绝对路径、反斜杠，且用 canonicalize 兜底防止符号链接逃逸。

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::{EngineError, Result};

/// 判断目录是否“像”一个 .minecraft（存在 versions/、assets/、libraries/ 之一）。
pub fn looks_like_game_dir(p: &Path) -> bool {
    p.join("versions").is_dir() || p.join("assets").is_dir() || p.join("libraries").is_dir()
}

/// 平台默认的 .minecraft 路径：%APPDATA%/.minecraft 或 $HOME/.minecraft。
pub fn default_game_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(".minecraft"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".minecraft"))
    }
}

/// 智能识别 .minecraft 的准确位置：
/// 1. 选中目录本身即 .minecraft（有 versions/ 等结构）→ 直接使用；
/// 2. 选中目录下有 `.minecraft` 子目录 → 使用它；
/// 3. 扫描一层子目录：任一子目录本身像 .minecraft，或其内部的 `.minecraft` 像；
///    恰好一个候选 → 使用；多个候选 → 优先名为 `.minecraft` 的，否则报歧义错误。
pub fn detect_game_dir(selected: &Path) -> std::result::Result<PathBuf, String> {
    if !selected.is_dir() {
        return Err(format!("所选路径不是文件夹: {}", selected.display()));
    }
    if looks_like_game_dir(selected) {
        return Ok(selected.to_path_buf());
    }
    let dot = selected.join(".minecraft");
    if dot.is_dir() && looks_like_game_dir(&dot) {
        return Ok(dot);
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(selected)
        .map_err(|e| format!("无法读取目录 {}: {e}", selected.display()))?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        if looks_like_game_dir(&p) {
            candidates.push(p);
            continue;
        }
        let inner = p.join(".minecraft");
        if inner.is_dir() && looks_like_game_dir(&inner) {
            candidates.push(inner);
        }
    }
    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }
    if candidates.is_empty() {
        Err(
            "未能在所选目录中找到 .minecraft（需包含 versions/、assets/ 或 libraries/ 的目录）。\
             也可以选择一个空文件夹，应用会将其初始化为新的 .minecraft。"
                .into(),
        )
    } else {
        Err(format!(
            "找到多个候选目录，请直接选择其中一个: {}",
            candidates
                .iter()
                .map(|c| c.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// 在选定位置创建 .minecraft 骨架目录（versions/assets/libraries）。
pub fn ensure_game_skeleton(dir: &Path) -> Result<()> {
    for sub in ["versions", "assets", "libraries"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    Ok(())
}

/// 拼接 game_dir 与相对路径，拒绝任何形式的穿越（`..`、绝对路径、反斜杠）。
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    if rel.is_empty() || rel.contains('\\') {
        return Err(EngineError::PathTraversal(rel.to_string()));
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(EngineError::PathTraversal(rel.to_string()));
    }
    for comp in rel_path.components() {
        match comp {
            Component::Normal(_) => {}
            _ => return Err(EngineError::PathTraversal(rel.to_string())),
        }
    }
    Ok(root.join(rel_path))
}

/// 基于 canonicalize 的越界兜底：目标（或其最深已存在的祖先）解析后必须仍在
/// root 之内，可拦截“已存在的符号链接指向 root 之外”的情况。
/// 已解析的祖先带缓存，避免对上千个资产文件做重复 syscall。
#[derive(Default)]
pub struct PathGuard {
    cache: std::sync::Mutex<HashMap<PathBuf, PathBuf>>,
}

impl PathGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ensure_within(&self, root: &Path, target: &Path) -> Result<()> {
        let root_canon = self.canonical_ancestor(root)?;
        let (anc_canon, tail) = self.canonical_ancestor_with_tail(target)?;
        let full = anc_canon.join(tail);
        if full.starts_with(&root_canon) {
            Ok(())
        } else {
            Err(EngineError::PathTraversal(target.display().to_string()))
        }
    }

    fn canonical_ancestor(&self, p: &Path) -> Result<PathBuf> {
        let (c, _) = self.canonical_ancestor_with_tail(p)?;
        Ok(c)
    }

    /// 返回 (最深已存在祖先的 canonical 路径, 其下未存在的相对尾部)。
    fn canonical_ancestor_with_tail(&self, target: &Path) -> Result<(PathBuf, PathBuf)> {
        let mut tail_parts: Vec<std::ffi::OsString> = Vec::new();
        let mut probe = target.to_path_buf();
        loop {
            if let Some(cached) = self.cached(&probe) {
                return Ok(self.split_tail(cached, &tail_parts));
            }
            if let Ok(canon) = std::fs::canonicalize(&probe) {
                self.store(probe.clone(), canon.clone());
                return Ok(self.split_tail(canon, &tail_parts));
            }
            match (probe.file_name(), probe.parent()) {
                (Some(name), Some(parent)) => {
                    tail_parts.push(name.to_os_string());
                    probe = parent.to_path_buf();
                }
                _ => return Err(EngineError::PathTraversal(target.display().to_string())),
            }
        }
    }

    fn split_tail(
        &self,
        anc_canon: PathBuf,
        tail_parts: &[std::ffi::OsString],
    ) -> (PathBuf, PathBuf) {
        let mut full = anc_canon.clone();
        for part in tail_parts.iter().rev() {
            full.push(part);
        }
        let tail = full
            .strip_prefix(&anc_canon)
            .unwrap_or(Path::new(""))
            .to_path_buf();
        (anc_canon, tail)
    }

    fn cached(&self, key: &Path) -> Option<PathBuf> {
        self.cache.lock().ok()?.get(key).cloned()
    }

    fn store(&self, key: PathBuf, value: PathBuf) {
        if let Ok(mut c) = self.cache.lock() {
            c.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_game(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join("versions")).unwrap();
        dir.to_path_buf()
    }

    #[test]
    fn detect_direct_game_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let g = make_game(tmp.path());
        assert_eq!(detect_game_dir(&g).unwrap(), g);
    }

    #[test]
    fn detect_parent_containing_dot_minecraft() {
        let tmp = tempfile::tempdir().unwrap();
        let g = make_game(&tmp.path().join(".minecraft"));
        assert_eq!(detect_game_dir(tmp.path()).unwrap(), g);
    }

    #[test]
    fn detect_sibling_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let g = make_game(&tmp.path().join("some-launcher").join(".minecraft"));
        assert_eq!(detect_game_dir(tmp.path()).unwrap(), g);
    }

    #[test]
    fn detect_ambiguous_reports_error() {
        let tmp = tempfile::tempdir().unwrap();
        let a = make_game(&tmp.path().join("aaa"));
        let b = make_game(&tmp.path().join("bbb"));
        let err = detect_game_dir(tmp.path()).unwrap_err();
        assert!(
            err.contains(a.to_string_lossy().as_ref())
                && err.contains(b.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn detect_none_reports_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(detect_game_dir(tmp.path()).is_err());
        assert!(detect_game_dir(&tmp.path().join("missing")).is_err());
    }

    #[test]
    fn safe_join_rejects_traversal() {
        let root = Path::new("/tmp/mc");
        assert!(safe_join(root, "versions/a/b.json").is_ok());
        assert!(safe_join(root, "../escape").is_err());
        assert!(safe_join(root, "a/../../escape").is_err());
        assert!(safe_join(root, "/etc/passwd").is_err());
        assert!(safe_join(root, "a\\..\\..\\escape").is_err());
        assert!(safe_join(root, "").is_err());
    }

    #[test]
    fn ensure_within_blocks_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let assets = tmp.path().join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), assets.join("objects")).unwrap();
        let guard = PathGuard::new();
        let target = assets.join("objects").join("ab").join("cdef");
        assert!(guard.ensure_within(tmp.path(), &target).is_err());
        // 正常路径应通过
        assert!(guard
            .ensure_within(tmp.path(), &tmp.path().join("versions/1.20.1/1.20.1.jar"))
            .is_ok());
    }
}

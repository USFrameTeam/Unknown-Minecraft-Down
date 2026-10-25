//! Forge 安装支持。
//!
//! 实现方式与官方安装器一致：
//! - 下载官方安装器 jar（BMCLAPI 提供哈希，可校验）；
//! - 解析 install_profile.json（spec 0/1）与内嵌 version.json；
//! - 下载 profile.libraries 到 libraries/；
//! - 提取安装器内嵌的 maven/* 产物到 libraries/；
//! - 现代 Forge（1.13+，含 processors）需要以系统 Java 执行安装器的处理管线
//!   （MCP_DATA / DOWNLOAD_MOJMAPS / MERGE_MAPPING / jarsplitter / FART /
//!   binarypatcher），并以 outputs 的 SHA-1 校验产物；
//!   —— 该能力已由产品需求明确授权，仅限 Forge 官方安装器的 processor 执行。
//! - 旧版 Forge（≤1.12.2）无 processors，纯下载即可。
//!
//! 注意：DOWNLOAD_MOJMAPS 等 processor 会直接访问 Mojang 官方端点
//! （piston-meta / piston-data），Forge 安装需要这两个域名可达。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::downloader::{download_file, hash_file};
use crate::mirror::Mirror;
use crate::version_json::ChainEntry;
use crate::{EngineError, Result};

/// BMCLAPI 的 Forge 构建列表（同时提供安装器 SHA-1，无论下载镜像偏好如何，
/// 列表与哈希始终取自 BMCLAPI；文件本体按用户镜像偏好下载）。
pub const FORGE_LIST_API: &str = "https://bmclapi2.bangbang93.com/forge/minecraft";

#[derive(Debug, Clone, Deserialize)]
pub struct ForgeBuild {
    #[serde(default)]
    pub branch: Option<String>,
    pub build: i64,
    pub mcversion: String,
    pub version: String,
    pub modified: String,
    #[serde(default)]
    files: Vec<ForgeFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct ForgeFile {
    format: String,
    category: String,
    hash: String,
}

impl ForgeBuild {
    /// 完整构件 id，如 `1.20.1-47.3.0`（老版本可能带 branch 后缀）。
    pub fn artifact_id(&self) -> String {
        match &self.branch {
            Some(b) if !b.is_empty() => format!("{}-{}-{}", self.mcversion, self.version, b),
            _ => format!("{}-{}", self.mcversion, self.version),
        }
    }

    /// 版本目录 id，与官方安装器一致：`1.20.1-forge-47.3.0`。
    pub fn version_id(&self) -> String {
        match &self.branch {
            Some(b) if !b.is_empty() => format!("{}-forge-{}-{}", self.mcversion, self.version, b),
            _ => format!("{}-forge-{}", self.mcversion, self.version),
        }
    }

    pub fn installer_sha1(&self) -> Option<&str> {
        self.files
            .iter()
            .find(|f| f.category == "installer" && f.format == "jar")
            .map(|f| f.hash.as_str())
    }
}

pub async fn fetch_forge_builds(
    client: &reqwest::Client,
    mc_version: &str,
) -> Result<Vec<ForgeBuild>> {
    let url = format!("{FORGE_LIST_API}/{mc_version}");
    let resp = client.get(&url).send().await?.error_for_status()?;
    let mut builds = resp.json::<Vec<ForgeBuild>>().await?;
    builds.sort_by_key(|b| std::cmp::Reverse(b.build));
    Ok(builds)
}

/// Forge maven 构件 URL（installer / universal 等）。
pub fn forge_maven_url(mirror: Mirror, artifact_id: &str, file_suffix: &str) -> String {
    mirror.map_url(&format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{artifact_id}/forge-{artifact_id}-{file_suffix}.jar"
    ))
    .expect("常量 URL 必定合法")
}

// ---------------------------------------------------------------------------
// install_profile.json 解析
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct InstallProfile {
    #[serde(default)]
    pub spec: i64,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub minecraft: String,
    /// 内嵌 version.json 在安装器内的路径，如 "/version.json"
    #[serde(default)]
    pub json: Option<String>,
    #[serde(default)]
    pub data: HashMap<String, SideValue>,
    /// 复用引擎 Library 结构：现代 profile 带 downloads.artifact（url+sha1），
    /// 旧式 profile 用 url/checksums。
    #[serde(default)]
    pub libraries: Vec<crate::version_json::Library>,
    #[serde(default)]
    pub processors: Vec<Processor>,
}

impl InstallProfile {
    pub fn has_client_processors(&self) -> bool {
        self.processors.iter().any(|p| p.runs_on_client())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SideValue {
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub server: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Processor {
    #[serde(default)]
    pub sides: Option<Vec<String>>,
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub outputs: Option<HashMap<String, String>>,
}

impl Processor {
    pub fn runs_on_client(&self) -> bool {
        self.sides
            .as_ref()
            .map(|s| s.iter().any(|x| x == "client"))
            .unwrap_or(true)
    }
}

pub fn parse_profile(text: &str) -> Result<InstallProfile> {
    Ok(serde_json::from_str(text)?)
}

/// processor bracket 引用 → maven 仓库相对路径。支持的形态（源自真实
/// install_profile 的 data 表与 args）：
/// - `[g:a:v]`                          → `g/a/v/a-v.jar`
/// - `[g:a:v@ext]`                      → `g/a/v/a-v.ext`（如 mcp_config 的 `…@zip`）
/// - `[g:a:v:classifier]`               → `g/a/v/a-v-classifier.jar`
/// - `[g:a:v:classifier@ext]`           → `g/a/v/a-v-classifier.ext`（如 `mappings@txt`）
pub fn maven_artifact_path(coord: &str) -> String {
    let segs: Vec<&str> = coord.split(':').collect();
    fn split_ver_ext(v: &str) -> (&str, &str) {
        match v.rsplit_once('@') {
            Some((vv, ext)) => (vv, ext),
            None => (v, "jar"),
        }
    }
    match segs.as_slice() {
        [g, a, v] => {
            let (v, ext) = split_ver_ext(v);
            format!("{}/{}/{}/{}-{}.{}", g.replace('.', "/"), a, v, a, v, ext)
        }
        [g, a, v, last] => {
            // 第 4 段：classifier（默认 jar）或 classifier@ext
            let (file, ext) = match last.split_once('@') {
                Some((c, ext)) => (format!("{a}-{v}-{c}"), ext),
                None => (format!("{a}-{v}-{last}"), "jar"),
            };
            format!("{}/{}/{}/{}.{}", g.replace('.', "/"), a, v, file, ext)
        }
        _ => coord.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 安装器下载与读取
// ---------------------------------------------------------------------------

/// 下载安装器到缓存目录（带 SHA-1 校验），返回本地路径。URL 由调用方构造
/// （Forge / NeoForge 各自的 maven 路径）。
pub async fn fetch_installer_at(
    client: &reqwest::Client,
    url: &str,
    file_name: &str,
    expected_sha1: Option<&str>,
    cache_dir: &Path,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    tokio::fs::create_dir_all(cache_dir).await?;
    let dest = cache_dir.join(file_name);
    download_file(client, url, &dest, expected_sha1, None, cancel, &|_| {}).await?;
    Ok(dest)
}

/// 下载 Forge 安装器（带 BMCLAPI 提供的 SHA-1 校验），返回本地路径。
pub async fn fetch_installer(
    client: &reqwest::Client,
    mirror: Mirror,
    artifact_id: &str,
    expected_sha1: Option<&str>,
    cache_dir: &Path,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    let url = forge_maven_url(mirror, artifact_id, "installer");
    fetch_installer_at(
        client,
        &url,
        &format!("forge-{artifact_id}-installer.jar"),
        expected_sha1,
        cache_dir,
        cancel,
    )
    .await
}

pub fn read_zip_entry(zip_path: &Path, entry: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| EngineError::Custom(format!("打开安装器失败: {e}")))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| EngineError::Custom(format!("读取安装器 zip 结构失败: {e}")))?;
    let mut e = zip
        .by_name(entry)
        .map_err(|_| EngineError::Custom(format!("安装器内缺少条目 {entry}")))?;
    let mut buf = Vec::with_capacity(e.size() as usize);
    e.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn zip_entry_exists(zip_path: &Path, entry: &str) -> bool {
    let Ok(file) = std::fs::File::open(zip_path) else {
        return false;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return false;
    };
    let Ok(_entry) = zip.by_name(entry) else {
        return false;
    };
    true
}

/// 读取安装器内嵌 version.json（install_profile.json 的 `json` 字段指向）。
pub fn read_embedded_version_json(installer: &Path, profile: &InstallProfile) -> Result<String> {
    let entry = profile
        .json
        .clone()
        .unwrap_or_else(|| "/version.json".into());
    let entry = entry.trim_start_matches('/');
    let bytes = read_zip_entry(installer, entry)?;
    String::from_utf8(bytes)
        .map_err(|_| EngineError::Custom("内嵌 version.json 不是有效 UTF-8".into()))
}

/// 提取安装器内嵌的 `maven/<path>` 产物到 libraries/，返回提取数量。
/// 仅提取 profile.libraries 中声明的坐标（与官方安装器行为一致），避免
/// 把安装器里无关的依赖写进游戏 libraries。
pub fn extract_embedded_maven(
    installer: &Path,
    profile: &InstallProfile,
    libraries_dir: &Path,
) -> Result<u32> {
    let file = std::fs::File::open(installer)
        .map_err(|e| EngineError::Custom(format!("打开安装器失败: {e}")))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| EngineError::Custom(format!("读取安装器 zip 结构失败: {e}")))?;
    let mut count = 0u32;
    for lib in &profile.libraries {
        let Some(name) = &lib.name else { continue };
        let rel = maven_artifact_path(name);
        let entry = format!("maven/{rel}");
        let Ok(mut e) = zip.by_name(&entry) else {
            continue;
        };
        if e.is_dir() {
            continue;
        }
        let dest = libraries_dir.join(&rel);
        let expected = lib
            .downloads
            .as_ref()
            .and_then(|d| d.artifact.as_ref().and_then(|a| a.sha1.clone()))
            .or_else(|| lib.checksums.as_ref().and_then(|c| c.first().cloned()));
        if let Some(expect) = &expected {
            if dest.exists() && hash_file_sync(&dest) == expect.to_lowercase() {
                continue; // 已存在且一致
            }
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut e, &mut out)?;
        count += 1;
    }
    Ok(count)
}

fn hash_file_sync(path: &Path) -> String {
    use sha1::{Digest, Sha1};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        match std::io::Read::read(&mut f, &mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return String::new(),
        }
    }
    hex::encode(hasher.finalize())
}

/// data 值中的 "/data/xxx" 形式：从安装器提取到工作目录，返回真实文件路径。
pub fn extract_installer_data_file(
    installer: &Path,
    entry: &str,
    work_dir: &Path,
) -> Result<PathBuf> {
    let rel = entry.trim_start_matches('/');
    let sanitized = rel.replace('/', "_");
    let dest = work_dir.join(sanitized);
    if !dest.exists() {
        let bytes = read_zip_entry(installer, rel)?;
        std::fs::create_dir_all(work_dir)?;
        std::fs::write(&dest, bytes)?;
    }
    Ok(dest)
}

// ---------------------------------------------------------------------------
// processor 参数解析
// ---------------------------------------------------------------------------

pub struct ProcessorCtx<'a> {
    pub game_dir: &'a Path,
    pub installer: &'a Path,
    pub work_dir: &'a Path,
    pub data: &'a HashMap<String, SideValue>,
    pub mc_version: &'a str,
}

/// 解析单个 processor 参数 / outputs 键值：
/// - `{NAME}`：内置符号（MINECRAFT/ROOT/SIDE/INSTALLER/LIBRARY_DIR/MINECRAFT_VERSION）
///   或 data 表引用（值可能是 'literal'、[maven 坐标] 或 /安装器内路径）
/// - `[g:a:v:...]`：libraries 目录下的 maven 产物路径
/// - `'literal'`：去掉引号的字面量
/// - 其他：原样返回
pub fn resolve_token(token: &str, ctx: &ProcessorCtx<'_>) -> Result<String> {
    let libraries = ctx.game_dir.join("libraries");
    if let Some(name) = token.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
        let value = match name {
            "MINECRAFT" | "ROOT" => return Ok(ctx.game_dir.display().to_string()),
            "LIBRARY_DIR" => return Ok(libraries.display().to_string()),
            "SIDE" => return Ok("client".into()),
            "INSTALLER" => return Ok(ctx.installer.display().to_string()),
            "MINECRAFT_VERSION" => return Ok(ctx.mc_version.to_string()),
            // 原版客户端 jar（processor 的补丁输入）
            "MINECRAFT_JAR" => {
                return Ok(ctx
                    .game_dir
                    .join(format!(
                        "versions/{}/{}.jar",
                        ctx.mc_version, ctx.mc_version
                    ))
                    .display()
                    .to_string())
            }
            _ => ctx
                .data
                .get(name)
                .map(|v| v.client.clone())
                .ok_or_else(|| EngineError::Custom(format!("install_profile 缺少 data[{name}]")))?,
        };
        return resolve_value(&value, ctx);
    }
    resolve_value(token, ctx)
}

/// 解析 data 值（或直接的 [..]/'..' 形式）。
pub fn resolve_value(value: &str, ctx: &ProcessorCtx<'_>) -> Result<String> {
    if let Some(coord) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        let libraries = ctx.game_dir.join("libraries");
        return Ok(libraries
            .join(maven_artifact_path(coord))
            .display()
            .to_string());
    }
    if let Some(lit) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return Ok(lit.to_string());
    }
    if value.starts_with('/') {
        // 安装器内嵌文件（如 /data/client.lzma）→ 提取为真实路径
        let p = extract_installer_data_file(ctx.installer, value, ctx.work_dir)?;
        return Ok(p.display().to_string());
    }
    Ok(value.to_string())
}

// ---------------------------------------------------------------------------
// NeoForge（与 Forge 同构的安装器管线，差异仅在 maven 坐标与版本列表来源）
// ---------------------------------------------------------------------------

/// 版本目录 id，与官方安装器一致：`neoforge-20.4.237`。
pub fn neoforge_version_id(nf_version: &str) -> String {
    format!("neoforge-{nf_version}")
}

/// MC 版本 → NeoForge 版本前缀："1.20.4"→"20.4."、"1.21"→"21.0."。
fn neoforge_prefix_for_mc(mc_version: &str) -> Option<String> {
    let rest = mc_version.strip_prefix('1')?.strip_prefix('.')?;
    let segs: Vec<&str> = rest.split('.').collect();
    match segs.as_slice() {
        [a, b] => Some(format!("{a}.{b}.")),
        [a] => Some(format!("{a}.0.")),
        _ => None,
    }
}

/// 按点分段数值比较（"20.4.80" < "20.4.237" < "26.3.0.97"）。
fn version_desc_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect()
    };
    parse(a).cmp(&parse(b)).reverse()
}

/// 解析 maven-metadata.xml：返回该 MC 版本可用的稳定 NeoForge 版本（降序），
/// 过滤 alpha/beta/snapshot 等带后缀版本与非数字段。
pub fn parse_neoforge_metadata(xml: &str, mc_version: &str) -> Vec<String> {
    let Some(prefix) = neoforge_prefix_for_mc(mc_version) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for cap in xml.split("<version>").skip(1) {
        let Some(v) = cap.split("</version>").next() else {
            continue;
        };
        let v = v.trim();
        if !v.starts_with(&prefix) || v.contains('-') || v.contains('+') {
            continue;
        }
        if !v
            .split('.')
            .all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        out.push(v.to_string());
    }
    out.sort_by(|a, b| version_desc_cmp(a, b));
    out.dedup();
    out
}

/// 获取某 MC 版本的稳定 NeoForge 版本列表（降序）。
pub async fn fetch_neoforge_versions(
    client: &reqwest::Client,
    mirror: Mirror,
    mc_version: &str,
) -> Result<Vec<String>> {
    let url = mirror.map_url(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml",
    )?;
    let xml = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_neoforge_metadata(&xml, mc_version))
}

/// NeoForge 安装器 URL。
pub fn neoforge_installer_url(mirror: Mirror, nf_version: &str) -> String {
    mirror
        .map_url(&format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{nf_version}/neoforge-{nf_version}-installer.jar"
        ))
        .expect("常量 URL 必定合法")
}

/// 读取 NeoForge 版本入口（安装器 → profile + 内嵌 version.json）。
/// 安装器哈希取 maven 旁挂的 `.sha1`；获取失败则跳过校验
/// （产物最终仍以 PATCHED_SHA / outputs 校验兜底）。
pub async fn fetch_neoforge_version_entry(
    client: &reqwest::Client,
    mirror: Mirror,
    nf_version: &str,
    _game_dir: &Path,
    cancel: &CancellationToken,
) -> Result<(ChainEntry, InstallProfile, PathBuf)> {
    let cache = std::env::temp_dir()
        .join("mc-downloader-neoforge")
        .join(sanitize_component(nf_version));
    let url = neoforge_installer_url(mirror, nf_version);
    let expected = match client.get(format!("{url}.sha1")).send().await {
        Ok(resp) if resp.status().is_success() => resp.text().await.ok().and_then(|t| {
            let t = t.split_whitespace().next().unwrap_or("").to_string();
            (!t.is_empty()).then_some(t)
        }),
        _ => None,
    };
    let installer = fetch_installer_at(
        client,
        &url,
        &format!("neoforge-{nf_version}-installer.jar"),
        expected.as_deref(),
        &cache,
        cancel,
    )
    .await?;
    let profile_text = String::from_utf8(read_zip_entry(&installer, "install_profile.json")?)
        .map_err(|_| EngineError::Custom("install_profile.json 不是有效 UTF-8".into()))?;
    let profile = parse_profile(&profile_text)?;
    let vjson = read_embedded_version_json(&installer, &profile)?;
    let json: crate::version_json::VersionJson = serde_json::from_str(&vjson)?;
    Ok((
        ChainEntry {
            json,
            raw: Some(vjson),
            source_url: None,
            manifest_sha1: None,
        },
        profile,
        installer,
    ))
}

// ---------------------------------------------------------------------------
// Java 定位与 processor 执行
// ---------------------------------------------------------------------------

/// 定位可用 Java：显式路径 > JAVA_HOME > PATH > 常见安装目录。
/// 仅用于执行 Forge 安装器的 processors，绝不用于启动游戏。
pub fn locate_java(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|s| !s.is_empty()) {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin").join(exe);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let candidates: Vec<PathBuf> = if cfg!(windows) {
        [
            "C:/Program Files/Java",
            "C:/Program Files/Eclipse Adoptium",
            "C:/Program Files/Microsoft",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Library/Java/JavaVirtualMachines")]
    } else {
        vec![PathBuf::from("/usr/lib/jvm")]
    };
    for base in candidates {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let p = entry.path().join("bin").join(exe);
                if p.is_file() {
                    return Some(p);
                }
                // macOS 布局：<home>/Contents/Home/bin/java
                let p2 = entry.path().join("Contents/Home/bin").join(exe);
                if p2.is_file() {
                    return Some(p2);
                }
            }
        }
    }
    None
}

/// 校验 Java 可用（执行 `java -version`）。
pub async fn java_works(java: &Path) -> bool {
    tokio::process::Command::new(java)
        .arg("-version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn main_class_of_jar(jar: &Path) -> Result<String> {
    let bytes = read_zip_entry(jar, "META-INF/MANIFEST.MF")?;
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Main-Class:") {
            return Ok(v.trim().to_string());
        }
    }
    Err(EngineError::Custom(format!(
        "processor jar 缺少 Main-Class（{}）",
        jar.display()
    )))
}

/// 执行单个 processor 并校验 outputs。这是 Forge 安装中唯一执行外部代码的位置，
/// 仅运行官方安装器自带/声明的 processor jar。
pub async fn run_processor(
    java: &Path,
    profile: &Processor,
    ctx: &ProcessorCtx<'_>,
    cancel: &CancellationToken,
) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(EngineError::Canceled);
    }
    let libraries = ctx.game_dir.join("libraries");
    let jar_path = libraries.join(maven_artifact_path(&profile.jar));
    let main_class = main_class_of_jar(&jar_path)?;

    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-cp");
    let mut cp = vec![jar_path.clone()];
    for c in &profile.classpath {
        cp.push(libraries.join(maven_artifact_path(c)));
    }
    let cp_str = cp
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(if cfg!(windows) { ";" } else { ":" });
    cmd.arg(cp_str).arg(&main_class);
    for a in &profile.args {
        cmd.arg(resolve_token(a, ctx)?);
    }
    // 官方安装器的 processor 需要较长时间（映射合并/二进制补丁），不设硬超时，
    // 由取消令牌控制中止（kill_on_drop 保证取消时进程被杀死）。
    let child = cmd
        .current_dir(ctx.game_dir)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| EngineError::Custom(format!("启动 Java 失败（{}）: {e}", java.display())))?;
    let out = tokio::select! {
        _ = cancel.cancelled() => return Err(EngineError::Canceled),
        o = child.wait_with_output() => o.map_err(|e| EngineError::Custom(format!("执行 processor 失败: {e}")))?,
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(EngineError::Custom(format!(
            "processor 失败（exit {}）：{}{}",
            out.status,
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!("\nstdout: {}", stdout.trim())
            }
        )));
    }

    // outputs 校验：key=文件路径模板，value=SHA-1（可能是 {TOKEN} 引用）
    if let Some(outputs) = &profile.outputs {
        for (path_tpl, sha_tpl) in outputs {
            let path = resolve_token(path_tpl, ctx)?;
            let expect = resolve_token(sha_tpl, ctx)?;
            let p = Path::new(&path);
            if !p.exists() {
                return Err(EngineError::Custom(format!("processor 未产出文件：{path}")));
            }
            let actual = hash_file(p).await?;
            if !expect.is_empty() && actual != expect.to_lowercase() {
                return Err(EngineError::Sha1Mismatch {
                    path,
                    expected: expect,
                    actual,
                });
            }
        }
    }
    Ok(())
}

/// 按顺序执行全部 client 侧 processors。
pub async fn run_client_processors(
    java: &Path,
    profile: &InstallProfile,
    ctx: &ProcessorCtx<'_>,
    cancel: &CancellationToken,
    on_step: &(dyn Fn(&str) + Send + Sync),
) -> Result<()> {
    for p in &profile.processors {
        if !p.runs_on_client() {
            continue;
        }
        let label = p
            .args
            .iter()
            .position(|a| a == "--task")
            .and_then(|i| p.args.get(i + 1))
            .cloned()
            .unwrap_or_else(|| p.jar.clone());
        on_step(&label);
        run_processor(java, p, ctx, cancel).await?;
    }
    Ok(())
}

/// 官方安装器要求目标目录存在 launcher_profiles.json（启动器标记文件）。
pub async fn ensure_launcher_profiles_stub(game_dir: &Path) -> Result<()> {
    let p = game_dir.join("launcher_profiles.json");
    if !p.exists() {
        tokio::fs::write(&p, br#"{"profiles":{},"settings":{},"version":3}"#).await?;
    }
    Ok(())
}

/// 安装器缓存目录（系统临时目录下，按构件 id 命名，支持断点续传）。
pub fn installer_cache_dir(artifact_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("mc-downloader-forge")
        .join(sanitize_component(artifact_id))
}

fn sanitize_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 读取 forge 版本入口（下载安装器 → 解析 profile → 内嵌 version.json）。
/// 返回 (ChainEntry, InstallProfile, 安装器本地路径)。
pub async fn fetch_forge_version_entry(
    client: &reqwest::Client,
    mirror: Mirror,
    artifact_id: &str,
    installer_sha1: Option<&str>,
    _game_dir: &Path,
    cancel: &CancellationToken,
) -> Result<(ChainEntry, InstallProfile, PathBuf)> {
    let cache = installer_cache_dir(artifact_id);
    let installer =
        fetch_installer(client, mirror, artifact_id, installer_sha1, &cache, cancel).await?;
    let profile_text = String::from_utf8(read_zip_entry(&installer, "install_profile.json")?)
        .map_err(|_| EngineError::Custom("install_profile.json 不是有效 UTF-8".into()))?;
    let profile = parse_profile(&profile_text)?;
    let vjson = read_embedded_version_json(&installer, &profile)?;
    let json: crate::version_json::VersionJson = serde_json::from_str(&vjson)?;
    Ok((
        ChainEntry {
            json,
            raw: Some(vjson),
            source_url: None,
            manifest_sha1: None,
        },
        profile,
        installer,
    ))
}

/// 供下载阶段使用：profile.libraries → FileJob 列表（processor 依赖库等）。
/// 旧式无 url 的库默认走 `default_base`（Forge / NeoForge 各自的官方 maven）。
pub fn profile_library_jobs(
    profile: &InstallProfile,
    mirror: Mirror,
    default_base: &str,
) -> Result<Vec<crate::plan::FileJob>> {
    use crate::plan::FileKind;
    use crate::version_json::{library_artifact_with_base, OsContext};

    let ctx = OsContext::detect();
    let mut jobs = Vec::new();
    for lib in &profile.libraries {
        let Some(art) = library_artifact_with_base(lib, &ctx, None, default_base)? else {
            continue;
        };
        let Some(rel) = &art.path else { continue };
        jobs.push(crate::plan::FileJob {
            url: mirror.map_url(&art.url)?,
            dest: format!("libraries/{rel}"),
            sha1: art.sha1.clone(),
            size: art.size,
            kind: FileKind::Library,
        });
    }
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_dir::safe_join;

    fn ctx<'a>(
        game_dir: &'a Path,
        installer: &'a Path,
        work_dir: &'a Path,
        data: &'a HashMap<String, SideValue>,
        mc: &'a str,
    ) -> ProcessorCtx<'a> {
        ProcessorCtx {
            game_dir,
            installer,
            work_dir,
            data,
            mc_version: mc,
        }
    }

    #[test]
    fn maven_artifact_path_variants() {
        assert_eq!(
            maven_artifact_path("net.minecraftforge:installertools:1.4.1"),
            "net/minecraftforge/installertools/1.4.1/installertools-1.4.1.jar"
        );
        assert_eq!(
            maven_artifact_path("net.minecraftforge:ForgeAutoRenamingTool:0.1.22:all"),
            "net/minecraftforge/ForgeAutoRenamingTool/0.1.22/ForgeAutoRenamingTool-0.1.22-all.jar"
        );
        assert_eq!(
            maven_artifact_path("de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412@zip"),
            "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip"
        );
        assert_eq!(
            maven_artifact_path("de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412:mappings@txt"),
            "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412-mappings.txt"
        );
        assert_eq!(
            maven_artifact_path("net.minecraftforge:forge:1.20.1-47.3.0:client"),
            "net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-client.jar"
        );
    }

    #[test]
    fn resolve_token_builtins_and_data() {
        let dir = tempfile::tempdir().unwrap();
        let installer = dir.path().join("installer.jar");
        let work = dir.path().join("work");
        let mut data = HashMap::new();
        data.insert(
            "MCP_VERSION".to_string(),
            SideValue {
                client: "'20230612.114412'".into(),
                server: String::new(),
            },
        );
        data.insert(
            "PATCHED".to_string(),
            SideValue {
                client: "[net.minecraftforge:forge:1.20.1-47.3.0:client]".into(),
                server: String::new(),
            },
        );
        let c = ctx(dir.path(), &installer, &work, &data, "1.20.1");
        assert_eq!(resolve_token("{SIDE}", &c).unwrap(), "client");
        assert_eq!(resolve_token("{MINECRAFT_VERSION}", &c).unwrap(), "1.20.1");
        assert_eq!(
            resolve_token("{LIBRARY_DIR}", &c).unwrap(),
            dir.path().join("libraries").display().to_string()
        );
        assert_eq!(
            resolve_token("{MCP_VERSION}", &c).unwrap(),
            "20230612.114412"
        );
        assert_eq!(
            resolve_token("{PATCHED}", &c).unwrap(),
            dir.path()
                .join("libraries/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-client.jar")
                .display()
                .to_string()
        );
        assert_eq!(resolve_token("--task", &c).unwrap(), "--task");
        assert_eq!(resolve_token("'literal text'", &c).unwrap(), "literal text");
        // 未知 token → 报错
        assert!(resolve_token("{UNKNOWN}", &c).is_err());
    }

    #[test]
    fn parse_real_profile_shape() {
        // 与真实 1.20.1-47.3.0 install_profile.json 同构的精简样例
        let text = r#"{
            "spec": 1,
            "profile": "forge",
            "version": "1.20.1-forge-47.3.0",
            "minecraft": "1.20.1",
            "json": "/version.json",
            "data": {
                "BINPATCH": {"client": "/data/client.lzma", "server": "/data/server.lzma"},
                "MCP_VERSION": {"client": "'20230612.114412'", "server": "'20230612.114412'"}
            },
            "libraries": [
                {"name": "com.github.jponge:lzma-java:1.3"},
                {"name": "de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412@zip", "downloads": {"artifact": {
                    "path": "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip",
                    "url": "https://maven.minecraftforge.net/de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip",
                    "sha1": "c7d29380ddb38becad7c0819b5b325e43bca23f0", "size": 1740711
                }}}
            ],
            "processors": [
                {
                    "sides": ["client", "server"],
                    "jar": "net.minecraftforge:installertools:1.4.1",
                    "classpath": ["net.sf.jopt-simple:jopt-simple:6.0-alpha-3"],
                    "args": ["--task", "MCP_DATA", "--version", "{MINECRAFT_VERSION}"],
                    "outputs": {}
                },
                {
                    "sides": ["server"],
                    "jar": "net.minecraftforge:installertools:1.4.1",
                    "classpath": [],
                    "args": ["--task", "EXTRACT_FILES"],
                    "outputs": null
                }
            ]
        }"#;
        let p = parse_profile(text).unwrap();
        assert_eq!(p.spec, 1);
        assert_eq!(p.minecraft, "1.20.1");
        assert_eq!(p.json.as_deref(), Some("/version.json"));
        assert_eq!(p.libraries.len(), 2);
        assert_eq!(p.processors.len(), 2);
        assert!(p.processors[0].runs_on_client());
        assert!(!p.processors[1].runs_on_client());
        assert!(p.has_client_processors());
        assert!(!InstallProfile {
            processors: vec![],
            ..parse_profile(text).unwrap()
        }
        .has_client_processors());

        // downloads.artifact 直接采用；无 url 的旧式库默认走 forge maven
        let _dir = tempfile::tempdir().unwrap();
        let jobs_off =
            profile_library_jobs(&p, Mirror::Official, "https://maven.minecraftforge.net/")
                .unwrap();
        assert_eq!(jobs_off.len(), 2);
        assert_eq!(
            jobs_off[0].url,
            "https://maven.minecraftforge.net/com/github/jponge/lzma-java/1.3/lzma-java-1.3.jar"
        );
        assert_eq!(
            jobs_off[0].dest,
            "libraries/com/github/jponge/lzma-java/1.3/lzma-java-1.3.jar"
        );
        assert_eq!(
            jobs_off[1].url,
            "https://maven.minecraftforge.net/de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip"
        );
        assert_eq!(
            jobs_off[1].sha1.as_deref(),
            Some("c7d29380ddb38becad7c0819b5b325e43bca23f0")
        );
        let jobs_bm =
            profile_library_jobs(&p, Mirror::Bmclapi, "https://maven.minecraftforge.net/").unwrap();
        assert!(jobs_bm[0]
            .url
            .starts_with("https://bmclapi2.bangbang93.com/maven/"));
    }

    #[test]
    fn installer_url_and_cache() {
        let u = forge_maven_url(Mirror::Bmclapi, "1.20.1-47.3.0", "installer");
        assert_eq!(
            u,
            "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar"
        );
        assert!(installer_cache_dir("1.20.1-47.3.0").ends_with("mc-downloader-forge/1.20.1-47.3.0"));
        // 组件名清洗
        // 组件名清洗：不含路径分隔符、不是 "."/".."
        let name = installer_cache_dir("../evil")
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(!name.contains('/'));
        assert_ne!(name, "..");
        assert_ne!(name, ".");
    }

    #[test]
    fn forge_build_fields() {
        let json = r#"{
            "build": 47300000,
            "mcversion": "1.20.1",
            "version": "47.3.0",
            "modified": "2024-06-03T00:00:00.000Z",
            "files": [
                {"format": "txt", "category": "changelog", "hash": "aaa"},
                {"format": "jar", "category": "installer", "hash": "2bdc0e68"},
                {"format": "zip", "category": "mdk", "hash": "bbb"}
            ]
        }"#;
        let b: ForgeBuild = serde_json::from_str(json).unwrap();
        assert_eq!(b.artifact_id(), "1.20.1-47.3.0");
        assert_eq!(b.version_id(), "1.20.1-forge-47.3.0");
        assert_eq!(b.installer_sha1(), Some("2bdc0e68"));
    }

    #[test]
    fn neoforge_metadata_parse_and_prefix() {
        assert_eq!(neoforge_prefix_for_mc("1.20.4").as_deref(), Some("20.4."));
        assert_eq!(neoforge_prefix_for_mc("1.21").as_deref(), Some("21.0."));
        assert_eq!(neoforge_prefix_for_mc("1.26.3").as_deref(), Some("26.3."));
        assert_eq!(neoforge_prefix_for_mc("22w13a"), None);

        let xml = r#"<metadata><versioning><versions>
<version>26.1.0.0-alpha.1+snapshot-1</version>
<version>20.4.80</version>
<version>20.4.237</version>
<version>20.4.253</version>
<version>21.1.264</version>
<version>1.20.4-20.4.237</version>
</versions></versioning></metadata>"#;
        let vs = parse_neoforge_metadata(xml, "1.20.4");
        assert_eq!(vs, vec!["20.4.253", "20.4.237", "20.4.80"]);
        // 带后缀 / 其他 MC 前缀均被过滤
        assert_eq!(parse_neoforge_metadata(xml, "1.21.1"), vec!["21.1.264"]);
        assert_eq!(parse_neoforge_metadata(xml, "1.19.4"), Vec::<String>::new());
        // 降序对 4 段版本同样成立
        let vs26 = parse_neoforge_metadata(
            "<version>26.3.0.97</version><version>26.3.0.9</version>",
            "1.26.3",
        );
        assert_eq!(vs26, vec!["26.3.0.97", "26.3.0.9"]);
    }

    #[test]
    fn neoforge_urls_and_ids() {
        assert_eq!(neoforge_version_id("20.4.237"), "neoforge-20.4.237");
        let u = neoforge_installer_url(Mirror::Bmclapi, "20.4.237");
        assert_eq!(
            u,
            "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-installer.jar"
        );
        assert_eq!(
            neoforge_installer_url(Mirror::Official, "20.4.237"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-installer.jar"
        );
    }

    #[test]
    fn minecraft_jar_token_resolves_to_vanilla_client() {
        let dir = tempfile::tempdir().unwrap();
        let installer = dir.path().join("installer.jar");
        let work = dir.path().join("work");
        let data = HashMap::new();
        let c = ctx(dir.path(), &installer, &work, &data, "1.20.1");
        assert_eq!(
            resolve_token("{MINECRAFT_JAR}", &c).unwrap(),
            dir.path()
                .join("versions/1.20.1/1.20.1.jar")
                .display()
                .to_string()
        );
    }

    #[test]
    fn safe_join_used_in_profile_library_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let p = parse_profile(r#"{"libraries":[{"name":"a:b:1"}]}"#).unwrap();
        let jobs = profile_library_jobs(&p, Mirror::Official, "https://maven.minecraftforge.net/")
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].dest.starts_with("libraries/"));
        let _ = safe_join(dir.path(), &jobs[0].dest).unwrap();
    }
}

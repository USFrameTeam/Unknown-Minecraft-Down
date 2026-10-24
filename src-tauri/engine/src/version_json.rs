//! version JSON 解析：libraries / rules / natives / assetIndex，以及
//! `inheritsFrom` 继承链的递归解析与合并（Forge / Fabric 等加载器版本兼容）。
//!
//! rules 语义（与官方启动器一致）：
//! - 无 `rules` 字段 → 允许；
//! - 有 `rules` 字段 → 默认拒绝，按顺序求值，**最后一条命中的规则**决定结果。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::VersionSummary;
use crate::mirror::Mirror;
use crate::{EngineError, Result};

// ---------------------------------------------------------------------------
// 数据模型（未知字段通过 flatten 保留，落盘时不丢信息）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<VersionDownloads>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersionMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub game: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jvm: Vec<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<Rule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<HashMap<String, String>>,
    /// 旧式 maven 仓库基地址（如 https://libraries.minecraft.net/）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<ExtractRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksums: Option<Vec<String>>,
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractRules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexMeta {
    pub id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<Artifact>,
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersionMeta {
    #[serde(default)]
    pub major_version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
}

// ---------------------------------------------------------------------------
// OS 上下文与 rules 求值
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsName {
    Windows,
    Linux,
    Osx,
}

impl OsName {
    pub fn detect() -> Self {
        match std::env::consts::OS {
            "windows" => OsName::Windows,
            "macos" => OsName::Osx,
            _ => OsName::Linux,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            OsName::Windows => "windows",
            OsName::Linux => "linux",
            OsName::Osx => "osx",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OsContext {
    pub os: OsName,
    /// "x86" | "x64" | "arm64"
    pub arch: &'static str,
    /// 下载器不声明任何 feature（demo 用户、自定义分辨率等），恒为空。
    pub features: HashMap<String, bool>,
}

impl OsContext {
    pub fn detect() -> Self {
        let arch = match std::env::consts::ARCH {
            "x86" => "x86",
            "aarch64" => "arm64",
            _ => "x64",
        };
        OsContext {
            os: OsName::detect(),
            arch,
            features: HashMap::new(),
        }
    }
}

pub fn evaluate_rules(rules: Option<&[Rule]>, ctx: &OsContext) -> bool {
    let Some(rules) = rules else { return true };
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, ctx) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, ctx: &OsContext) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != ctx.os.as_str() {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != ctx.arch {
                return false;
            }
        }
    }
    if let Some(features) = &rule.features {
        if !features.is_empty() {
            for (key, want) in features {
                let want = want.as_bool().unwrap_or(true);
                if ctx.features.get(key).copied().unwrap_or(false) != want {
                    return false;
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// maven 坐标与 library artifact
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenRef {
    /// 仓库内相对路径，如 com/google/gson/gson/2.2.4/gson-2.2.4.jar
    pub path: String,
    pub classifier: Option<String>,
    pub ext: String,
}

/// 解析 maven 坐标 `group:artifact:version[:classifier][@ext]`。
pub fn parse_maven_name(name: &str) -> Result<MavenRef> {
    let (coords, ext) = match name.rsplit_once('@') {
        Some((c, e)) if !e.is_empty() => (c, e.to_string()),
        _ => (name, "jar".to_string()),
    };
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() < 3 || parts.len() > 4 || parts.iter().any(|p| p.is_empty()) {
        return Err(EngineError::Custom(format!("无法解析 maven 坐标: {name}")));
    }
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).copied().map(str::to_string);
    let file = match &classifier {
        Some(c) => format!("{artifact}-{version}-{c}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    let path = format!("{}/{}/{}/{}", group.replace('.', "/"), artifact, version, file);
    Ok(MavenRef {
        path,
        classifier,
        ext,
    })
}

/// natives 分类器：`{"windows": "natives-windows-${arch}", ...}` -> 当前平台的分类器。
pub fn natives_classifier(lib: &Library, ctx: &OsContext) -> Option<String> {
    let natives = lib.natives.as_ref()?;
    let raw = natives.get(ctx.os.as_str())?.clone();
    let bits = if ctx.arch == "x86" { "32" } else { "64" };
    Some(raw.replace("${arch}", bits))
}

/// Mojang 官方库仓库，version JSON 中无 url 旧式库的默认基地址。
pub const MOJANG_LIB_BASE: &str = "https://libraries.minecraft.net/";

/// 取 library 的下载 artifact。
/// - 新式 `downloads`：主 artifact 或 classifiers[classifier]；
/// - 旧式：`url`（默认官方库仓库）+ maven 坐标拼接；
/// `downloads` 存在但无匹配 classifier 时返回 `None`（该库对当前平台不适用）。
pub fn library_artifact(
    lib: &Library,
    ctx: &OsContext,
    classifier: Option<&str>,
) -> Result<Option<Artifact>> {
    library_artifact_with_base(lib, ctx, classifier, MOJANG_LIB_BASE)
}

/// 同 `library_artifact`，但旧式库的默认仓库基地址可指定
/// （Forge 的 install_profile.libraries 默认走 Forge maven）。
pub fn library_artifact_with_base(
    lib: &Library,
    _ctx: &OsContext,
    classifier: Option<&str>,
    default_base: &str,
) -> Result<Option<Artifact>> {
    if let Some(dl) = &lib.downloads {
        let found = match classifier {
            Some(c) => dl.classifiers.as_ref().and_then(|m| m.get(c)).cloned(),
            None => dl.artifact.clone(),
        };
        return Ok(found.filter(|a| !a.url.is_empty()));
    }
    let name = lib.name.as_deref().ok_or_else(|| {
        EngineError::Custom("library 缺少 name 字段且无 downloads 段".to_string())
    })?;
    let full_name = match classifier {
        Some(c) => format!("{name}:{c}"),
        None => name.to_string(),
    };
    let maven = parse_maven_name(&full_name)?;
    let base = lib.url.clone().unwrap_or_else(|| default_base.to_string());
    let base = if base.ends_with('/') { base } else { format!("{base}/") };
    Ok(Some(Artifact {
        path: Some(maven.path),
        url: format!("{base}{}", maven_name_path(&full_name)),
        sha1: lib.checksums.as_ref().and_then(|c| c.first().cloned()),
        size: None,
    }))
}

fn maven_name_path(name: &str) -> String {
    parse_maven_name(name)
        .map(|m| m.path)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 继承链解析（inheritsFrom）
// ---------------------------------------------------------------------------

/// 链上单个版本条目：解析后的 JSON + 来源信息（用于落盘与校验）。
#[derive(Debug, Clone)]
pub struct ChainEntry {
    pub json: VersionJson,
    /// 原始文本（本地读取或网络响应），落盘时保持字节一致。
    pub raw: Option<String>,
    pub source_url: Option<String>,
    pub manifest_sha1: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedChain {
    /// 根（通常是最底层 vanilla 版本）在最前，请求的加载器版本在最后。
    pub entries: Vec<ChainEntry>,
    pub requested_id: String,
    /// 客户端 jar 归属的版本 id（取链上最后出现的显式 `jar` 字段，否则为根版本 id）。
    pub jar_id: String,
    pub client_jar: Option<Artifact>,
    pub asset_index: AssetIndexMeta,
    /// 合并去重后的 libraries（子版本同名库覆盖父版本）。
    pub libraries: Vec<Library>,
}

const MAX_CHAIN_DEPTH: usize = 8;

/// 延迟获取 manifest 索引：只有当本地缺失某个父版本 JSON 时才会发起网络请求，
/// 因此本地完整的安装可以完全离线地完成校验。
pub struct ManifestHandle<'a> {
    client: &'a reqwest::Client,
    mirror: Mirror,
    cache: tokio::sync::OnceCell<HashMap<String, VersionSummary>>,
}

impl<'a> ManifestHandle<'a> {
    pub fn new(client: &'a reqwest::Client, mirror: Mirror) -> Self {
        Self {
            client,
            mirror,
            cache: tokio::sync::OnceCell::new(),
        }
    }

    pub async fn index(&self) -> Result<&HashMap<String, VersionSummary>> {
        self.cache
            .get_or_try_init(|| async {
                let manifest =
                    crate::manifest::fetch_manifest(self.client, self.mirror).await?;
                Ok(manifest.into_index())
            })
            .await
    }
}

/// 读取或抓取某个版本的 JSON。
/// 优先本地 `versions/<id>/<id>.json`（启动器/之前下载留下的），缺失则走清单。
pub async fn load_or_fetch_version_json(
    client: &reqwest::Client,
    mirror: Mirror,
    game_dir: &Path,
    id: &str,
    manifest: &ManifestHandle<'_>,
) -> Result<ChainEntry> {
    let local = game_dir.join(format!("versions/{id}/{id}.json"));
    if let Ok(text) = tokio::fs::read_to_string(&local).await {
        if let Ok(v) = serde_json::from_str::<VersionJson>(&text) {
            if v.id == id {
                return Ok(ChainEntry {
                    json: v,
                    raw: Some(text),
                    source_url: None,
                    manifest_sha1: None,
                });
            }
        }
        log::warn!("本地版本 JSON 无效，改从网络获取: {}", local.display());
    }
    let index = manifest.index().await?;
    let summary = index
        .get(id)
        .ok_or_else(|| EngineError::VersionNotFound(id.to_string()))?;
    fetch_version_json_by_summary(client, mirror, summary).await
}

pub async fn fetch_version_json_by_summary(
    client: &reqwest::Client,
    mirror: Mirror,
    summary: &VersionSummary,
) -> Result<ChainEntry> {
    let url = mirror.map_url(&summary.url)?;
    let resp = client.get(&url).send().await?.error_for_status()?;
    let text = resp.text().await?;
    let v: VersionJson = serde_json::from_str(&text)?;
    if v.id != summary.id {
        return Err(EngineError::Custom(format!(
            "版本 JSON 的 id（{}）与请求（{}）不一致",
            v.id, summary.id
        )));
    }
    Ok(ChainEntry {
        json: v,
        raw: Some(text),
        source_url: Some(url),
        manifest_sha1: summary.sha1.clone(),
    })
}

/// 从入口版本 JSON 出发，递归解析 `inheritsFrom` 链并合并。
pub async fn resolve_version(
    client: &reqwest::Client,
    mirror: Mirror,
    game_dir: &Path,
    entry: ChainEntry,
    manifest: &ManifestHandle<'_>,
) -> Result<ResolvedChain> {
    let requested_id = entry.json.id.clone();
    let mut chain = vec![entry];
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(requested_id.clone());

    loop {
        let Some(parent_id) = chain.last().and_then(|e| e.json.inherits_from.clone()) else {
            break;
        };
        if !seen.insert(parent_id.clone()) {
            return Err(EngineError::Custom(format!(
                "检测到 inheritsFrom 环: {parent_id}"
            )));
        }
        if chain.len() >= MAX_CHAIN_DEPTH {
            return Err(EngineError::Custom(
                "inheritsFrom 继承链过深（>8），疑似异常版本 JSON".into(),
            ));
        }
        let parent = load_or_fetch_version_json(client, mirror, game_dir, &parent_id, manifest)
            .await?;
        chain.push(parent);
    }
    chain.reverse(); // 根在最前

    // jar 归属：链上（根->子）最后出现的显式 `jar` 字段；默认为根版本 id。
    let mut jar_id = chain[0].json.id.clone();
    for e in &chain {
        if let Some(j) = &e.json.jar {
            jar_id = j.clone();
        }
    }
    let client_jar = chain
        .iter()
        .find(|e| e.json.id == jar_id)
        .and_then(|e| e.json.downloads.as_ref())
        .and_then(|d| d.client.clone())
        .filter(|a| !a.url.is_empty());

    // assetIndex：子版本优先。
    let asset_index = chain
        .iter()
        .rev()
        .find_map(|e| e.json.asset_index.clone())
        .ok_or_else(|| EngineError::Custom("继承链上缺少 assetIndex 定义".into()))?;

    // libraries 合并：父 + 子；同名（maven name 相同）子版本覆盖父版本。
    let mut libraries: Vec<Library> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for e in &chain {
        for lib in &e.json.libraries {
            match &lib.name {
                Some(n) => match by_name.get(n) {
                    Some(&i) => libraries[i] = lib.clone(),
                    None => {
                        by_name.insert(n.clone(), libraries.len());
                        libraries.push(lib.clone());
                    }
                },
                None => libraries.push(lib.clone()),
            }
        }
    }

    Ok(ResolvedChain {
        entries: chain,
        requested_id,
        jar_id,
        client_jar,
        asset_index,
        libraries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(os: OsName, arch: &'static str) -> OsContext {
        OsContext {
            os,
            arch,
            features: HashMap::new(),
        }
    }

    #[test]
    fn rules_no_rules_allows() {
        let c = ctx(OsName::Linux, "x64");
        assert!(evaluate_rules(None, &c));
    }

    #[test]
    fn rules_last_match_wins() {
        let c = ctx(OsName::Linux, "x64");
        let rules = vec![
            Rule {
                action: "allow".into(),
                os: Some(OsRule { name: Some("osx".into()), arch: None }),
                features: None,
            },
            Rule {
                action: "allow".into(),
                os: Some(OsRule { name: Some("linux".into()), arch: None }),
                features: None,
            },
        ];
        assert!(evaluate_rules(Some(&rules), &c));

        // 仅允许 osx → linux 上所有规则都不命中 → 默认拒绝
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule { name: Some("osx".into()), arch: None }),
            features: None,
        }];
        assert!(!evaluate_rules(Some(&rules), &c));
    }

    #[test]
    fn rules_arch_gate() {
        let c = ctx(OsName::Windows, "x64");
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule { name: Some("windows".into()), arch: Some("x86".into()) }),
            features: None,
        }];
        assert!(!evaluate_rules(Some(&rules), &c));
    }

    #[test]
    fn rules_features_unsatisfied_denies() {
        let c = ctx(OsName::Linux, "x64");
        let mut f = HashMap::new();
        f.insert("is_demo_user".to_string(), Value::Bool(true));
        let rules = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some(f),
        }];
        assert!(!evaluate_rules(Some(&rules), &c));
    }

    #[test]
    fn rules_deny_rule_blocks() {
        let c = ctx(OsName::Windows, "x64");
        let rules = vec![
            Rule {
                action: "allow".into(),
                os: Some(OsRule { name: Some("windows".into()), arch: None }),
                features: None,
            },
            Rule {
                action: "deny".into(),
                os: Some(OsRule { name: Some("windows".into()), arch: None }),
                features: None,
            },
        ];
        assert!(!evaluate_rules(Some(&rules), &c));
    }

    #[test]
    fn classifier_with_arch_placeholder() {
        let mut natives = HashMap::new();
        natives.insert("windows".to_string(), "natives-windows-${arch}".to_string());
        let lib = Library {
            natives: Some(natives),
            ..Default::default()
        };
        let c = ctx(OsName::Windows, "x64");
        assert_eq!(natives_classifier(&lib, &c), Some("natives-windows-64".into()));
        let c32 = ctx(OsName::Windows, "x86");
        assert_eq!(natives_classifier(&lib, &c32), Some("natives-windows-32".into()));
        let l = ctx(OsName::Linux, "x64");
        assert_eq!(natives_classifier(&lib, &l), None);
    }

    #[test]
    fn maven_name_parsing() {
        let m = parse_maven_name("com.google.code.gson:gson:2.2.4").unwrap();
        assert_eq!(m.path, "com/google/code/gson/gson/2.2.4/gson-2.2.4.jar");
        assert_eq!(m.classifier, None);

        let m = parse_maven_name("net.java.jinput:jinput-platform:2.0.5:natives-linux").unwrap();
        assert_eq!(
            m.path,
            "net/java/jinput/jinput-platform/2.0.5/jinput-platform-2.0.5-natives-linux.jar"
        );

        let m = parse_maven_name("com.twitch:twitch-platform:5.16:natives-windows-${arch}@zip").unwrap();
        assert_eq!(m.ext, "zip");
        assert!(m.path.ends_with(".zip"));

        assert!(parse_maven_name("bad-name").is_err());
        assert!(parse_maven_name("a:b:").is_err());
    }

    #[test]
    fn legacy_artifact_uses_lib_url() {
        let lib = Library {
            name: Some("org.lwjgl.lwjgl:lwjgl:3.1.6".into()),
            url: Some("https://maven.fabricmc.net/".into()),
            ..Default::default()
        };
        let c = ctx(OsName::Linux, "x64");
        let art = library_artifact(&lib, &c, None).unwrap().unwrap();
        assert_eq!(
            art.url,
            "https://maven.fabricmc.net/org/lwjgl/lwjgl/lwjgl/3.1.6/lwjgl-3.1.6.jar"
        );
        // 无尾斜杠的 base 也要补上
        let lib2 = Library { url: Some("https://maven.example.com".into()), ..lib.clone() };
        let art2 = library_artifact(&lib2, &c, None).unwrap().unwrap();
        assert!(art2.url.starts_with("https://maven.example.com/"));
    }

    #[test]
    fn downloads_without_matching_classifier_returns_none() {
        let lib = Library {
            name: Some("a:b:1.0".into()),
            downloads: Some(LibraryDownloads {
                artifact: Some(Artifact { url: "https://x/a.jar".into(), ..Default::default() }),
                classifiers: None,
            }),
            natives: Some(HashMap::from([("linux".into(), "natives-linux".into())])),
            ..Default::default()
        };
        let c = ctx(OsName::Linux, "x64");
        // 主 artifact 可用
        assert!(library_artifact(&lib, &c, None).unwrap().is_some());
        // natives classifier 不存在 → None
        assert!(library_artifact(&lib, &c, Some("natives-linux")).unwrap().is_none());
    }
}

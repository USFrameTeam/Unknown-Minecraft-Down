//! version_manifest_v2.json 的获取与解析。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::mirror::Mirror;
use crate::{EngineError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub release_time: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub latest: Latest,
    pub versions: Vec<VersionSummary>,
}

impl Manifest {
    pub fn find(&self, id: &str) -> Option<&VersionSummary> {
        self.versions.iter().find(|v| v.id == id)
    }

    pub fn into_index(self) -> HashMap<String, VersionSummary> {
        self.versions
            .into_iter()
            .map(|v| (v.id.clone(), v))
            .collect()
    }
}

pub async fn fetch_manifest(client: &reqwest::Client, mirror: Mirror) -> Result<Manifest> {
    let url = mirror.manifest_url();
    let resp = client.get(url).send().await?.error_for_status()?;
    Ok(resp.json::<Manifest>().await?)
}

/// 按 id 在清单中查找版本摘要；找不到返回 `VersionNotFound`。
pub async fn fetch_version_summary(
    client: &reqwest::Client,
    mirror: Mirror,
    id: &str,
) -> Result<VersionSummary> {
    let manifest = fetch_manifest(client, mirror).await?;
    manifest
        .find(id)
        .cloned()
        .ok_or_else(|| EngineError::VersionNotFound(id.to_string()))
}

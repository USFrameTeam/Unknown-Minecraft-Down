//! 下载源域名映射：Mojang 官方 <-> BMCLAPI 镜像。
//!
//! BMCLAPI 的镜像策略是对官方域名做整体替换，路径保持一致：
//! - piston-meta.mojang.com / launchermeta.mojang.com -> bmclapi2.bangbang93.com
//! - piston-data.mojang.com                           -> bmclapi2.bangbang93.com
//! - resources.download.minecraft.net                 -> bmclapi2.bangbang93.com/assets
//! - libraries.minecraft.net                          -> bmclapi2.bangbang93.com/maven
//!
//! 硬约束：所有请求强制升级为 HTTPS，其余域名原样保留。

use serde::{Deserialize, Serialize};

use crate::{EngineError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mirror {
    Official,
    Bmclapi,
}

impl Mirror {
    pub const OFFICIAL_MANIFEST: &'static str =
        "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

    pub fn as_str(&self) -> &'static str {
        match self {
            Mirror::Official => "official",
            Mirror::Bmclapi => "bmclapi",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "bmclapi" => Mirror::Bmclapi,
            _ => Mirror::Official,
        }
    }

    pub fn manifest_url(&self) -> &'static str {
        Self::OFFICIAL_MANIFEST
    }

    /// Forge 官方 maven 基地址（profile/version JSON 中无 url 的库默认从此解析）。
    pub fn forge_maven_base(&self) -> &'static str {
        match self {
            Mirror::Official => "https://maven.minecraftforge.net/",
            Mirror::Bmclapi => "https://bmclapi2.bangbang93.com/maven/",
        }
    }

    /// NeoForge 官方 maven 基地址。
    pub fn neoforge_maven_base(&self) -> &'static str {
        match self {
            Mirror::Official => "https://maven.neoforged.net/",
            Mirror::Bmclapi => "https://bmclapi2.bangbang93.com/maven/",
        }
    }

    /// 把任一 MC 资源 URL 映射到当前镜像，并将 `http://` 强制升级为 `https://`。
    /// 环回地址（测试/自动化端点）豁免强制升级。
    pub fn map_url(&self, raw: &str) -> Result<String> {
        let mut url = raw.trim().to_string();
        if let Some(rest) = url.strip_prefix("http://") {
            let host = rest.split('/').next().unwrap_or("");
            let local = host.starts_with("127.0.0.1")
                || host.starts_with("localhost")
                || host.starts_with("[::1]")
                || host.ends_with(".localhost");
            if !local {
                url = format!("https://{rest}");
            }
        }
        if !url.starts_with("https://")
            && !url.starts_with("http://127.0.0.1")
            && !url.starts_with("http://localhost")
            && !url.starts_with("http://[::1]")
        {
            return Err(EngineError::InvalidUrl(raw.to_string()));
        }
        Ok(match self {
            Mirror::Official => url,
            Mirror::Bmclapi => rewrite_to_bmclapi(&url),
        })
    }
}

pub fn rewrite_to_bmclapi(url: &str) -> String {
    const BMCL: &str = "https://bmclapi2.bangbang93.com";
    url.replace("https://piston-meta.mojang.com", BMCL)
        .replace("https://launchermeta.mojang.com", BMCL)
        .replace("https://launcher.mojang.com", BMCL)
        .replace("https://piston-data.mojang.com", BMCL)
        .replace(
            "https://resources.download.minecraft.net",
            &format!("{BMCL}/assets"),
        )
        .replace("https://libraries.minecraft.net", &format!("{BMCL}/maven"))
        .replace("https://maven.minecraftforge.net", &format!("{BMCL}/maven"))
        // NeoForge 官方带 /releases/ 段，BMCLAPI 镜像不带，需先处理长前缀
        .replace(
            "https://maven.neoforged.net/releases/",
            &format!("{BMCL}/maven/"),
        )
        .replace("https://maven.neoforged.net/", &format!("{BMCL}/maven/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_passthrough_keeps_https() {
        let m = Mirror::Official;
        assert_eq!(
            m.map_url("https://libraries.minecraft.net/a/b.jar")
                .unwrap(),
            "https://libraries.minecraft.net/a/b.jar"
        );
    }

    #[test]
    fn upgrades_http_to_https() {
        let m = Mirror::Official;
        assert_eq!(
            m.map_url("http://resources.download.minecraft.net/aa/bb")
                .unwrap(),
            "https://resources.download.minecraft.net/aa/bb"
        );
    }

    #[test]
    fn rejects_non_http() {
        assert!(Mirror::Official.map_url("ftp://example.com/x").is_err());
        assert!(Mirror::Official.map_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn loopback_http_is_exempt_from_https_upgrade() {
        let m = Mirror::Official;
        assert_eq!(
            m.map_url("http://127.0.0.1:1234/lib.jar").unwrap(),
            "http://127.0.0.1:1234/lib.jar"
        );
        assert_eq!(
            m.map_url("http://localhost:5678/x.json").unwrap(),
            "http://localhost:5678/x.json"
        );
        // 非环回的 http 仍必须强升 https
        assert_eq!(
            m.map_url("http://example.com/x").unwrap(),
            "https://example.com/x"
        );
    }

    #[test]
    fn bmclapi_rewrites_all_official_hosts() {
        let m = Mirror::Bmclapi;
        assert_eq!(
            m.map_url("https://piston-meta.mojang.com/v1/packages/x/1.20.1.json")
                .unwrap(),
            "https://bmclapi2.bangbang93.com/v1/packages/x/1.20.1.json"
        );
        assert_eq!(
            m.map_url("https://piston-data.mojang.com/v1/objects/abc/client.jar")
                .unwrap(),
            "https://bmclapi2.bangbang93.com/v1/objects/abc/client.jar"
        );
        assert_eq!(
            m.map_url("https://resources.download.minecraft.net/ab/abcd")
                .unwrap(),
            "https://bmclapi2.bangbang93.com/assets/ab/abcd"
        );
        assert_eq!(
            m.map_url("https://libraries.minecraft.net/com/g/g/a.jar")
                .unwrap(),
            "https://bmclapi2.bangbang93.com/maven/com/g/g/a.jar"
        );
        assert_eq!(
            m.map_url("http://launchermeta.mojang.com/mc/game/version_manifest_v2.json")
                .unwrap(),
            "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json"
        );
    }

    #[test]
    fn bmclapi_leaves_third_party_hosts_alone() {
        let m = Mirror::Bmclapi;
        assert_eq!(
            m.map_url("https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar").unwrap(),
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
        );
    }

    #[test]
    fn bmclapi_rewrites_neoforge_maven() {
        let m = Mirror::Bmclapi;
        assert_eq!(
            m.map_url("https://maven.neoforged.net/releases/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-universal.jar").unwrap(),
            "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-universal.jar"
        );
        assert_eq!(
            Mirror::Official.neoforge_maven_base(),
            "https://maven.neoforged.net/"
        );
        assert_eq!(
            Mirror::Bmclapi.neoforge_maven_base(),
            "https://bmclapi2.bangbang93.com/maven/"
        );
    }

    #[test]
    fn bmclapi_rewrites_forge_maven() {
        let m = Mirror::Bmclapi;
        assert_eq!(
            m.map_url("https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-universal.jar").unwrap(),
            "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-universal.jar"
        );
        assert_eq!(
            m.forge_maven_base(),
            "https://bmclapi2.bangbang93.com/maven/"
        );
        assert_eq!(
            Mirror::Official.forge_maven_base(),
            "https://maven.minecraftforge.net/"
        );
    }

    #[test]
    fn parse_mirror() {
        assert_eq!(Mirror::parse("bmclapi"), Mirror::Bmclapi);
        assert_eq!(Mirror::parse("official"), Mirror::Official);
        assert_eq!(Mirror::parse("unknown"), Mirror::Official);
    }
}

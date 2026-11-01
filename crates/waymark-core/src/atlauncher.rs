//! ATLauncher's public modpacks. A version is a list of files on their CDN plus
//! a zip of configs. Packs for old Minecraft versions patch the game's own jar,
//! which Waymark doesn't do, so only lines that mostly use a normal loader are offered.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Core;
use crate::content::{ContentHit, ContentKind, ContentPage, ContentSource, SearchQuery};
use crate::error::{Error, Result};
use crate::install::safe_relative;
use crate::lock::{Loader, LoaderKind};
use crate::projects::{BodyFormat, Link, ProjectPage, VersionRow};

const API: &str = "https://api.atlauncher.com/v1";
pub const CDN: &str = "https://download.nodecdn.net/containers/atl";
const ICONS: &str = "https://cdn.atlcdn.net/images/packs";
const SITE: &str = "https://atlauncher.com/pack";
/// Where a pack's own list may send a download. Anything else is left for the player.
const DIRECT_HOSTS: [&str; 5] = [
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
    "cdn.modrinth.com",
    "github.com",
    "download.nodecdn.net",
];
/// The oldest Minecraft line whose packs mostly don't need jar patching.
const FIRST_MINOR: u32 = 12;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub name: String,
    pub safe_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "websiteURL")]
    pub website_url: Option<String>,
    #[serde(default)]
    pub versions: Vec<PackVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackVersion {
    pub version: String,
    #[serde(default)]
    pub minecraft: String,
    #[serde(default)]
    pub published: i64,
}

#[derive(Serialize, Deserialize)]
struct Listing {
    #[serde(default)]
    data: Vec<Pack>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub minecraft: String,
    #[serde(default)]
    pub loader: Option<ConfigLoader>,
    #[serde(default)]
    pub mods: Vec<ConfigMod>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigLoader {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub metadata: LoaderMetadata,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LoaderMetadata {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMod {
    pub name: String,
    #[serde(default)]
    pub url: String,
    pub file: String,
    #[serde(default)]
    pub download: String,
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub extract_to: Option<String>,
    #[serde(default = "yes")]
    pub client: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub recommended: bool,
}

/// Where a file ends up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Copied to this path.
    File(String),
    /// Unpacked into this folder, or the game folder when it's empty.
    Unpack(String),
}

impl ConfigMod {
    /// Whether a fresh install takes this file. Optional files follow the
    /// pack author's recommendation, the same as ATLauncher's own defaults.
    pub fn wanted(&self) -> bool {
        self.client && (!self.optional || self.recommended)
    }

    /// Where it goes. Files meant for the game's own jar have no place here.
    pub fn placement(&self) -> Result<Option<Placement>> {
        safe_relative(&self.file)?;
        let folder = match self.kind.as_str() {
            "mods" | "dependency" => "mods",
            "coremods" => "coremods",
            "resourcepack" => "resourcepacks",
            "texturepack" => "texturepacks",
            "shaderpack" => "shaderpacks",
            "extract" | "root" | "resourcepackextract" | "texturepackextract" => {
                let base = match self.extract_to.as_deref() {
                    None | Some("" | "root") => String::new(),
                    Some("resourcepack") => "resourcepacks".to_string(),
                    Some("texturepack") => "texturepacks".to_string(),
                    Some(other) => other.to_string(),
                };
                if !base.is_empty() {
                    safe_relative(&base)?;
                }
                return Ok(Some(Placement::Unpack(base)));
            }
            _ => return Ok(None),
        };
        Ok(Some(Placement::File(format!("{folder}/{}", self.file))))
    }

    /// The address to fetch, if it's one that can be trusted.
    pub fn address(&self) -> Option<String> {
        match self.download.as_str() {
            "server" => Some(format!("{CDN}/{}", self.url.trim_start_matches('/'))),
            "direct" => {
                let host = self.url.strip_prefix("https://")?.split('/').next()?;
                DIRECT_HOSTS.contains(&host).then(|| self.url.clone())
            }
            _ => None,
        }
    }
}

/// Reads the Minecraft version and loader a version uses, or says why it can't be installed.
pub fn game_of(config: &Config) -> Result<(String, Loader)> {
    let Some(loader) = &config.loader else {
        return Err(Error::Other(
            "this version patches the game's own files, which Waymark doesn't support yet".into(),
        ));
    };
    let kind = match loader.kind.as_str() {
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        other => {
            return Err(Error::Other(format!(
                "this pack uses {other}, which Waymark doesn't support"
            )));
        }
    };
    let version = loader
        .metadata
        .loader
        .clone()
        .filter(|v| !v.is_empty())
        .or_else(|| loader.metadata.version.clone());
    Ok((config.minecraft.clone(), Loader { kind, version }))
}

fn supported(minecraft: &str) -> bool {
    let mut parts = minecraft.split('.').map(|part| part.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(1), Some(minor)) => minor >= FIRST_MINOR,
        (Some(major), _) => major >= 23,
        _ => false,
    }
}

/// The versions Waymark can install, newest first.
pub fn usable(pack: &Pack) -> Vec<&PackVersion> {
    let mut versions: Vec<&PackVersion> = pack
        .versions
        .iter()
        .filter(|version| supported(&version.minecraft))
        .collect();
    versions.sort_by_key(|version| std::cmp::Reverse(version.published));
    versions
}

fn date(seconds: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(seconds)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

fn icon(pack: &Pack) -> String {
    format!("{ICONS}/{}.png", pack.safe_name.to_ascii_lowercase())
}

fn hit(pack: &Pack) -> ContentHit {
    ContentHit {
        source: ContentSource::AtLauncher,
        id: pack.safe_name.clone(),
        slug: pack.safe_name.clone(),
        title: pack.name.clone(),
        summary: pack
            .description
            .clone()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string(),
        author: String::new(),
        downloads: 0,
        icon: Some(icon(pack)),
        banner: None,
        kind: ContentKind::Modpack,
        categories: Vec::new(),
    }
}

pub fn page(pack: &Pack) -> ProjectPage {
    let versions = usable(pack);
    let mut game_versions = Vec::new();
    for version in &versions {
        if !game_versions.contains(&version.minecraft) {
            game_versions.push(version.minecraft.clone());
        }
    }
    ProjectPage {
        source: ContentSource::AtLauncher,
        id: pack.safe_name.clone(),
        slug: pack.safe_name.clone(),
        kind: ContentKind::Modpack,
        title: pack.name.clone(),
        summary: pack
            .description
            .clone()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string(),
        body: pack.description.clone().unwrap_or_default(),
        format: BodyFormat::Markdown,
        icon: Some(icon(pack)),
        downloads: 0,
        followers: 0,
        published: versions.last().and_then(|version| date(version.published)),
        updated: versions.first().and_then(|version| date(version.published)),
        categories: Vec::new(),
        loaders: Vec::new(),
        game_versions,
        client: None,
        server: None,
        license: None,
        links: pack
            .website_url
            .iter()
            .filter(|url| url.starts_with("https://"))
            .map(|url| Link {
                label: "Website".into(),
                url: url.clone(),
            })
            .collect(),
        gallery: Vec::new(),
        creators: Vec::new(),
        page: format!("{SITE}/{}", pack.safe_name),
    }
}

pub fn rows(pack: &Pack) -> Vec<VersionRow> {
    usable(pack)
        .into_iter()
        .map(|version| VersionRow {
            id: version.version.clone(),
            name: version.version.clone(),
            number: version.version.clone(),
            channel: "release".to_string(),
            published: date(version.published).unwrap_or_default(),
            downloads: 0,
            loaders: Vec::new(),
            game_versions: vec![version.minecraft.clone()],
            file: String::new(),
            size: 0,
            available: true,
        })
        .collect()
}

/// Ids go in addresses, so only plain ones are accepted.
fn plain(text: &str) -> Result<&str> {
    let fine = !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if fine {
        Ok(text)
    } else {
        Err(Error::NotFound(format!("the pack {text}")))
    }
}

impl Core {
    async fn atl_packs(&self) -> Result<Vec<Pack>> {
        let cache = self.paths.cache().join("atlauncher-packs.json");
        let listing: Listing = self
            .net
            .cached_json(
                &format!("{API}/packs/full/public"),
                &cache,
                std::time::Duration::from_secs(3600),
            )
            .await?;
        Ok(listing.data)
    }

    pub(crate) async fn atl_pack(&self, id: &str) -> Result<Pack> {
        self.atl_packs()
            .await?
            .into_iter()
            .find(|pack| pack.safe_name == id)
            .ok_or_else(|| Error::NotFound(format!("the pack {id}")))
    }

    pub(crate) async fn atl_config(&self, pack: &str, version: &str) -> Result<Config> {
        self.net
            .json(&format!(
                "{CDN}/packs/{}/versions/{}/Configs.json",
                plain(pack)?,
                plain(version)?
            ))
            .await
    }

    pub(crate) async fn atl_search(&self, query: &SearchQuery) -> Result<ContentPage> {
        if query.kind != ContentKind::Modpack {
            return Ok(ContentPage {
                hits: Vec::new(),
                total: 0,
            });
        }
        let text = query.text.trim().to_lowercase();
        let packs = self.atl_packs().await?;
        let mut found: Vec<(&Pack, i64)> = packs
            .iter()
            .filter(|pack| text.is_empty() || pack.name.to_lowercase().contains(&text))
            .filter_map(|pack| {
                let versions = usable(pack);
                let latest = versions.iter().find(|version| {
                    query
                        .minecraft
                        .as_ref()
                        .is_none_or(|wanted| *wanted == version.minecraft)
                })?;
                Some((pack, latest.published))
            })
            .collect();
        found.sort_by_key(|(_, published)| std::cmp::Reverse(*published));
        let total = found.len() as u64;
        let hits = found
            .into_iter()
            .skip(query.offset as usize)
            .take(query.limit as usize)
            .map(|(pack, _)| hit(pack))
            .collect();
        Ok(ContentPage { hits, total })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, download: &str, url: &str) -> ConfigMod {
        ConfigMod {
            name: "m".into(),
            url: url.into(),
            file: "m.jar".into(),
            download: download.into(),
            filesize: None,
            kind: kind.into(),
            extract_to: None,
            client: true,
            optional: false,
            recommended: true,
        }
    }

    #[test]
    fn files_land_in_the_right_folder() {
        assert_eq!(
            entry("mods", "server", "").placement().unwrap(),
            Some(Placement::File("mods/m.jar".into()))
        );
        assert_eq!(
            entry("extract", "server", "").placement().unwrap(),
            Some(Placement::Unpack(String::new()))
        );
        assert_eq!(entry("jar", "server", "").placement().unwrap(), None);
        let mut hostile = entry("mods", "server", "");
        hostile.file = "../../evil.exe".into();
        assert!(hostile.placement().is_err());
        let mut escape = entry("extract", "server", "");
        escape.extract_to = Some("../..".into());
        assert!(escape.placement().is_err());
    }

    #[test]
    fn only_known_hosts_are_fetched() {
        assert_eq!(
            entry("mods", "server", "packs/x/m.jar").address().unwrap(),
            format!("{CDN}/packs/x/m.jar")
        );
        assert!(
            entry(
                "mods",
                "direct",
                "https://edge.forgecdn.net/files/1/2/m.jar"
            )
            .address()
            .is_some()
        );
        assert!(
            entry("mods", "direct", "http://edge.forgecdn.net/files/1/2/m.jar")
                .address()
                .is_none()
        );
        assert!(
            entry(
                "mods",
                "direct",
                "https://evil.example/edge.forgecdn.net/m.jar"
            )
            .address()
            .is_none()
        );
        assert!(
            entry("mods", "browser", "https://edge.forgecdn.net/m.jar")
                .address()
                .is_none()
        );
    }

    #[test]
    fn optional_files_follow_the_recommendation() {
        let mut item = entry("mods", "server", "");
        item.optional = true;
        assert!(item.wanted());
        item.recommended = false;
        assert!(!item.wanted());
        item.optional = false;
        assert!(item.wanted());
        item.client = false;
        assert!(!item.wanted());
    }

    #[test]
    fn reads_the_loader_or_says_why_not() {
        let config: Config = serde_json::from_str(r#"{"minecraft":"1.21.1","loader":{"type":"neoforge","metadata":{"version":"21.1.83","loader":""}},"mods":[]}"#).unwrap();
        let (minecraft, loader) = game_of(&config).unwrap();
        assert_eq!(minecraft, "1.21.1");
        assert_eq!(loader.kind, LoaderKind::NeoForge);
        assert_eq!(loader.version.as_deref(), Some("21.1.83"));

        let fabric: Config = serde_json::from_str(
            r#"{"minecraft":"1.20.1","loader":{"type":"fabric","metadata":{"loader":"0.14.21"}}}"#,
        )
        .unwrap();
        assert_eq!(
            game_of(&fabric).unwrap().1.version.as_deref(),
            Some("0.14.21")
        );

        let old: Config = serde_json::from_str(r#"{"minecraft":"1.6.4"}"#).unwrap();
        assert!(game_of(&old).is_err());
    }

    #[test]
    fn old_minecraft_versions_are_not_offered() {
        assert!(
            supported("1.12.2") && supported("1.16.5") && supported("1.21.1") && supported("26.1")
        );
        assert!(!supported("1.11.2") && !supported("1.7.10"));
    }

    #[test]
    fn addresses_only_take_plain_ids() {
        assert!(plain("DNSTechpack").is_ok());
        assert!(plain("../../etc").is_err());
        assert!(plain("a/b").is_err());
    }
}

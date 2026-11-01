//! Feed The Beast's modpack API. Unlike the other stores there's no pack file
//! to download: a version is a list of files, each with its own address.

use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::content::{ContentHit, ContentKind, ContentPage, ContentSource, SearchQuery};
use crate::error::{Error, Result};
use crate::install::safe_relative;
use crate::lock::{Loader, LoaderKind};
use crate::net::Net;
use crate::projects::{BodyFormat, Creator, Link, ProjectPage, VersionRow};

const API: &str = "https://api.feed-the-beast.com/v1/modpacks/public/modpack";
const SITE: &str = "https://www.feed-the-beast.com/modpacks";
/// Where pack files really come from. Anything else in a list is refused.
const ALLOWED_HOSTS: [&str; 4] = [
    "files.feed-the-beast.com",
    "cdn.feed-the-beast.com",
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
];

#[derive(Clone, Debug, Deserialize)]
pub struct Pack {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub installs: u64,
    #[serde(default)]
    pub released: i64,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub art: Vec<Art>,
    #[serde(default)]
    pub links: Vec<PackLink>,
    #[serde(default)]
    pub versions: Vec<PackVersion>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Author {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Art {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackLink {
    pub name: String,
    pub link: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Target {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackVersion {
    pub id: u64,
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub targets: Vec<Target>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VersionFiles {
    #[serde(default)]
    pub files: Vec<PackFile>,
    #[serde(default)]
    pub targets: Vec<Target>,
}

/// The API sends these ids as numbers for some packs and as text for others.
fn number_or_text<'de, D: serde::Deserializer<'de>>(from: D) -> std::result::Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(u64),
        Text(String),
    }
    match Id::deserialize(from)? {
        Id::Number(id) => Ok(id),
        Id::Text(text) => text.parse().map_err(serde::de::Error::custom),
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct CurseForgeFile {
    #[serde(deserialize_with = "number_or_text")]
    pub project: u64,
    #[serde(deserialize_with = "number_or_text")]
    pub file: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackFile {
    #[serde(default)]
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub serveronly: bool,
    #[serde(default)]
    pub curseforge: Option<CurseForgeFile>,
}

impl PackFile {
    /// Where the file goes in the game folder, checked because it comes from a list.
    pub fn location(&self) -> Result<String> {
        let folder = self.path.trim_start_matches("./").trim_matches('/');
        let path = if folder.is_empty() {
            self.name.clone()
        } else {
            format!("{folder}/{}", self.name)
        };
        safe_relative(&path)?;
        Ok(path)
    }

    /// Whether the address is one FTB really uses.
    pub fn trusted(&self) -> bool {
        let Some(rest) = self.url.strip_prefix("https://") else {
            return false;
        };
        ALLOWED_HOSTS.contains(&rest.split('/').next().unwrap_or(""))
    }
}

fn loader_kind(name: &str) -> Option<LoaderKind> {
    match name {
        "forge" => Some(LoaderKind::Forge),
        "neoforge" => Some(LoaderKind::NeoForge),
        "fabric" => Some(LoaderKind::Fabric),
        "quilt" => Some(LoaderKind::Quilt),
        _ => None,
    }
}

/// A version's Minecraft and loader, from its list of targets.
pub fn game_of(targets: &[Target]) -> Option<(String, Loader)> {
    let minecraft = targets
        .iter()
        .find(|target| target.name == "minecraft")?
        .version
        .clone();
    let loader = targets.iter().find_map(|target| {
        Some(Loader {
            kind: loader_kind(&target.name)?,
            version: Some(target.version.clone()),
        })
    });
    Some((minecraft, loader.unwrap_or_default()))
}

fn icon(pack: &Pack) -> Option<String> {
    pack.art
        .iter()
        .find(|art| art.kind == "square")
        .or(pack.art.first())
        .map(|art| art.url.clone())
}

fn date(seconds: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(seconds)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

pub async fn pack(net: &Net, id: &str) -> Result<Pack> {
    net.json(&format!("{API}/{id}")).await
}

pub async fn version_files(net: &Net, pack: &str, version: &str) -> Result<VersionFiles> {
    net.json(&format!("{API}/{pack}/{version}")).await
}

/// The version to install: the newest full release, or failing that the newest of anything.
pub fn newest(pack: &Pack) -> Option<&PackVersion> {
    let latest = |full: bool| {
        pack.versions
            .iter()
            .filter(|v| !full || v.kind == "release")
            .max_by_key(|v| v.id)
    };
    latest(true).or_else(|| latest(false))
}

/// Forge has no usable installer before 1.6, so those packs can't be set up.
fn installable(minecraft: &str) -> bool {
    !matches!(minecraft.split('.').nth(1).and_then(|minor| minor.parse::<u32>().ok()), Some(minor) if minecraft.starts_with("1.") && minor < 6)
}

fn fits(pack: &Pack, query: &SearchQuery) -> bool {
    pack.versions.iter().any(|version| {
        game_of(&version.targets).is_some_and(|(minecraft, loader)| {
            installable(&minecraft)
                && query
                    .minecraft
                    .as_ref()
                    .is_none_or(|wanted| *wanted == minecraft)
                && query.loader.is_none_or(|wanted| wanted == loader.kind)
        })
    })
}

#[derive(Deserialize)]
struct Ids {
    #[serde(default)]
    packs: Vec<u64>,
}

/// The most packs one search will give back.
const REACH: u32 = 100;

pub async fn search(net: &Net, query: &SearchQuery) -> Result<ContentPage> {
    if query.kind != ContentKind::Modpack {
        return Ok(ContentPage {
            hits: Vec::new(),
            total: 0,
        });
    }
    // There's no paging, so ask for all of them and slice the page out here.
    let ids: Ids = if query.text.trim().is_empty() {
        net.json(&format!("{API}/popular/installs/FTB/{REACH}"))
            .await?
    } else {
        Ok::<Ids, Error>(
            net.client()
                .get(format!("{API}/search/{REACH}"))
                .query(&[("term", query.text.trim())])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?,
        )?
    };
    let total = ids.packs.len() as u64;
    let page: Vec<String> = ids
        .packs
        .iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .map(u64::to_string)
        .collect();
    let found = futures_util::future::join_all(page.iter().map(|id| pack(net, id))).await;
    let hits = found
        .into_iter()
        .flatten()
        .filter(|details| fits(details, query))
        .map(|details| ContentHit {
            source: ContentSource::Ftb,
            id: details.id.to_string(),
            icon: icon(&details),
            banner: None,
            slug: details.slug,
            title: details.name,
            summary: details.synopsis,
            author: details
                .authors
                .first()
                .map(|author| author.name.clone())
                .unwrap_or_default(),
            downloads: details.installs,
            kind: ContentKind::Modpack,
            categories: Vec::new(),
        })
        .collect();
    Ok(ContentPage { hits, total })
}

pub fn page(pack: Pack) -> ProjectPage {
    let mut game_versions = Vec::new();
    let mut loaders = Vec::new();
    for version in &pack.versions {
        let Some((minecraft, loader)) = game_of(&version.targets) else {
            continue;
        };
        if !game_versions.contains(&minecraft) {
            game_versions.push(minecraft);
        }
        if loader.kind != LoaderKind::Vanilla
            && !loaders.iter().any(|known| known == loader.kind.slug())
        {
            loaders.push(loader.kind.slug().to_string());
        }
    }
    ProjectPage {
        source: ContentSource::Ftb,
        id: pack.id.to_string(),
        page: format!("{SITE}/{}-{}", pack.id, pack.slug),
        icon: icon(&pack),
        slug: pack.slug,
        kind: ContentKind::Modpack,
        title: pack.name,
        summary: pack.synopsis,
        body: pack.description,
        format: BodyFormat::Markdown,
        downloads: pack.installs,
        followers: 0,
        published: date(pack.released),
        updated: date(pack.updated),
        categories: Vec::new(),
        loaders,
        game_versions,
        client: None,
        server: None,
        license: None,
        links: pack
            .links
            .into_iter()
            .filter(|link| link.link.starts_with("https://"))
            .map(|link| Link {
                label: link.name,
                url: link.link,
            })
            .collect(),
        gallery: Vec::new(),
        creators: pack
            .authors
            .into_iter()
            .map(|author| Creator {
                name: author.name,
                role: None,
                avatar: None,
                url: None,
            })
            .collect(),
    }
}

/// Newest first, to match the other stores.
pub fn rows(pack: &Pack) -> Vec<VersionRow> {
    let mut versions: Vec<&PackVersion> = pack.versions.iter().collect();
    versions.sort_by_key(|version| std::cmp::Reverse(version.id));
    versions
        .into_iter()
        .map(|version| {
            let (minecraft, loader) = game_of(&version.targets).unwrap_or_default();
            VersionRow {
                id: version.id.to_string(),
                name: version.name.clone(),
                number: version.name.clone(),
                channel: if version.kind == "release" {
                    "release".to_string()
                } else {
                    "beta".to_string()
                },
                published: date(version.updated).unwrap_or_default(),
                downloads: 0,
                loaders: if loader.kind == LoaderKind::Vanilla {
                    Vec::new()
                } else {
                    vec![loader.kind.slug().to_string()]
                },
                game_versions: vec![minecraft],
                file: String::new(),
                size: 0,
                available: true,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, name: &str, url: &str) -> PackFile {
        PackFile {
            path: path.into(),
            name: name.into(),
            url: url.into(),
            sha1: String::new(),
            size: 0,
            serveronly: false,
            curseforge: None,
        }
    }

    #[test]
    fn files_land_where_the_list_says_and_nowhere_else() {
        assert_eq!(
            file("./config", "a.toml", "").location().unwrap(),
            "config/a.toml"
        );
        assert_eq!(
            file("./", "options.txt", "").location().unwrap(),
            "options.txt"
        );
        assert_eq!(file("mods/", "m.jar", "").location().unwrap(), "mods/m.jar");
        assert!(file("../..", "evil.exe", "").location().is_err());
        assert!(file("./config", "../../evil.exe", "").location().is_err());
        assert!(file("./config\\..", "evil.exe", "").location().is_err());
    }

    #[test]
    fn only_known_hosts_are_trusted() {
        assert!(file("", "a", "https://files.feed-the-beast.com/blob/a").trusted());
        assert!(file("", "a", "https://edge.forgecdn.net/files/1/2/a.jar").trusted());
        assert!(!file("", "a", "http://files.feed-the-beast.com/blob/a").trusted());
        assert!(!file("", "a", "https://evil.example/files.feed-the-beast.com/a").trusted());
        assert!(!file("", "a", "https://files.feed-the-beast.com.evil.example/a").trusted());
    }

    #[test]
    fn curseforge_ids_can_be_numbers_or_text() {
        let file: CurseForgeFile =
            serde_json::from_str(r#"{"project":"250363","file":3326041}"#).unwrap();
        assert_eq!((file.project, file.file), (250363, 3326041));
    }

    #[test]
    fn packs_too_old_for_any_installer_are_hidden() {
        assert!(!installable("1.4.7") && !installable("1.5.2"));
        assert!(
            installable("1.6.4")
                && installable("1.7.10")
                && installable("1.21.1")
                && installable("26.1")
        );
    }

    #[test]
    fn reads_the_game_from_targets() {
        let targets = [
            Target {
                name: "minecraft".into(),
                version: "1.18.2".into(),
            },
            Target {
                name: "forge".into(),
                version: "40.2.34".into(),
            },
            Target {
                name: "java".into(),
                version: "17.0.2+8".into(),
            },
        ];
        let (minecraft, loader) = game_of(&targets).unwrap();
        assert_eq!(minecraft, "1.18.2");
        assert_eq!(loader.kind, LoaderKind::Forge);
        assert_eq!(loader.version.as_deref(), Some("40.2.34"));
    }

    #[test]
    fn prefers_the_newest_full_release() {
        let version = |id, kind: &str| PackVersion {
            id,
            name: id.to_string(),
            kind: kind.into(),
            updated: 0,
            targets: Vec::new(),
        };
        let mut pack = Pack {
            id: 1,
            name: "P".into(),
            slug: String::new(),
            synopsis: String::new(),
            description: String::new(),
            installs: 0,
            released: 0,
            updated: 0,
            authors: Vec::new(),
            art: Vec::new(),
            links: Vec::new(),
            versions: vec![
                version(1, "release"),
                version(2, "release"),
                version(3, "beta"),
            ],
        };
        assert_eq!(newest(&pack).unwrap().id, 2);
        pack.versions = vec![version(3, "beta"), version(4, "alpha")];
        assert_eq!(newest(&pack).unwrap().id, 4);
    }
}

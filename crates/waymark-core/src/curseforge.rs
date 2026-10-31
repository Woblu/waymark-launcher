//! The CurseForge API. Authors can opt out of third-party downloads, and we
//! respect that: when a file has no download URL, the player is sent to
//! CurseForge. Reconstructing the CDN link would go around the author's choice.

use serde::Deserialize;
use serde_json::json;

use crate::brand::CURSEFORGE_API_KEY;
use crate::content::{ContentHit, ContentKind, ContentPage, ContentSource, SearchQuery};
use crate::error::{Error, Result};
use crate::lock::LoaderKind;
use crate::net::Net;

const API: &str = "https://api.curseforge.com/v1";
const MINECRAFT: u32 = 432;
const SORT_BY_POPULARITY: &str = "2";
const REQUIRED_DEPENDENCY: u32 = 3;
const SHA1: u32 = 1;

#[derive(Deserialize)]
struct Page<T> {
    data: Vec<T>,
    #[serde(default)]
    pagination: Option<Pagination>,
}

#[derive(Deserialize)]
struct One<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    total_count: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub logo: Option<Image>,
    #[serde(default)]
    pub screenshots: Vec<Image>,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub links: Option<Links>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Author {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub url: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Category {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(default)]
    pub website_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub id: u64,
    pub mod_id: u64,
    pub display_name: String,
    pub file_name: String,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub file_length: u64,
    #[serde(default)]
    pub hashes: Vec<Hash>,
    #[serde(default)]
    pub dependencies: Vec<FileDependency>,
    #[serde(default)]
    pub release_type: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Hash {
    pub value: String,
    pub algo: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDependency {
    pub mod_id: u64,
    pub relation_type: u32,
}

impl File {
    pub fn sha1(&self) -> Option<String> {
        self.hashes.iter().find(|h| h.algo == SHA1).map(|h| h.value.clone())
    }

    pub fn required_mods(&self) -> Vec<u64> {
        self.dependencies
            .iter()
            .filter(|d| d.relation_type == REQUIRED_DEPENDENCY)
            .map(|d| d.mod_id)
            .collect()
    }
}

fn key() -> Result<&'static str> {
    CURSEFORGE_API_KEY.ok_or_else(|| Error::Other("this build has no CurseForge access".into()))
}

pub fn available() -> bool {
    CURSEFORGE_API_KEY.is_some()
}

fn class_id(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "6",
        ContentKind::Modpack => "4471",
        ContentKind::ResourcePack => "12",
        ContentKind::Shader => "6552",
    }
}

fn loader_type(loader: LoaderKind) -> Option<&'static str> {
    match loader {
        LoaderKind::Forge => Some("1"),
        LoaderKind::Fabric => Some("4"),
        LoaderKind::Quilt => Some("5"),
        LoaderKind::NeoForge => Some("6"),
        LoaderKind::Vanilla => None,
    }
}

pub async fn search(net: &Net, query: &SearchQuery) -> Result<ContentPage> {
    let game = MINECRAFT.to_string();
    let index = query.offset.to_string();
    let size = query.limit.clamp(1, 50).to_string();
    let mut parameters = vec![
        ("gameId", game.as_str()),
        ("classId", class_id(query.kind)),
        ("searchFilter", query.text.as_str()),
        ("sortField", SORT_BY_POPULARITY),
        ("sortOrder", "desc"),
        ("index", index.as_str()),
        ("pageSize", size.as_str()),
    ];
    if let Some(minecraft) = &query.minecraft {
        parameters.push(("gameVersion", minecraft));
    }
    if matches!(query.kind, ContentKind::Mod | ContentKind::Modpack)
        && let Some(loader) = query.loader.and_then(loader_type) {
            parameters.push(("modLoaderType", loader));
        }
    let page: Page<Mod> = net
        .client()
        .get(format!("{API}/mods/search"))
        .header("x-api-key", key()?)
        .query(&parameters)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(ContentPage {
        total: page.pagination.map_or(0, |p| p.total_count),
        hits: page
            .data
            .into_iter()
            .map(|project| ContentHit {
                source: ContentSource::CurseForge,
                id: project.id.to_string(),
                slug: project.slug,
                title: project.name,
                summary: project.summary,
                author: project.authors.first().map(|a| a.name.clone()).unwrap_or_default(),
                downloads: project.download_count,
                icon: project.logo.as_ref().map(|l| l.thumbnail_url.clone().unwrap_or_else(|| l.url.clone())),
                banner: project.screenshots.first().map(|s| s.url.clone()),
                kind: query.kind,
                categories: project.categories.into_iter().map(|c| c.name).take(3).collect(),
            })
            .collect(),
    })
}

pub async fn project(net: &Net, id: u64) -> Result<Mod> {
    let response: One<Mod> = net
        .client()
        .get(format!("{API}/mods/{id}"))
        .header("x-api-key", key()?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response.data)
}

/// Files of a project that fit an instance, newest first.
pub async fn files(net: &Net, id: u64, minecraft: &str, loader: LoaderKind, kind: ContentKind) -> Result<Vec<File>> {
    let mut parameters = vec![("gameVersion", minecraft.to_string()), ("pageSize", "50".to_string())];
    if matches!(kind, ContentKind::Mod | ContentKind::Modpack)
        && let Some(loader) = loader_type(loader) {
            parameters.push(("modLoaderType", loader.to_string()));
        }
    let page: Page<File> = net
        .client()
        .get(format!("{API}/mods/{id}/files"))
        .header("x-api-key", key()?)
        .query(&parameters)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(page.data)
}

pub async fn files_by_id(net: &Net, ids: &[u64]) -> Result<Vec<File>> {
    let page: Page<File> = net
        .client()
        .post(format!("{API}/mods/files"))
        .header("x-api-key", key()?)
        .json(&json!({ "fileIds": ids }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(page.data)
}

pub async fn mods_by_id(net: &Net, ids: &[u64]) -> Result<Vec<Mod>> {
    let page: Page<Mod> = net
        .client()
        .post(format!("{API}/mods"))
        .header("x-api-key", key()?)
        .json(&json!({ "modIds": ids }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(page.data)
}

/// The newest stable file, or the newest of anything. Release type 1 is a
/// full release; 2 and 3 are beta and alpha.
pub fn pick(files: &[File]) -> Option<&File> {
    files.iter().find(|f| f.release_type == 1).or(files.first())
}

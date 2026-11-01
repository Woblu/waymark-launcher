//! The CurseForge API. Authors can opt out of third-party downloads, and we
//! respect that: when a file has no download URL, the player is sent to
//! CurseForge. Reconstructing the CDN link would go around the author's choice.

use serde::Deserialize;
use serde_json::json;

use crate::brand::CURSEFORGE_API_KEY;
use crate::content::{
    ContentHit, ContentKind, ContentPage, ContentSource, SearchCategory, SearchQuery, Sort,
};
use crate::error::{Error, Result};
use crate::lock::LoaderKind;
use crate::net::Net;

const API: &str = "https://api.curseforge.com/v1";
const MINECRAFT: u32 = 432;
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
    #[serde(default)]
    pub class_id: Option<u32>,
    #[serde(default)]
    pub license_type: Option<String>,
    #[serde(default)]
    pub date_created: Option<String>,
    #[serde(default)]
    pub date_modified: Option<String>,
    #[serde(default)]
    pub date_released: Option<String>,
    #[serde(default)]
    pub latest_files_indexes: Vec<FileIndex>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub url: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndex {
    pub game_version: String,
    #[serde(default)]
    pub mod_loader: Option<u32>,
    #[serde(default)]
    pub file_id: u64,
    #[serde(default)]
    pub release_type: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Category {
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub wiki_url: Option<String>,
    #[serde(default)]
    pub issues_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
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
    #[serde(default)]
    pub file_date: String,
    #[serde(default)]
    pub download_count: u64,
    /// Minecraft versions mixed in with loaders and sides, like `Fabric` and `Client`.
    #[serde(default)]
    pub game_versions: Vec<String>,
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
        self.hashes
            .iter()
            .find(|h| h.algo == SHA1)
            .map(|h| h.value.clone())
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
        ContentKind::DataPack => "6945",
    }
}

pub fn kind_of(class: Option<u32>) -> ContentKind {
    match class {
        Some(4471) => ContentKind::Modpack,
        Some(12) => ContentKind::ResourcePack,
        Some(6552) => ContentKind::Shader,
        Some(6945) => ContentKind::DataPack,
        _ => ContentKind::Mod,
    }
}

pub fn channel(release_type: u32) -> &'static str {
    match release_type {
        2 => "beta",
        3 => "alpha",
        _ => "release",
    }
}

/// The API's numbers for loaders, as the names the rest of the app uses.
pub fn loader_name(loader: Option<u32>) -> Option<&'static str> {
    match loader? {
        1 => Some("forge"),
        4 => Some("fabric"),
        5 => Some("quilt"),
        6 => Some("neoforge"),
        _ => None,
    }
}

pub fn loader_type_of(name: &str) -> Option<LoaderKind> {
    match name {
        "forge" => Some(LoaderKind::Forge),
        "fabric" => Some(LoaderKind::Fabric),
        "quilt" => Some(LoaderKind::Quilt),
        "neoforge" => Some(LoaderKind::NeoForge),
        _ => None,
    }
}

/// Separates a file's `gameVersions` into loaders and Minecraft versions,
/// dropping sides and Java versions.
pub fn split_game_versions(tags: &[String]) -> (Vec<String>, Vec<String>) {
    let mut loaders = Vec::new();
    let mut minecraft = Vec::new();
    for tag in tags {
        let lower = tag.to_ascii_lowercase();
        if loader_type_of(&lower).is_some() {
            loaders.push(lower);
        } else if tag.starts_with(|c: char| c.is_ascii_digit()) {
            minecraft.push(tag.clone());
        }
    }
    (loaders, minecraft)
}

fn loader_type(loader: LoaderKind) -> Option<&'static str> {
    match loader {
        LoaderKind::Forge => Some("1"),
        LoaderKind::Fabric => Some("4"),
        LoaderKind::Quilt => Some("5"),
        LoaderKind::NeoForge => Some("6"),
        _ => None,
    }
}

/// The API's numbers for its sort orders. There's no follower count here, so
/// followers falls back to popularity.
fn sort_field(sort: Sort) -> &'static str {
    match sort {
        Sort::Relevance | Sort::Follows => "2",
        Sort::Updated => "3",
        Sort::Downloads => "6",
        Sort::Newest => "11",
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CategoryRow {
    id: u64,
    name: String,
    #[serde(default)]
    is_class: Option<bool>,
}

pub async fn categories(net: &Net, kind: ContentKind) -> Result<Vec<SearchCategory>> {
    let page: Page<CategoryRow> = net
        .client()
        .get(format!("{API}/categories"))
        .header("x-api-key", key()?)
        .query(&[
            ("gameId", MINECRAFT.to_string()),
            ("classId", class_id(kind).to_string()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut rows: Vec<CategoryRow> = page
        .data
        .into_iter()
        .filter(|row| row.is_class != Some(true))
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows
        .into_iter()
        .map(|row| SearchCategory {
            id: row.id.to_string(),
            name: row.name,
            group: "categories".to_string(),
        })
        .collect())
}

pub async fn search(net: &Net, query: &SearchQuery) -> Result<ContentPage> {
    let game = MINECRAFT.to_string();
    let index = query.offset.to_string();
    let size = query.limit.clamp(1, 50).to_string();
    let chosen = format!("[{}]", query.categories.join(","));
    let mut parameters = vec![
        ("gameId", game.as_str()),
        ("classId", class_id(query.kind)),
        ("searchFilter", query.text.as_str()),
        ("sortField", sort_field(query.sort)),
        ("sortOrder", "desc"),
        ("index", index.as_str()),
        ("pageSize", size.as_str()),
    ];
    if let Some(minecraft) = &query.minecraft {
        parameters.push(("gameVersion", minecraft));
    }
    if !query.categories.is_empty() {
        parameters.push(("categoryIds", chosen.as_str()));
    }
    if matches!(query.kind, ContentKind::Mod | ContentKind::Modpack)
        && let Some(loader) = query.loader.and_then(loader_type)
    {
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
                author: project
                    .authors
                    .first()
                    .map(|a| a.name.clone())
                    .unwrap_or_default(),
                downloads: project.download_count,
                icon: project
                    .logo
                    .as_ref()
                    .map(|l| l.thumbnail_url.clone().unwrap_or_else(|| l.url.clone())),
                banner: project.screenshots.first().map(|s| s.url.clone()),
                kind: query.kind,
                categories: project
                    .categories
                    .into_iter()
                    .map(|c| c.name)
                    .take(3)
                    .collect(),
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

/// The long description, as HTML.
pub async fn description(net: &Net, id: u64) -> Result<String> {
    let response: One<String> = net
        .client()
        .get(format!("{API}/mods/{id}/description"))
        .header("x-api-key", key()?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response.data)
}

/// One file's changelog, as HTML.
pub async fn file_changelog(net: &Net, id: u64, file: u64) -> Result<String> {
    let response: One<String> = net
        .client()
        .get(format!("{API}/mods/{id}/files/{file}/changelog"))
        .header("x-api-key", key()?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(response.data)
}

/// A page of a project's files, newest first, with the total that match.
pub async fn files_page(
    net: &Net,
    id: u64,
    minecraft: Option<&str>,
    loader: Option<LoaderKind>,
    offset: u32,
) -> Result<(Vec<File>, u64)> {
    let mut parameters = vec![
        ("index", offset.to_string()),
        ("pageSize", "50".to_string()),
    ];
    if let Some(minecraft) = minecraft {
        parameters.push(("gameVersion", minecraft.to_string()));
    }
    if let Some(loader) = loader.and_then(loader_type) {
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
    let total = page
        .pagination
        .map_or(page.data.len() as u64, |p| p.total_count);
    Ok((page.data, total))
}

/// Files of a project that fit an instance, newest first.
pub async fn files(
    net: &Net,
    id: u64,
    minecraft: &str,
    loader: LoaderKind,
    kind: ContentKind,
) -> Result<Vec<File>> {
    let mut parameters = vec![
        ("gameVersion", minecraft.to_string()),
        ("pageSize", "50".to_string()),
    ];
    if matches!(kind, ContentKind::Mod | ContentKind::Modpack)
        && let Some(loader) = loader_type(loader)
    {
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

/// The newest file for an instance, going by the project's own index. There's
/// no update endpoint, so this is how one request can check many mods at once.
pub fn latest_file(
    project: &Mod,
    minecraft: &str,
    loader: LoaderKind,
    kind: ContentKind,
) -> Option<u64> {
    let wanted = loader_type(loader).and_then(|id| id.parse::<u32>().ok());
    let fits = |index: &&FileIndex| {
        index.game_version == minecraft && (kind != ContentKind::Mod || index.mod_loader == wanted)
    };
    let matching: Vec<&FileIndex> = project.latest_files_indexes.iter().filter(fits).collect();
    let newest = |full_only: bool| {
        matching
            .iter()
            .filter(|index| !full_only || index.release_type == 1)
            .map(|index| index.file_id)
            .max()
    };
    newest(true).or_else(|| newest(false))
}

/// The newest stable file, or the newest of anything. Release type 1 is a
/// full release; 2 and 3 are beta and alpha.
pub fn pick(files: &[File]) -> Option<&File> {
    files.iter().find(|f| f.release_type == 1).or(files.first())
}

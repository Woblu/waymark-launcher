//! The Modrinth API.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::content::{
    ContentHit, ContentKind, ContentPage, ContentSource, SearchCategory, SearchQuery, Sort,
};
use crate::error::Result;
use crate::lock::LoaderKind;
use crate::net::Net;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<Hit>,
    total_hits: u64,
}

#[derive(Deserialize)]
struct Hit {
    project_id: String,
    slug: Option<String>,
    title: String,
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    downloads: u64,
    icon_url: Option<String>,
    featured_gallery: Option<String>,
    #[serde(default)]
    display_categories: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub title: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
}

/// Everything the store's page says about a project.
#[derive(Clone, Debug, Deserialize)]
pub struct ProjectDetails {
    pub id: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub project_type: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub additional_categories: Vec<String>,
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
    #[serde(default)]
    pub license: Option<License>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub followers: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub gallery: Vec<GalleryImage>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub published: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub issues_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub wiki_url: Option<String>,
    #[serde(default)]
    pub discord_url: Option<String>,
    #[serde(default)]
    pub donation_urls: Option<Vec<Donation>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct License {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GalleryImage {
    pub url: String,
    #[serde(default)]
    pub raw_url: Option<String>,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub ordering: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Donation {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub platform: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Member {
    #[serde(default)]
    pub role: String,
    pub user: User,
}

#[derive(Clone, Debug, Deserialize)]
pub struct User {
    pub username: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

/// A row in a project's version list.
#[derive(Clone, Debug, Deserialize)]
pub struct VersionListing {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub version_type: String,
    pub date_published: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
}

#[derive(Deserialize)]
struct Changelog {
    #[serde(default)]
    changelog: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    #[serde(default)]
    pub version_type: String,
    pub date_published: String,
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub game_versions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    pub size: u64,
    pub hashes: FileHashes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileHashes {
    pub sha512: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub dependency_type: String,
}

impl Version {
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.primary).or(self.files.first())
    }

    /// Project ids this version can't run without.
    pub fn required_projects(&self) -> Vec<String> {
        self.dependencies
            .iter()
            .filter(|d| d.dependency_type == "required")
            .filter_map(|d| d.project_id.clone())
            .collect()
    }
}

/// Quilt runs Fabric mods, so its filter includes both.
fn loader_filter(loader: LoaderKind) -> Vec<&'static str> {
    match loader {
        LoaderKind::Vanilla => Vec::new(),
        LoaderKind::Quilt => vec!["quilt", "fabric"],
        other => vec![other.slug()],
    }
}

fn kind_facet(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "mod",
        ContentKind::Modpack => "modpack",
        ContentKind::ResourcePack => "resourcepack",
        ContentKind::Shader => "shader",
        ContentKind::DataPack => "datapack",
    }
}

/// The loader names a kind of content is published under, which for packs
/// have nothing to do with the instance's loader.
fn version_loaders(kind: ContentKind, loader: LoaderKind) -> Vec<&'static str> {
    match kind {
        ContentKind::Mod | ContentKind::Modpack => loader_filter(loader),
        ContentKind::ResourcePack => vec!["minecraft"],
        ContentKind::Shader => vec!["iris", "optifine", "canvas", "vanilla"],
        ContentKind::DataPack => vec!["datapack"],
    }
}

/// Search has nothing to rank by until there's text, so an empty search shows what's popular.
fn sort_index(sort: Sort, text: &str) -> &'static str {
    match sort {
        Sort::Relevance if text.is_empty() => "downloads",
        Sort::Relevance => "relevance",
        Sort::Downloads => "downloads",
        Sort::Follows => "follows",
        Sort::Newest => "newest",
        Sort::Updated => "updated",
    }
}

#[derive(Deserialize)]
struct CategoryTag {
    name: String,
    project_type: String,
    header: String,
}

/// Data packs are filed under mods here, so they share mod categories.
pub async fn categories(net: &Net, kind: ContentKind) -> Result<Vec<SearchCategory>> {
    let wanted = if kind == ContentKind::DataPack {
        "mod"
    } else {
        kind_facet(kind)
    };
    let tags: Vec<CategoryTag> = net.json(&format!("{API}/tag/category")).await?;
    Ok(tags
        .into_iter()
        .filter(|tag| tag.project_type == wanted)
        .map(|tag| SearchCategory {
            id: tag.name.clone(),
            name: tag.name,
            group: tag.header,
        })
        .collect())
}

pub async fn search(net: &Net, query: &SearchQuery) -> Result<ContentPage> {
    let mut facets = vec![json!([format!("project_type:{}", kind_facet(query.kind))])];
    if let Some(minecraft) = &query.minecraft {
        facets.push(json!([format!("versions:{minecraft}")]));
    }
    for category in &query.categories {
        facets.push(json!([format!("categories:{category}")]));
    }
    if matches!(query.kind, ContentKind::Mod | ContentKind::Modpack)
        && let Some(loader) = query.loader
    {
        let names: Vec<String> = loader_filter(loader)
            .iter()
            .map(|l| format!("categories:{l}"))
            .collect();
        if !names.is_empty() {
            facets.push(json!(names));
        }
    }
    let response: SearchResponse = net
        .client()
        .get(format!("{API}/search"))
        .query(&[
            ("query", query.text.as_str()),
            ("facets", &json!(facets).to_string()),
            ("index", sort_index(query.sort, &query.text)),
            ("offset", &query.offset.to_string()),
            ("limit", &query.limit.clamp(1, 100).to_string()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(ContentPage {
        total: response.total_hits,
        hits: response
            .hits
            .into_iter()
            .map(|hit| ContentHit {
                source: ContentSource::Modrinth,
                id: hit.project_id,
                slug: hit.slug.unwrap_or_default(),
                title: hit.title,
                summary: hit.description,
                author: hit.author,
                downloads: hit.downloads,
                icon: hit.icon_url,
                banner: hit.featured_gallery,
                kind: query.kind,
                categories: hit.display_categories,
            })
            .collect(),
    })
}

pub async fn project(net: &Net, id: &str) -> Result<Project> {
    net.json(&format!("{API}/project/{id}")).await
}

pub async fn details(net: &Net, id: &str) -> Result<ProjectDetails> {
    net.json(&format!("{API}/project/{id}")).await
}

pub async fn members(net: &Net, id: &str) -> Result<Vec<Member>> {
    net.json(&format!("{API}/project/{id}/members")).await
}

/// Every version of a project, optionally narrowed down. Changelogs are left
/// out because they make long lists several times bigger.
pub async fn version_listing(
    net: &Net,
    id: &str,
    minecraft: Option<&str>,
    loader: Option<&str>,
) -> Result<Vec<VersionListing>> {
    let mut parameters = vec![("include_changelog", "false".to_string())];
    if let Some(minecraft) = minecraft {
        parameters.push(("game_versions", json!([minecraft]).to_string()));
    }
    if let Some(loader) = loader {
        parameters.push(("loaders", json!([loader]).to_string()));
    }
    Ok(net
        .client()
        .get(format!("{API}/project/{id}/version"))
        .query(&parameters)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub async fn changelog(net: &Net, version: &str) -> Result<String> {
    let response: Changelog = net.json(&format!("{API}/version/{version}")).await?;
    Ok(response.changelog.unwrap_or_default())
}

/// Versions of a project that fit an instance, newest first.
pub async fn versions(
    net: &Net,
    id: &str,
    minecraft: &str,
    loader: LoaderKind,
    kind: ContentKind,
) -> Result<Vec<Version>> {
    let mut parameters = vec![("game_versions", json!([minecraft]).to_string())];
    let loaders = version_loaders(kind, loader);
    if !loaders.is_empty() {
        parameters.push(("loaders", json!(loaders).to_string()));
    }
    Ok(net
        .client()
        .get(format!("{API}/project/{id}/version"))
        .query(&parameters)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Asks for the newest version of many files in one request, keyed by sha512.
pub async fn latest_for_files(
    net: &Net,
    hashes: &[String],
    minecraft: &str,
    loader: LoaderKind,
    kind: ContentKind,
) -> Result<std::collections::HashMap<String, Version>> {
    Ok(net
        .client()
        .post(format!("{API}/version_files/update"))
        .json(&json!({
            "hashes": hashes,
            "algorithm": "sha512",
            "loaders": version_loaders(kind, loader),
            "game_versions": [minecraft],
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Identifies files by hash. Files Modrinth doesn't know are left out.
pub async fn versions_for_files(
    net: &Net,
    hashes: &[String],
) -> Result<std::collections::HashMap<String, Version>> {
    identify(net, hashes, "sha512").await
}

/// The same lookup by SHA-1, which is the hash CurseForge gives for its files.
pub async fn versions_for_sha1(
    net: &Net,
    hashes: &[String],
) -> Result<std::collections::HashMap<String, Version>> {
    identify(net, hashes, "sha1").await
}

async fn identify(
    net: &Net,
    hashes: &[String],
    algorithm: &str,
) -> Result<std::collections::HashMap<String, Version>> {
    Ok(net
        .client()
        .post(format!("{API}/version_files"))
        .json(&json!({ "hashes": hashes, "algorithm": algorithm }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Many projects in one request.
pub async fn projects(net: &Net, ids: &[String]) -> Result<Vec<Project>> {
    Ok(net
        .client()
        .get(format!("{API}/projects"))
        .query(&[("ids", json!(ids).to_string())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Many versions in one request.
pub async fn versions_by_id(net: &Net, ids: &[String]) -> Result<Vec<Version>> {
    Ok(net
        .client()
        .get(format!("{API}/versions"))
        .query(&[("ids", json!(ids).to_string())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// The best version to install: the newest full release, or failing that the
/// newest of anything.
pub fn pick(versions: &[Version]) -> Option<&Version> {
    versions
        .iter()
        .find(|v| v.version_type == "release")
        .or(versions.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: &str, kind: &str) -> Version {
        Version {
            id: id.into(),
            project_id: "p".into(),
            version_number: id.into(),
            version_type: kind.into(),
            date_published: String::new(),
            files: Vec::new(),
            dependencies: Vec::new(),
            game_versions: Vec::new(),
        }
    }

    #[test]
    fn prefers_the_newest_full_release() {
        let versions = [
            version("3", "beta"),
            version("2", "release"),
            version("1", "release"),
        ];
        assert_eq!(pick(&versions).unwrap().id, "2");
    }

    #[test]
    fn falls_back_to_a_prerelease_when_there_is_nothing_else() {
        let versions = [version("3", "alpha"), version("2", "beta")];
        assert_eq!(pick(&versions).unwrap().id, "3");
    }

    #[test]
    fn only_required_dependencies_are_pulled_in() {
        let mut v = version("1", "release");
        v.dependencies = vec![
            Dependency {
                project_id: Some("needed".into()),
                version_id: None,
                dependency_type: "required".into(),
            },
            Dependency {
                project_id: Some("nice".into()),
                version_id: None,
                dependency_type: "optional".into(),
            },
            Dependency {
                project_id: Some("bad".into()),
                version_id: None,
                dependency_type: "incompatible".into(),
            },
        ];
        assert_eq!(v.required_projects(), ["needed"]);
    }

    #[test]
    fn an_empty_search_shows_what_is_popular() {
        assert_eq!(sort_index(Sort::Relevance, ""), "downloads");
        assert_eq!(sort_index(Sort::Relevance, "sodium"), "relevance");
        assert_eq!(sort_index(Sort::Newest, ""), "newest");
    }

    #[test]
    fn packs_are_published_under_their_own_loaders() {
        assert_eq!(
            version_loaders(ContentKind::Mod, LoaderKind::Forge),
            ["forge"]
        );
        assert_eq!(
            version_loaders(ContentKind::ResourcePack, LoaderKind::Forge),
            ["minecraft"]
        );
        assert_eq!(
            version_loaders(ContentKind::DataPack, LoaderKind::Forge),
            ["datapack"]
        );
        assert!(version_loaders(ContentKind::Shader, LoaderKind::Forge).contains(&"iris"));
    }

    #[test]
    fn quilt_can_use_fabric_mods() {
        assert_eq!(loader_filter(LoaderKind::Quilt), ["quilt", "fabric"]);
        assert_eq!(loader_filter(LoaderKind::NeoForge), ["neoforge"]);
        assert!(loader_filter(LoaderKind::Vanilla).is_empty());
    }
}

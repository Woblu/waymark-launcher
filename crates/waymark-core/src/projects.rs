//! What a project page shows. Modrinth and CurseForge describe projects very
//! differently, so both are turned into one shape here and the UI only knows
//! that shape. Bodies arrive as the store wrote them, and the UI sanitizes them,
//! because they're written by strangers.

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::content::{ContentKind, ContentSource};
use crate::curseforge;
use crate::error::{Error, Result};
use crate::identify::same_mod;
use crate::lock::{LoaderKind, Source, content_folder};
use crate::modinfo::{self, ModInfo};
use crate::modrinth;

const MODRINTH_SITE: &str = "https://modrinth.com";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyFormat {
    Markdown,
    Html,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Shot {
    pub url: String,
    /// The original, for viewing large. Listing pages use the smaller `url`.
    pub full: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Creator {
    pub name: String,
    pub role: Option<String>,
    pub avatar: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPage {
    pub source: ContentSource,
    pub id: String,
    pub slug: String,
    pub kind: ContentKind,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub format: BodyFormat,
    pub icon: Option<String>,
    pub downloads: u64,
    pub followers: u64,
    pub published: Option<String>,
    pub updated: Option<String>,
    pub categories: Vec<String>,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub client: Option<String>,
    pub server: Option<String>,
    pub license: Option<Link>,
    pub links: Vec<Link>,
    pub gallery: Vec<Shot>,
    pub creators: Vec<Creator>,
    /// The project's page on its store's website.
    pub page: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionRow {
    pub id: String,
    pub name: String,
    pub number: String,
    pub channel: String,
    pub published: String,
    pub downloads: u64,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub file: String,
    pub size: u64,
    /// False when a CurseForge author has blocked downloads through other apps.
    pub available: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct VersionList {
    pub versions: Vec<VersionRow>,
    pub total: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFilter {
    #[serde(default)]
    pub minecraft: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub offset: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Needed {
    pub source: ContentSource,
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
}

/// What would be added to an instance, and what it would bring along.
#[derive(Clone, Debug, Serialize)]
pub struct Fit {
    pub version: String,
    pub requires: Vec<Needed>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LocalMod {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub links: Vec<Link>,
    pub file: String,
    pub size: u64,
    pub icon: Option<String>,
}

/// Where a mod in an instance has its page.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InstalledPage {
    Store {
        source: ContentSource,
        project: String,
    },
    /// A file from elsewhere that shares its mod id with a store project, like
    /// a backport. The page is about the project, not this exact file.
    Namesake {
        source: ContentSource,
        project: String,
        file: String,
        version: Option<String>,
    },
    Local(LocalMod),
}

fn web_link(label: &str, url: Option<String>) -> Option<Link> {
    url.filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(|url| Link {
            label: label.into(),
            url,
        })
}

fn filled(text: String) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Custom licenses arrive as ids like `LicenseRef-All-Rights-Reserved`.
fn license_label(id: &str) -> String {
    match id.strip_prefix("LicenseRef-") {
        Some(custom) => custom.replace('-', " "),
        None => id.to_string(),
    }
}

/// Modrinth files data packs under mods, and only their loader tells them apart.
fn modrinth_kind(project_type: &str, loaders: &[String]) -> ContentKind {
    match project_type {
        "modpack" => ContentKind::Modpack,
        "resourcepack" => ContentKind::ResourcePack,
        "shader" => ContentKind::Shader,
        "datapack" => ContentKind::DataPack,
        _ if !loaders.is_empty() && loaders.iter().all(|loader| loader == "datapack") => {
            ContentKind::DataPack
        }
        _ => ContentKind::Mod,
    }
}

fn from_modrinth(project: modrinth::ProjectDetails, members: Vec<modrinth::Member>) -> ProjectPage {
    let slug = project.slug.clone().unwrap_or_else(|| project.id.clone());
    let page = format!("{MODRINTH_SITE}/{}/{slug}", project.project_type);

    let mut gallery = project.gallery;
    gallery.sort_by_key(|image| (!image.featured, image.ordering));
    let gallery = gallery
        .into_iter()
        .map(|image| Shot {
            full: image.raw_url.clone().unwrap_or_else(|| image.url.clone()),
            url: image.url,
            title: image.title.and_then(filled),
            description: image.description.and_then(filled),
        })
        .collect();

    let mut links: Vec<Link> = [
        web_link("Issues", project.issues_url),
        web_link("Source", project.source_url),
        web_link("Wiki", project.wiki_url),
        web_link("Discord", project.discord_url),
    ]
    .into_iter()
    .flatten()
    .collect();
    links.extend(
        project
            .donation_urls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|donation| {
                let label = if donation.id == "other" {
                    "Donate".to_string()
                } else {
                    donation.platform
                };
                web_link(&label, Some(donation.url))
            }),
    );

    let license = project.license.and_then(|license| {
        let label = filled(license.name).or_else(|| filled(license_label(&license.id)))?;
        Some(Link {
            label,
            url: license.url.unwrap_or_default(),
        })
    });

    ProjectPage {
        source: ContentSource::Modrinth,
        id: project.id,
        slug,
        kind: modrinth_kind(&project.project_type, &project.loaders),
        title: project.title,
        summary: project.description,
        body: project.body,
        format: BodyFormat::Markdown,
        icon: project.icon_url,
        downloads: project.downloads,
        followers: project.followers,
        published: project.published,
        updated: project.updated,
        categories: project
            .categories
            .into_iter()
            .chain(project.additional_categories)
            .collect(),
        loaders: project.loaders,
        game_versions: project.game_versions,
        client: project.client_side,
        server: project.server_side,
        license,
        links,
        gallery,
        creators: members
            .into_iter()
            .map(|member| Creator {
                url: Some(format!("{MODRINTH_SITE}/user/{}", member.user.username)),
                name: member.user.username,
                role: filled(member.role),
                avatar: member.user.avatar_url,
            })
            .collect(),
        page,
    }
}

fn from_curseforge(project: curseforge::Mod, body: String) -> ProjectPage {
    let site = project.links.clone().unwrap_or_default();
    let page = site.website_url.clone().unwrap_or_default();

    let mut game_versions = Vec::new();
    let mut loaders = Vec::new();
    for file in &project.latest_files_indexes {
        if !game_versions.contains(&file.game_version) {
            game_versions.push(file.game_version.clone());
        }
        if let Some(loader) = curseforge::loader_name(file.mod_loader)
            && !loaders.iter().any(|known| known == loader)
        {
            loaders.push(loader.to_string());
        }
    }

    ProjectPage {
        source: ContentSource::CurseForge,
        id: project.id.to_string(),
        slug: project.slug,
        kind: curseforge::kind_of(project.class_id),
        title: project.name,
        summary: project.summary,
        body,
        format: BodyFormat::Html,
        icon: project.logo.map(|logo| logo.url),
        downloads: project.download_count,
        followers: 0,
        published: project.date_created,
        updated: project.date_released.or(project.date_modified),
        categories: project.categories.into_iter().map(|c| c.name).collect(),
        loaders,
        game_versions,
        client: None,
        server: None,
        license: project.license_type.and_then(filled).map(|label| Link {
            label,
            url: String::new(),
        }),
        links: [
            web_link("Issues", site.issues_url),
            web_link("Source", site.source_url),
            web_link("Wiki", site.wiki_url),
        ]
        .into_iter()
        .flatten()
        .collect(),
        gallery: project
            .screenshots
            .into_iter()
            .map(|shot| Shot {
                full: shot.url.clone(),
                url: shot.thumbnail_url.unwrap_or(shot.url),
                title: shot.title.and_then(filled),
                description: shot.description.and_then(filled),
            })
            .collect(),
        creators: project
            .authors
            .into_iter()
            .map(|author| Creator {
                name: author.name,
                role: None,
                avatar: author.avatar_url,
                url: author.url,
            })
            .collect(),
        page,
    }
}

fn modrinth_row(version: modrinth::VersionListing) -> VersionRow {
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or(version.files.first());
    VersionRow {
        file: file.map(|f| f.filename.clone()).unwrap_or_default(),
        size: file.map_or(0, |f| f.size),
        id: version.id,
        name: version.name,
        number: version.version_number,
        channel: version.version_type,
        published: version.date_published,
        downloads: version.downloads,
        loaders: version.loaders,
        game_versions: version.game_versions,
        available: true,
    }
}

fn curseforge_row(file: curseforge::File) -> VersionRow {
    let (loaders, game_versions) = curseforge::split_game_versions(&file.game_versions);
    VersionRow {
        id: file.id.to_string(),
        number: file.display_name.clone(),
        name: file.display_name,
        channel: curseforge::channel(file.release_type).to_string(),
        published: file.file_date,
        downloads: file.download_count,
        loaders,
        game_versions,
        available: file.download_url.is_some(),
        file: file.file_name,
        size: file.file_length,
    }
}

fn parse_id(project: &str) -> Result<u64> {
    project
        .parse()
        .map_err(|_| Error::NotFound(format!("CurseForge project {project}")))
}

/// Mod ids come from inside jars, so only plain ones go into a URL.
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn local_mod(
    name: String,
    file: String,
    size: u64,
    version: Option<String>,
    icon: Option<String>,
    info: ModInfo,
) -> LocalMod {
    LocalMod {
        name,
        version: version.or(info.version),
        description: info.description.and_then(filled),
        authors: info.authors,
        license: info.license,
        links: [
            web_link("Website", info.homepage),
            web_link("Source", info.sources),
            web_link("Issues", info.issues),
        ]
        .into_iter()
        .flatten()
        .collect(),
        file,
        size,
        icon,
    }
}

impl Core {
    pub async fn project_page(&self, source: ContentSource, project: &str) -> Result<ProjectPage> {
        match source {
            ContentSource::Ftb => Ok(crate::ftb::page(
                crate::ftb::pack(&self.net, project).await?,
            )),
            ContentSource::AtLauncher => {
                Ok(crate::atlauncher::page(&self.atl_pack(project).await?))
            }
            ContentSource::Modrinth => {
                let (details, members) = tokio::join!(
                    modrinth::details(&self.net, project),
                    modrinth::members(&self.net, project)
                );
                Ok(from_modrinth(details?, members.unwrap_or_default()))
            }
            ContentSource::CurseForge => {
                let id = parse_id(project)?;
                let (details, body) = tokio::join!(
                    curseforge::project(&self.net, id),
                    curseforge::description(&self.net, id)
                );
                Ok(from_curseforge(details?, body.unwrap_or_default()))
            }
        }
    }

    pub async fn project_versions(
        &self,
        source: ContentSource,
        project: &str,
        filter: &VersionFilter,
    ) -> Result<VersionList> {
        match source {
            ContentSource::Ftb => {
                let pack = crate::ftb::pack(&self.net, project).await?;
                let fits = |row: &VersionRow| {
                    filter
                        .minecraft
                        .as_ref()
                        .is_none_or(|wanted| row.game_versions.contains(wanted))
                        && filter
                            .loader
                            .as_ref()
                            .is_none_or(|wanted| row.loaders.contains(wanted))
                };
                let versions: Vec<VersionRow> =
                    crate::ftb::rows(&pack).into_iter().filter(fits).collect();
                Ok(VersionList {
                    total: versions.len() as u64,
                    versions,
                })
            }
            ContentSource::AtLauncher => {
                let pack = self.atl_pack(project).await?;
                let fits = |row: &VersionRow| {
                    filter
                        .minecraft
                        .as_ref()
                        .is_none_or(|wanted| row.game_versions.contains(wanted))
                };
                let versions: Vec<VersionRow> = crate::atlauncher::rows(&pack)
                    .into_iter()
                    .filter(fits)
                    .collect();
                Ok(VersionList {
                    total: versions.len() as u64,
                    versions,
                })
            }
            ContentSource::Modrinth => {
                let versions = modrinth::version_listing(
                    &self.net,
                    project,
                    filter.minecraft.as_deref(),
                    filter.loader.as_deref(),
                )
                .await?;
                Ok(VersionList {
                    total: versions.len() as u64,
                    versions: versions.into_iter().map(modrinth_row).collect(),
                })
            }
            ContentSource::CurseForge => {
                let id = parse_id(project)?;
                let loader = filter
                    .loader
                    .as_deref()
                    .and_then(curseforge::loader_type_of);
                let (files, total) = curseforge::files_page(
                    &self.net,
                    id,
                    filter.minecraft.as_deref(),
                    loader,
                    filter.offset,
                )
                .await?;
                Ok(VersionList {
                    total,
                    versions: files.into_iter().map(curseforge_row).collect(),
                })
            }
        }
    }

    pub async fn version_changelog(
        &self,
        source: ContentSource,
        project: &str,
        version: &str,
    ) -> Result<String> {
        match source {
            ContentSource::Ftb | ContentSource::AtLauncher => Ok(String::new()),
            ContentSource::Modrinth => modrinth::changelog(&self.net, version).await,
            ContentSource::CurseForge => {
                let file = version
                    .parse()
                    .map_err(|_| Error::NotFound(format!("CurseForge file {version}")))?;
                curseforge::file_changelog(&self.net, parse_id(project)?, file).await
            }
        }
    }

    /// The version that would be added to an instance, if there is one.
    pub async fn project_fit(
        &self,
        id: &str,
        source: ContentSource,
        project: &str,
        kind: ContentKind,
    ) -> Result<Option<Fit>> {
        let instance = self.instance(id)?;
        let (minecraft, loader): (&str, LoaderKind) = (&instance.minecraft, instance.loader.kind);
        match source {
            ContentSource::Ftb | ContentSource::AtLauncher => Ok(None),
            ContentSource::Modrinth => {
                let versions =
                    modrinth::versions(&self.net, project, minecraft, loader, kind).await?;
                let Some(version) = modrinth::pick(&versions) else {
                    return Ok(None);
                };
                let needed = version.required_projects();
                let requires = if needed.is_empty() {
                    Vec::new()
                } else {
                    modrinth::projects(&self.net, &needed)
                        .await
                        .unwrap_or_default()
                };
                Ok(Some(Fit {
                    version: version.version_number.clone(),
                    requires: requires
                        .into_iter()
                        .map(|p| Needed {
                            source,
                            id: p.id,
                            title: p.title,
                            icon: p.icon_url,
                        })
                        .collect(),
                }))
            }
            ContentSource::CurseForge => {
                let files =
                    curseforge::files(&self.net, parse_id(project)?, minecraft, loader, kind)
                        .await?;
                let Some(file) = curseforge::pick(&files) else {
                    return Ok(None);
                };
                let needed = file.required_mods();
                let requires = if needed.is_empty() {
                    Vec::new()
                } else {
                    curseforge::mods_by_id(&self.net, &needed)
                        .await
                        .unwrap_or_default()
                };
                Ok(Some(Fit {
                    version: file.display_name.clone(),
                    requires: requires
                        .into_iter()
                        .map(|m| Needed {
                            source,
                            id: m.id.to_string(),
                            title: m.name,
                            icon: m.logo.map(|logo| logo.url),
                        })
                        .collect(),
                }))
            }
        }
    }

    /// Works out what to show for a mod in an instance. Mods from a store open
    /// their store page. Anything else gets the matching project if one exists,
    /// and otherwise whatever its own jar says about it.
    pub async fn installed_mod_page(&self, id: &str, hash: &str) -> Result<InstalledPage> {
        let lock = self.lockfile(id)?;
        let (kind, entry) = lock
            .everything()
            .find(|(_, m)| m.hash == hash)
            .ok_or_else(|| Error::NotFound(format!("file {hash}")))?;
        match &entry.source {
            Source::Modrinth { project, .. } => {
                return Ok(InstalledPage::Store {
                    source: ContentSource::Modrinth,
                    project: project.clone(),
                });
            }
            Source::CurseForge { project, .. } => {
                return Ok(InstalledPage::Store {
                    source: ContentSource::CurseForge,
                    project: project.to_string(),
                });
            }
            Source::Manual => {}
        }

        let jar = self
            .paths
            .game(id)
            .join(content_folder(kind, entry))
            .join(entry.disk_name());
        let info = modinfo::read(&jar).unwrap_or_default();
        if let Some(mod_id) = info.id.as_deref().filter(|id| plain_id(id))
            && let Ok(project) = modrinth::project(&self.net, mod_id).await
            && same_mod(&entry.name, &project.title)
        {
            return Ok(InstalledPage::Namesake {
                source: ContentSource::Modrinth,
                project: project.id,
                file: entry.file.clone(),
                version: entry.version.clone(),
            });
        }
        Ok(InstalledPage::Local(local_mod(
            entry.name.clone(),
            entry.file.clone(),
            entry.size,
            entry.version.clone(),
            entry.icon.clone(),
            info,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn modrinth_project() -> serde_json::Value {
        json!({
            "id": "AANobbMI",
            "slug": "sodium",
            "project_type": "mod",
            "title": "Sodium",
            "description": "Faster rendering.",
            "body": "# Sodium",
            "categories": ["optimization"],
            "additional_categories": ["utility"],
            "client_side": "required",
            "server_side": "unsupported",
            "license": { "id": "LicenseRef-Polyform", "name": "", "url": "https://example.com/license" },
            "downloads": 100,
            "followers": 5,
            "game_versions": ["1.21.4", "26.3"],
            "loaders": ["fabric", "neoforge"],
            "issues_url": "https://github.com/x/y/issues",
            "source_url": "javascript:alert(1)",
            "discord_url": null,
            "donation_urls": [
                { "id": "ko-fi", "platform": "Ko-fi", "url": "https://ko-fi.com/x" },
                { "id": "other", "platform": "Other", "url": "https://example.com/give" },
            ],
            "gallery": [
                { "url": "https://cdn/2.webp", "raw_url": "https://cdn/2.png", "featured": false, "title": "Second", "description": "", "ordering": 1 },
                { "url": "https://cdn/3.webp", "featured": false, "title": null, "description": null, "ordering": 0 },
                { "url": "https://cdn/1.webp", "raw_url": "https://cdn/1.png", "featured": true, "title": "First", "ordering": 9 },
            ],
        })
    }

    #[test]
    fn a_modrinth_project_becomes_a_page() {
        let details = serde_json::from_value(modrinth_project()).unwrap();
        let members = serde_json::from_value(json!([
            { "role": "Maintainer", "user": { "username": "jelly", "avatar_url": "https://cdn/a.png" } }
        ]))
        .unwrap();
        let page = from_modrinth(details, members);
        assert_eq!(page.page, "https://modrinth.com/mod/sodium");
        assert_eq!(page.format, BodyFormat::Markdown);
        assert_eq!(page.categories, ["optimization", "utility"]);
        assert_eq!(page.license.unwrap().label, "Polyform");
        assert_eq!(
            page.creators[0].url.as_deref(),
            Some("https://modrinth.com/user/jelly")
        );
        assert_eq!(page.creators[0].role.as_deref(), Some("Maintainer"));
    }

    #[test]
    fn the_featured_image_comes_first_and_then_the_authors_order() {
        let page = from_modrinth(
            serde_json::from_value(modrinth_project()).unwrap(),
            Vec::new(),
        );
        let order: Vec<&str> = page.gallery.iter().map(|shot| shot.url.as_str()).collect();
        assert_eq!(
            order,
            [
                "https://cdn/1.webp",
                "https://cdn/3.webp",
                "https://cdn/2.webp"
            ]
        );
        assert_eq!(page.gallery[0].full, "https://cdn/1.png");
        assert_eq!(
            page.gallery[1].full, "https://cdn/3.webp",
            "falls back to the smaller image"
        );
        assert_eq!(page.gallery[2].description, None, "blank text isn't shown");
    }

    #[test]
    fn only_web_links_reach_the_page() {
        let page = from_modrinth(
            serde_json::from_value(modrinth_project()).unwrap(),
            Vec::new(),
        );
        let labels: Vec<&str> = page.links.iter().map(|link| link.label.as_str()).collect();
        assert_eq!(labels, ["Issues", "Ko-fi", "Donate"]);
    }

    #[test]
    fn a_missing_donation_list_is_fine() {
        let mut project = modrinth_project();
        project["donation_urls"] = serde_json::Value::Null;
        let page = from_modrinth(serde_json::from_value(project).unwrap(), Vec::new());
        assert_eq!(page.links.len(), 1);
    }

    #[test]
    fn license_ids_read_as_names() {
        assert_eq!(
            license_label("LicenseRef-All-Rights-Reserved"),
            "All Rights Reserved"
        );
        assert_eq!(license_label("LGPL-3.0-only"), "LGPL-3.0-only");
    }

    #[test]
    fn modrinth_project_types_map_to_kinds() {
        assert_eq!(modrinth_kind("modpack", &[]), ContentKind::Modpack);
        assert_eq!(modrinth_kind("shader", &[]), ContentKind::Shader);
        assert_eq!(
            modrinth_kind("resourcepack", &[]),
            ContentKind::ResourcePack
        );
        assert_eq!(modrinth_kind("mod", &[]), ContentKind::Mod);
        assert_eq!(modrinth_kind("plugin", &[]), ContentKind::Mod);
        assert_eq!(
            modrinth_kind("mod", &["datapack".to_string()]),
            ContentKind::DataPack
        );
        assert_eq!(
            modrinth_kind("mod", &["datapack".to_string(), "fabric".to_string()]),
            ContentKind::Mod
        );
    }

    #[test]
    fn a_curseforge_project_becomes_a_page() {
        let details: curseforge::Mod = serde_json::from_value(json!({
            "id": 238222,
            "name": "Just Enough Items (JEI)",
            "slug": "jei",
            "summary": "View Items and Recipes",
            "downloadCount": 642654746u64,
            "classId": 6,
            "licenseType": "MIT License",
            "dateCreated": "2015-11-23T23:00:41.923Z",
            "dateReleased": "2026-10-31T15:09:19.357Z",
            "authors": [{ "name": "mezz", "url": "https://www.curseforge.com/members/mezz", "avatarUrl": "https://x/a.png" }],
            "logo": { "url": "https://media/logo.png", "thumbnailUrl": "https://media/thumb.png" },
            "screenshots": [{ "url": "https://media/s.png", "thumbnailUrl": "https://media/s-t.png", "title": "Recipes", "description": "" }],
            "categories": [{ "name": "API and Library" }],
            "links": { "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei", "wikiUrl": "", "issuesUrl": "https://github.com/mezz/x/issues", "sourceUrl": "https://github.com/mezz/x" },
            "latestFilesIndexes": [
                { "gameVersion": "1.21.11", "modLoader": 6 },
                { "gameVersion": "1.21.11", "modLoader": 4 },
                { "gameVersion": "1.20.1", "modLoader": 1 },
                { "gameVersion": "1.12.2", "modLoader": 0 },
            ],
        }))
        .unwrap();
        let page = from_curseforge(details, "<p>Hello</p>".into());
        assert_eq!(page.id, "238222");
        assert_eq!(page.format, BodyFormat::Html);
        assert_eq!(page.kind, ContentKind::Mod);
        assert_eq!(page.game_versions, ["1.21.11", "1.20.1", "1.12.2"]);
        assert_eq!(page.loaders, ["neoforge", "fabric", "forge"]);
        assert_eq!(page.license.unwrap().label, "MIT License");
        assert_eq!(page.icon.as_deref(), Some("https://media/logo.png"));
        assert_eq!(page.gallery[0].url, "https://media/s-t.png");
        assert_eq!(page.gallery[0].full, "https://media/s.png");
        let labels: Vec<&str> = page.links.iter().map(|link| link.label.as_str()).collect();
        assert_eq!(labels, ["Issues", "Source"]);
    }

    #[test]
    fn a_blocked_curseforge_file_is_marked_unavailable() {
        let file: curseforge::File = serde_json::from_value(json!({
            "id": 1,
            "modId": 2,
            "displayName": "JEI 19",
            "fileName": "jei.jar",
            "fileLength": 500,
            "releaseType": 2,
            "fileDate": "2026-01-01T00:00:00Z",
            "gameVersions": ["Client", "NeoForge", "Server", "1.21.11", "Java 21"],
        }))
        .unwrap();
        let row = curseforge_row(file);
        assert!(!row.available);
        assert_eq!(row.channel, "beta");
        assert_eq!(row.loaders, ["neoforge"]);
        assert_eq!(row.game_versions, ["1.21.11"]);
    }

    #[test]
    fn a_jar_can_only_put_a_plain_id_into_a_url() {
        assert!(plain_id("fabric-api"));
        assert!(plain_id("mod_menu2"));
        assert!(!plain_id(""));
        assert!(!plain_id("../user/x"));
        assert!(!plain_id("a?b=c"));
    }

    #[test]
    fn a_local_mod_prefers_what_the_lockfile_knows_and_keeps_only_web_links() {
        let info = ModInfo {
            version: Some("from jar".into()),
            description: Some("  A mod.  ".into()),
            authors: vec!["Someone".into()],
            license: Some("MIT".into()),
            homepage: Some("mailto:me@example.com".into()),
            sources: Some("https://github.com/x/y".into()),
            ..ModInfo::default()
        };
        let page = local_mod(
            "Thing".into(),
            "thing.jar".into(),
            9,
            Some("1.0".into()),
            None,
            info,
        );
        assert_eq!(page.version.as_deref(), Some("1.0"));
        assert_eq!(page.description.as_deref(), Some("A mod."));
        assert_eq!(
            page.links,
            [Link {
                label: "Source".into(),
                url: "https://github.com/x/y".into()
            }]
        );
    }
}

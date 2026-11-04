//! The server list: public Java servers that owners list on Modrinth, and every
//! server already saved in the player's instances. A listing names its address and
//! what it needs, either a Minecraft version or one exact version of a modpack, so
//! joining makes the right instance once and then reuses it.
//!
//! Listings come from Modrinth's v3 search, which is public like the v2 one the
//! rest of Waymark uses, but newer and less settled, so everything is read
//! leniently and a listing Waymark can't use is left out instead of failing a page.

use std::collections::BTreeMap;
use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Core;
use crate::content::SearchCategory;
use crate::error::{Error, Result};
use crate::instance::{Instance, NewInstance, ServerLink};
use crate::lock::LoaderKind;
use crate::packs::ManualFile;

const API: &str = "https://api.modrinth.com/v3";
const PROJECT_TYPE: &str = "minecraft_java_server";
const SAVED_FILE: &str = "saved-servers.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerSort {
    /// Modrinth ranks these by recent joins from its own app.
    #[default]
    Popular,
    Followed,
    Newest,
    Updated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    Vanilla,
    Modded,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerQuery {
    pub text: String,
    pub sort: ServerSort,
    pub kind: Option<ServerKind>,
    /// Every one of these must match.
    pub categories: Vec<String>,
    pub region: Option<String>,
    pub language: Option<String>,
    pub offset: u32,
    pub limit: u32,
}

/// What a server needs to be joined.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerContent {
    /// Plain Minecraft, on any of these versions.
    Vanilla {
        versions: Vec<String>,
        recommended: Option<String>,
    },
    /// One exact version of a Modrinth modpack, which may be the server's own.
    Modpack {
        project: String,
        version: String,
        name: Option<String>,
        icon: Option<String>,
        /// The pack's Minecraft version and loader, as the listing gives them.
        minecraft: Option<String>,
        loader: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicServer {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub icon: Option<String>,
    pub banner: Option<String>,
    pub address: String,
    pub content: ServerContent,
    /// Whether Modrinth's last ping got an answer.
    pub online: bool,
    pub players: Option<u32>,
    pub capacity: Option<u32>,
    /// What the server calls its version, like `Paper 1.21.4`.
    pub version_name: Option<String>,
    pub region: Option<String>,
    pub languages: Vec<String>,
    pub categories: Vec<String>,
    /// Joins from the Modrinth App over the last two weeks.
    pub plays: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPage {
    pub servers: Vec<PublicServer>,
    pub total: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerLinkOut {
    pub label: String,
    pub url: String,
}

/// Everything shown on a server's own page.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDetails {
    #[serde(flatten)]
    pub server: PublicServer,
    /// Markdown, rendered through the same safe list as project pages.
    pub description: String,
    pub gallery: Vec<String>,
    pub links: Vec<ServerLinkOut>,
    pub followers: u64,
    /// The page on Modrinth, which descriptions resolve their links against.
    pub page: String,
    /// The instance already made for it, if there is one.
    pub instance: Option<String>,
}

/// An instance ready to join a server.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInstance {
    pub instance: Instance,
    pub address: String,
    pub created: bool,
    /// The server moved to a new pack version and the instance followed.
    pub updated: bool,
    /// Files a pack wanted that have to be downloaded by hand.
    pub manual: Vec<ManualFile>,
}

/// A listed server the player kept for later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptServer {
    pub id: String,
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRef {
    pub id: String,
    pub name: String,
    pub minecraft: String,
}

/// One address saved in the player's instances, and which instances have it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownServer {
    pub name: String,
    pub address: String,
    pub icon: Option<String>,
    pub instances: Vec<InstanceRef>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Plain ids, since they end up in web addresses and instance files.
fn plain(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// A listing from a search hit or a project, or nothing if it can't be joined
/// from here: a Bedrock-only server, an unknown kind of content, or a bad id.
fn read_server(raw: &Value) -> Option<PublicServer> {
    let id = text(raw, "project_id").or_else(|| text(raw, "id"))?;
    let java = raw.get("minecraft_java_server")?;
    let address = text(java, "address")?;
    let content = java.get("content")?;
    let mut categories = strings(raw, "categories");
    for extra in strings(raw, "additional_categories") {
        if !categories.contains(&extra) {
            categories.push(extra);
        }
    }
    // Loaders sit among the categories in search results.
    let loader = categories
        .iter()
        .find(|category| matches!(category.as_str(), "fabric" | "forge" | "neoforge" | "quilt"))
        .cloned();
    categories.retain(|category| Some(category) != loader.as_ref());
    let content = match content.get("kind").and_then(Value::as_str)? {
        "vanilla" => ServerContent::Vanilla {
            versions: strings(content, "supported_game_versions"),
            recommended: text(content, "recommended_game_version"),
        },
        "modpack" => {
            let project = text(content, "project_id").filter(|id| plain(id))?;
            let version = text(content, "version_id").filter(|id| plain(id))?;
            ServerContent::Modpack {
                project,
                version,
                name: text(content, "project_name"),
                icon: text(content, "project_icon"),
                minecraft: strings(raw, "game_versions").into_iter().next(),
                loader,
            }
        }
        _ => return None,
    };
    let ping = java.get("ping").and_then(|ping| ping.get("data"));
    let meta = raw.get("minecraft_server").unwrap_or(&Value::Null);
    let banner = text(raw, "featured_gallery").or_else(|| {
        raw.get("gallery")?
            .as_array()?
            .iter()
            .find(|image| image.get("featured").and_then(Value::as_bool) == Some(true))
            .or_else(|| raw.get("gallery")?.as_array()?.first())
            .and_then(|image| text(image, "url"))
    });
    Some(PublicServer {
        slug: text(raw, "slug").unwrap_or_else(|| id.clone()),
        id: id.clone(),
        name: text(raw, "name").unwrap_or_else(|| address.clone()),
        summary: text(raw, "summary").unwrap_or_default(),
        icon: text(raw, "icon_url"),
        banner,
        address,
        content,
        online: ping.is_some(),
        players: ping
            .and_then(|data| number(data, "players_online"))
            .map(|n| n as u32),
        capacity: ping
            .and_then(|data| number(data, "players_max"))
            .map(|n| n as u32)
            .or_else(|| number(meta, "max_players").map(|n| n as u32)),
        version_name: ping.and_then(|data| text(data, "version_name")),
        region: text(meta, "region"),
        languages: strings(meta, "languages"),
        categories,
        plays: number(java, "verified_plays_2w"),
    })
}

/// The search facets for a query, as Modrinth wants them.
fn facets(query: &ServerQuery) -> Value {
    let mut facets = vec![json!([format!("project_types:{PROJECT_TYPE}")])];
    if let Some(kind) = query.kind {
        let name = match kind {
            ServerKind::Vanilla => "vanilla",
            ServerKind::Modded => "modpack",
        };
        facets.push(json!([format!(
            "minecraft_java_server.content.kind:{name}"
        )]));
    }
    for category in query.categories.iter().filter(|c| plain(c)) {
        facets.push(json!([format!("categories:{category}")]));
    }
    if let Some(region) = query.region.as_deref().filter(|r| plain(r)) {
        facets.push(json!([format!("minecraft_server.region:{region}")]));
    }
    if let Some(language) = query.language.as_deref().filter(|l| plain(l)) {
        facets.push(json!([format!("minecraft_server.languages:{language}")]));
    }
    json!(facets)
}

fn sort_index(sort: ServerSort) -> &'static str {
    match sort {
        ServerSort::Popular => "relevance",
        ServerSort::Followed => "follows",
        ServerSort::Newest => "newest",
        ServerSort::Updated => "updated",
    }
}

/// Labels for the links an owner adds, in the order they read best.
fn read_links(raw: &Value) -> Vec<ServerLinkOut> {
    let Some(links) = raw.get("link_urls").and_then(Value::as_object) else {
        return Vec::new();
    };
    let label = |key: &str| match key {
        "discord" => "Discord".to_string(),
        "site" | "website" => "Website".to_string(),
        "store" => "Store".to_string(),
        "wiki" => "Wiki".to_string(),
        "source" => "Source".to_string(),
        "issues" => "Issues".to_string(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    };
    let ordered: BTreeMap<&String, &Value> = links.iter().collect();
    ordered
        .into_iter()
        .filter_map(|(key, link)| {
            let url = text(link, "url")?;
            url.starts_with("https://").then(|| ServerLinkOut {
                label: label(key),
                url,
            })
        })
        .collect()
}

/// The version a vanilla server is joined on: the owner's pick, or else the
/// newest one it lists.
fn vanilla_version(versions: &[String], recommended: Option<&str>) -> Option<String> {
    recommended.map(str::to_string).or_else(|| {
        versions
            .iter()
            .max_by(|a, b| compare_versions(a, b))
            .cloned()
    })
}

/// Orders release numbers like 1.20.1 < 1.21 < 26.1, leaving anything else last.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |text: &str| -> Option<Vec<u32>> {
        text.split('.').map(|part| part.parse().ok()).collect()
    };
    match (parts(a), parts(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => a.cmp(b),
    }
}

/// `host:port` with the default port dropped and the host in lower case, so the
/// same server saved two ways counts once.
fn address_key(address: &str) -> String {
    let (host, port) = crate::worlds::split_address(address);
    let host = host.to_ascii_lowercase();
    if port == 25565 {
        host
    } else {
        format!("{host}:{port}")
    }
}

impl Core {
    /// Listed servers matching a query, a page at a time.
    pub async fn public_servers(&self, query: &ServerQuery) -> Result<ServerPage> {
        let limit = if query.limit == 0 {
            24
        } else {
            query.limit.min(100)
        };
        let response: Value = self
            .net
            .client()
            .get(format!("{API}/search"))
            .query(&[
                ("query", query.text.trim()),
                ("facets", &facets(query).to_string()),
                ("index", sort_index(query.sort)),
                ("offset", &query.offset.to_string()),
                ("limit", &limit.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let servers = response
            .get("hits")
            .and_then(Value::as_array)
            .map(|hits| hits.iter().filter_map(read_server).collect())
            .unwrap_or_default();
        Ok(ServerPage {
            servers,
            total: number(&response, "total_hits").unwrap_or(0),
        })
    }

    /// The categories owners can file a server under, with their group.
    pub async fn server_categories(&self) -> Result<Vec<SearchCategory>> {
        let all: Vec<Value> = self.net.json(&format!("{API}/tag/category")).await?;
        Ok(all
            .iter()
            .filter(|category| {
                category.get("project_type").and_then(Value::as_str) == Some(PROJECT_TYPE)
            })
            .filter_map(|category| {
                let id = text(category, "name")?;
                let group = text(category, "header")
                    .map(|header| header.trim_start_matches("minecraft_server_").to_string())
                    .unwrap_or_default();
                Some(SearchCategory {
                    name: id.clone(),
                    id,
                    group,
                })
            })
            .collect())
    }

    /// A listed server's own page.
    pub async fn public_server(&self, id: &str) -> Result<ServerDetails> {
        if !plain(id) {
            return Err(Error::NotFound("that server".into()));
        }
        let raw: Value = self.net.json(&format!("{API}/project/{id}")).await?;
        let server = read_server(&raw).ok_or_else(|| {
            Error::Other(
                "this server can't be joined from Waymark, because it doesn't list a Java address"
                    .into(),
            )
        })?;
        let gallery = raw
            .get("gallery")
            .and_then(Value::as_array)
            .map(|images| {
                images
                    .iter()
                    .filter_map(|image| text(image, "url"))
                    .collect()
            })
            .unwrap_or_default();
        Ok(ServerDetails {
            description: text(&raw, "description").unwrap_or_default(),
            gallery,
            links: read_links(&raw),
            followers: number(&raw, "followers").unwrap_or(0),
            page: format!("https://modrinth.com/server/{}", server.slug),
            instance: self
                .linked_instance(&server.id)?
                .map(|instance| instance.id),
            server,
        })
    }

    fn linked_instance(&self, project: &str) -> Result<Option<Instance>> {
        Ok(self.instances()?.into_iter().find(|instance| {
            instance
                .server
                .as_ref()
                .is_some_and(|link| link.project == project)
        }))
    }

    /// Gets an instance ready to join a listed server. The one made for it last
    /// time is used again, moved to the server's pack version if that changed;
    /// otherwise a new one is made, with the server already in its list.
    pub async fn server_instance(&self, id: &str) -> Result<ServerInstance> {
        let details = self.public_server(id).await?;
        let server = details.server;
        let link = ServerLink {
            project: server.id.clone(),
            name: server.name.clone(),
            address: server.address.clone(),
            icon: server.icon.clone(),
        };
        let linked = self.linked_instance(&server.id)?;

        match &server.content {
            ServerContent::Vanilla {
                versions,
                recommended,
            } => {
                if let Some(instance) = linked.filter(|instance| {
                    versions.is_empty() || versions.contains(&instance.minecraft)
                }) {
                    return self.relink(instance, link, false, Vec::new());
                }
                let minecraft =
                    vanilla_version(versions, recommended.as_deref()).ok_or_else(|| {
                        Error::Other(
                            "this server doesn't say which Minecraft version it runs".into(),
                        )
                    })?;
                // Fabric leaves room for client mods, like the server owner expects.
                let fabric = self
                    .loader_versions(LoaderKind::Fabric, &minecraft)
                    .await
                    .is_ok_and(|found| !found.is_empty());
                let instance = self
                    .create_instance(NewInstance {
                        name: server.name.clone(),
                        minecraft,
                        loader: if fabric {
                            LoaderKind::Fabric
                        } else {
                            LoaderKind::Vanilla
                        },
                        loader_version: None,
                    })
                    .await?;
                if let Err(error) = self.add_starters(&instance.id).await {
                    tracing::warn!(%error, "could not add starter mods for a server");
                }
                self.relink(instance, link, true, Vec::new())
            }
            ServerContent::Modpack {
                project, version, ..
            } => {
                if let Some(instance) = linked {
                    let mut updated = false;
                    if self.pack_version_id(&instance.id).as_deref() != Some(version.as_str()) {
                        self.update_pack_to(&instance.id, version).await?;
                        updated = true;
                    }
                    let instance = self.instance(&instance.id)?;
                    return self
                        .relink(instance, link, false, Vec::new())
                        .map(|mut ready| {
                            ready.updated = updated;
                            ready
                        });
                }
                let installed = self.install_pack_version(project, version).await?;
                let mut instance = installed.instance;
                instance.name = server.name.clone();
                self.save_instance(&instance)?;
                self.relink(instance, link, true, installed.manual)
            }
        }
    }

    /// Ties an instance to its server and makes sure the server is in its list.
    fn relink(
        &self,
        mut instance: Instance,
        link: ServerLink,
        created: bool,
        manual: Vec<ManualFile>,
    ) -> Result<ServerInstance> {
        let address = link.address.clone();
        let listed = self
            .servers(&instance.id)
            .map(|servers| {
                servers
                    .iter()
                    .any(|saved| address_key(&saved.address) == address_key(&address))
            })
            .unwrap_or(false);
        if !listed && let Err(error) = self.add_server(&instance.id, &link.name, &address) {
            tracing::warn!(%error, "could not add the server to the instance's list");
        }
        if instance.server.as_ref() != Some(&link) {
            instance.server = Some(link);
            self.save_instance(&instance)?;
        }
        Ok(ServerInstance {
            instance,
            address,
            created,
            updated: false,
            manual,
        })
    }

    pub fn kept_servers(&self) -> Result<Vec<KeptServer>> {
        match fs::read(self.paths.root().join(SAVED_FILE)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Keeps a listed server for later, or lets it go.
    pub fn keep_server(&self, server: KeptServer, keep: bool) -> Result<Vec<KeptServer>> {
        if !plain(&server.id) {
            return Err(Error::NotFound("that server".into()));
        }
        let mut kept = self.kept_servers()?;
        kept.retain(|other| other.id != server.id);
        if keep {
            kept.insert(0, server);
        }
        let path = self.paths.root().join(SAVED_FILE);
        let part = path.with_extension("json.part");
        fs::write(&part, serde_json::to_vec_pretty(&kept)?)?;
        fs::rename(part, path)?;
        Ok(kept)
    }

    /// Every server saved in the player's instances, once per address.
    pub fn known_servers(&self) -> Result<Vec<KnownServer>> {
        let mut known: Vec<KnownServer> = Vec::new();
        for instance in self.instances()? {
            let Ok(servers) = self.servers(&instance.id) else {
                continue;
            };
            let place = InstanceRef {
                id: instance.id.clone(),
                name: instance.name.clone(),
                minecraft: instance.minecraft.clone(),
            };
            for saved in servers {
                if saved.address.trim().is_empty() {
                    continue;
                }
                let key = address_key(&saved.address);
                match known
                    .iter_mut()
                    .find(|other| address_key(&other.address) == key)
                {
                    Some(other) => {
                        if !other.instances.contains(&place) {
                            other.instances.push(place.clone());
                        }
                        if other.icon.is_none() {
                            other.icon = saved.icon.clone();
                        }
                    }
                    None => known.push(KnownServer {
                        name: saved.name.clone(),
                        address: saved.address.clone(),
                        icon: saved.icon.clone(),
                        instances: vec![place.clone()],
                    }),
                }
            }
        }
        Ok(known)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;

    fn modded() -> Value {
        json!({
            "project_id": "OgcNPs6n",
            "slug": "aero-server",
            "name": "Aero SMP",
            "summary": "Create and Create Aeronautics.",
            "icon_url": "https://cdn.modrinth.com/data/OgcNPs6n/icon.webp",
            "featured_gallery": "https://cdn.modrinth.com/data/OgcNPs6n/images/banner.webp",
            "categories": ["neoforge", "smp", "technical"],
            "minecraft_java_server": {
                "address": "play.aerosmp.com",
                "content": { "kind": "modpack", "version_id": "GDELFL7Y", "project_id": "OgcNPs6n", "project_name": "Aero SMP" },
                "ping": { "when": "2026-11-04T21:40:11Z", "data": { "players_online": 21, "players_max": 300, "version_name": "Velocity 1.7.2" } },
                "verified_plays_2w": 3974
            },
            "minecraft_server": { "region": "europe", "languages": ["en"], "max_players": null },
            "minecraft_bedrock_server": { "address": "" }
        })
    }

    #[test]
    fn a_modded_listing_is_read_with_its_live_numbers() {
        let server = read_server(&modded()).unwrap();
        assert_eq!(server.id, "OgcNPs6n");
        assert_eq!(server.address, "play.aerosmp.com");
        assert_eq!(
            server.content,
            ServerContent::Modpack {
                project: "OgcNPs6n".into(),
                version: "GDELFL7Y".into(),
                name: Some("Aero SMP".into()),
                icon: None,
                minecraft: None,
                loader: Some("neoforge".into())
            }
        );
        assert!(server.online);
        assert_eq!((server.players, server.capacity), (Some(21), Some(300)));
        assert_eq!(server.region.as_deref(), Some("europe"));
        assert_eq!(server.categories, ["smp", "technical"]);
        assert_eq!(server.plays, Some(3974));
        assert!(server.banner.unwrap().ends_with("banner.webp"));
    }

    #[test]
    fn a_vanilla_listing_and_an_unreachable_one() {
        let server = read_server(&json!({
            "id": "abc123",
            "name": "The Vee Hive",
            "minecraft_java_server": {
                "address": "play.veehive.net:25566",
                "content": { "kind": "vanilla", "supported_game_versions": ["1.21.4", "26.2"], "recommended_game_version": "26.2" },
                "ping": { "when": "2026-11-04T21:40:11Z" }
            },
            "gallery": [{ "url": "https://cdn.modrinth.com/a.webp" }, { "url": "https://cdn.modrinth.com/b.webp", "featured": true }]
        }))
        .unwrap();
        assert_eq!(
            server.content,
            ServerContent::Vanilla {
                versions: vec!["1.21.4".into(), "26.2".into()],
                recommended: Some("26.2".into())
            }
        );
        assert!(!server.online);
        assert_eq!(server.players, None);
        assert_eq!(
            server.banner.as_deref(),
            Some("https://cdn.modrinth.com/b.webp")
        );
    }

    #[test]
    fn listings_waymark_cannot_join_are_left_out() {
        let mut bedrock = modded();
        bedrock["minecraft_java_server"]["address"] = json!("");
        assert!(read_server(&bedrock).is_none());
        let mut unknown = modded();
        unknown["minecraft_java_server"]["content"]["kind"] = json!("datapack");
        assert!(read_server(&unknown).is_none());
        let mut hostile = modded();
        hostile["minecraft_java_server"]["content"]["version_id"] = json!("../../x");
        assert!(read_server(&hostile).is_none());
        assert!(read_server(&json!({ "project_id": "x" })).is_none());
    }

    #[test]
    fn filters_become_modrinth_facets() {
        let query = ServerQuery {
            kind: Some(ServerKind::Modded),
            categories: vec!["smp".into(), "pvp".into()],
            region: Some("us_east".into()),
            language: Some("de".into()),
            ..ServerQuery::default()
        };
        assert_eq!(
            facets(&query),
            json!([
                ["project_types:minecraft_java_server"],
                ["minecraft_java_server.content.kind:modpack"],
                ["categories:smp"],
                ["categories:pvp"],
                ["minecraft_server.region:us_east"],
                ["minecraft_server.languages:de"]
            ])
        );
        let sneaky = ServerQuery {
            categories: vec!["smp\"],[\"x".into()],
            ..ServerQuery::default()
        };
        assert_eq!(
            facets(&sneaky),
            json!([["project_types:minecraft_java_server"]])
        );
    }

    #[test]
    fn a_vanilla_server_is_joined_on_the_owner_s_pick_or_the_newest() {
        let versions = vec![
            "1.20.1".to_string(),
            "26.1".to_string(),
            "1.21.4".to_string(),
        ];
        assert_eq!(
            vanilla_version(&versions, Some("1.21.4")).as_deref(),
            Some("1.21.4")
        );
        assert_eq!(vanilla_version(&versions, None).as_deref(), Some("26.1"));
        assert_eq!(vanilla_version(&[], None), None);
    }

    #[test]
    fn links_are_labelled_and_only_https() {
        let links = read_links(&json!({ "link_urls": {
            "discord": { "url": "https://discord.gg/aerosmp" },
            "site": { "url": "http://insecure.example" },
            "store": { "url": "https://store.example" }
        }}));
        assert_eq!(
            links
                .iter()
                .map(|link| link.label.as_str())
                .collect::<Vec<_>>(),
            ["Discord", "Store"]
        );
    }

    #[test]
    fn kept_servers_and_the_servers_in_your_instances() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let kept = KeptServer {
            id: "OgcNPs6n".into(),
            name: "Aero SMP".into(),
            address: "play.aerosmp.com".into(),
            icon: None,
        };
        assert_eq!(
            core.keep_server(kept.clone(), true).unwrap(),
            vec![kept.clone()]
        );
        assert_eq!(core.kept_servers().unwrap(), vec![kept.clone()]);
        assert!(core.keep_server(kept, false).unwrap().is_empty());

        let a = core
            .register_instance("A", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let b = core
            .register_instance("B", "1.20.1", Loader::default(), "x".into(), None)
            .unwrap();
        core.add_server(&a.id, "Hypixel", "mc.hypixel.net").unwrap();
        core.add_server(&b.id, "Hypixel again", "MC.Hypixel.net:25565")
            .unwrap();
        core.add_server(&b.id, "Friends", "10.0.0.5:25570").unwrap();
        let known = core.known_servers().unwrap();
        assert_eq!(known.len(), 2, "{known:?}");
        // The name comes from the instance made most recently.
        let hypixel = known
            .iter()
            .find(|server| address_key(&server.address) == "mc.hypixel.net")
            .unwrap();
        assert_eq!(hypixel.instances.len(), 2);
    }

    /// Run with `cargo test -p waymark-core live -- --ignored` to check against Modrinth.
    #[tokio::test]
    #[ignore = "talks to Modrinth"]
    async fn live_listings_pages_and_categories_can_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let page = core
            .public_servers(&ServerQuery {
                limit: 50,
                ..ServerQuery::default()
            })
            .await
            .unwrap();
        assert!(page.total > 100, "only {} listed", page.total);
        assert!(
            page.servers.len() >= 40,
            "only {} of 50 could be read",
            page.servers.len()
        );
        let vanilla = core
            .public_servers(&ServerQuery {
                kind: Some(ServerKind::Vanilla),
                limit: 20,
                ..ServerQuery::default()
            })
            .await
            .unwrap();
        assert!(!vanilla.servers.is_empty());
        assert!(
            vanilla
                .servers
                .iter()
                .all(|server| matches!(server.content, ServerContent::Vanilla { .. }))
        );
        let searched = core
            .public_servers(&ServerQuery {
                text: page.servers[0].name.clone(),
                limit: 5,
                ..ServerQuery::default()
            })
            .await
            .unwrap();
        assert!(
            searched
                .servers
                .iter()
                .any(|server| server.id == page.servers[0].id)
        );
        let details = core.public_server(&page.servers[0].id).await.unwrap();
        assert_eq!(details.server.address, page.servers[0].address);
        assert!(!details.description.is_empty());
        let categories = core.server_categories().await.unwrap();
        assert!(categories.len() > 30, "{}", categories.len());
        assert!(
            categories
                .iter()
                .any(|category| category.group == "gameplay")
        );
    }

    /// Joining a real vanilla listing makes one linked instance, then reuses it.
    #[tokio::test]
    #[ignore = "talks to Modrinth, Mojang and Fabric"]
    async fn live_joining_a_vanilla_server_makes_one_instance() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let page = core
            .public_servers(&ServerQuery {
                kind: Some(ServerKind::Vanilla),
                limit: 10,
                ..ServerQuery::default()
            })
            .await
            .unwrap();
        let server = &page.servers[0];
        let first = core.server_instance(&server.id).await.unwrap();
        assert!(first.created);
        assert_eq!(first.address, server.address);
        let link = first.instance.server.clone().unwrap();
        assert_eq!(link.project, server.id);
        assert!(
            core.servers(&first.instance.id)
                .unwrap()
                .iter()
                .any(|saved| saved.address == server.address)
        );
        let again = core.server_instance(&server.id).await.unwrap();
        assert!(!again.created);
        assert_eq!(again.instance.id, first.instance.id);
        assert_eq!(core.instances().unwrap().len(), 1);
    }

    #[test]
    fn joining_again_reuses_the_instance_made_for_the_server() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Aero SMP", "1.21.1", Loader::default(), "x".into(), None)
            .unwrap();
        let link = ServerLink {
            project: "OgcNPs6n".into(),
            name: "Aero SMP".into(),
            address: "play.aerosmp.com".into(),
            icon: None,
        };
        let ready = core
            .relink(instance, link.clone(), true, Vec::new())
            .unwrap();
        assert_eq!(ready.address, "play.aerosmp.com");
        assert_eq!(
            core.linked_instance("OgcNPs6n").unwrap().unwrap().id,
            ready.instance.id
        );
        assert_eq!(
            core.instance(&ready.instance.id).unwrap().server,
            Some(link.clone())
        );
        // The server goes into its list once, however often it's joined.
        core.relink(ready.instance.clone(), link, false, Vec::new())
            .unwrap();
        assert_eq!(core.servers(&ready.instance.id).unwrap().len(), 1);
    }
}

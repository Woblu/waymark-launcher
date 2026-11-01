//! Modpacks: installing them as new instances, and updating them without
//! losing what the player changed. Pack files are untrusted input: an attacker
//! who controls a pack controls every path and URL in it.
//!
//! A pack is read into a tree of stored files, then merged onto the instance.
//! Installing is a merge onto nothing. Updating is a three-way merge between
//! what the pack installed last time, what's on disk now, and the new version,
//! which is what lets the player's additions and edits survive.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::Core;
use crate::atlauncher;
use crate::content::{ContentKind, ContentSource, side_from};
use crate::curseforge;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::ftb;
use crate::install::safe_relative;
use crate::instance::Instance;
use crate::lock::{Loader, LoaderKind, LockedMod, Lockfile, Origin, Side, Source};
use crate::modrinth;
use crate::net::{Expected, Fetch};
use crate::store::{Placement, hash_file};
use crate::waymarks::WaymarkKind;

/// The hosts the `.mrpack` format allows files to come from.
const ALLOWED_HOSTS: [&str; 4] = [
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];

/// How many ids go in one bulk request, which keeps the URL a sensible length.
const LOOKUP_BATCH: usize = 80;

pub(crate) const PACK_STATE: &str = "pack.json";
const MANUAL_FILES: &str = ".waymarks/manual-files.json";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrpackIndex {
    name: String,
    files: Vec<MrpackFile>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrpackFile {
    path: String,
    hashes: MrpackHashes,
    #[serde(default)]
    env: Option<MrpackEnv>,
    downloads: Vec<String>,
    file_size: u64,
}

#[derive(Deserialize)]
struct MrpackHashes {
    sha512: String,
}

#[derive(Deserialize)]
struct MrpackEnv {
    client: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseManifest {
    name: String,
    minecraft: CurseMinecraft,
    files: Vec<CurseFileRef>,
    #[serde(default = "default_overrides")]
    overrides: String,
}

fn default_overrides() -> String {
    "overrides".to_string()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CurseLoader>,
}

#[derive(Deserialize)]
struct CurseLoader {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseFileRef {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    #[serde(default = "required_by_default")]
    required: bool,
}

fn required_by_default() -> bool {
    true
}

/// A pack file an author won't allow other apps to download. The player gets
/// it from the store's own page, and it's picked up from their downloads.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualFile {
    pub name: String,
    pub page: String,
    pub file: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub project: u64,
    #[serde(default)]
    pub file_id: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackResult {
    pub instance: Instance,
    pub manual: Vec<ManualFile>,
}

/// Where a pack came from, so a newer version can be found later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackOrigin {
    pub source: ContentSource,
    pub project: String,
    pub version_id: String,
    pub version: String,
}

/// What a pack put in an instance, kept as the base of the next merge.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackState {
    name: String,
    origin: PackOrigin,
    files: BTreeMap<String, InstalledFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledFile {
    path: String,
    hash: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdate {
    pub name: String,
    pub current: String,
    pub latest: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdateReport {
    pub version: String,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// Files the player changed, kept as they are.
    pub kept: Vec<String>,
    /// Pack files the player removed, left out.
    pub stayed_removed: Vec<String>,
    pub minecraft_changed: bool,
    pub manual: Vec<ManualFile>,
}

struct TreeFile {
    path: String,
    size: u64,
    hash: String,
    source: Source,
    remote_hash: Option<String>,
    name: Option<String>,
    version: Option<String>,
    icon: Option<String>,
}

impl TreeFile {
    /// Mods keep their identity across versions even when the file name
    /// changes, which it nearly always does.
    fn key(&self) -> String {
        project_key(&self.source).unwrap_or_else(|| self.path.clone())
    }
}

struct PackTree {
    name: String,
    minecraft: String,
    loader: Loader,
    files: Vec<TreeFile>,
    manual: Vec<ManualFile>,
}

fn project_key(source: &Source) -> Option<String> {
    match source {
        Source::Modrinth { project, .. } => Some(format!("modrinth:{project}")),
        Source::CurseForge { project, .. } => Some(format!("curseforge:{project}")),
        Source::Manual => None,
    }
}

/// What to do with each file in a merge, decided from hashes alone.
#[derive(Debug, Default, PartialEq, Eq)]
struct Merge {
    place: Vec<String>,
    delete: Vec<String>,
    /// The player changed this file, so it stays as it is.
    kept: Vec<String>,
    /// The player removed this file, so it stays out.
    stayed_removed: Vec<String>,
}

/// Three-way merge keyed by file identity. A key can be missing from any side.
fn plan_merge(
    base: &BTreeMap<String, String>,
    ours: &BTreeMap<String, String>,
    theirs: &BTreeMap<String, String>,
) -> Merge {
    let mut merge = Merge::default();
    let keys: BTreeSet<&String> = base.keys().chain(theirs.keys()).collect();
    for key in keys {
        let (old, mine, new) = (base.get(key), ours.get(key), theirs.get(key));
        match (old, mine, new) {
            (None, None, Some(_)) => merge.place.push(key.clone()),
            (Some(_), None, Some(_)) => merge.stayed_removed.push(key.clone()),
            (Some(old), Some(mine), None) if old == mine => merge.delete.push(key.clone()),
            (Some(_), Some(_), None) => merge.kept.push(key.clone()),
            (Some(old), Some(mine), Some(new)) if mine == new || (mine != old && new == old) => {}
            (Some(old), Some(mine), Some(_)) if old == mine => merge.place.push(key.clone()),
            (_, Some(mine), Some(new)) if mine != new => merge.kept.push(key.clone()),
            _ => {}
        }
    }
    merge
}

fn host_allowed(url: &str) -> bool {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|host| ALLOWED_HOSTS.contains(&host))
}

fn mrpack_loader(dependencies: &BTreeMap<String, String>) -> Result<(String, Loader)> {
    let minecraft = dependencies
        .get("minecraft")
        .cloned()
        .ok_or_else(|| Error::Other("this pack doesn't say which Minecraft it needs".into()))?;
    let known = [
        ("fabric-loader", LoaderKind::Fabric),
        ("quilt-loader", LoaderKind::Quilt),
        ("neoforge", LoaderKind::NeoForge),
        ("forge", LoaderKind::Forge),
    ];
    let loader = known
        .iter()
        .find_map(|(name, kind)| {
            dependencies.get(*name).map(|v| Loader {
                kind: *kind,
                version: Some(v.clone()),
            })
        })
        .unwrap_or_default();
    Ok((minecraft, loader))
}

fn curse_loader(id: &str) -> Loader {
    let (name, version) = id.split_once('-').unwrap_or((id, ""));
    let kind = match name {
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        "neoforge" => LoaderKind::NeoForge,
        "forge" => LoaderKind::Forge,
        _ => LoaderKind::Vanilla,
    };
    Loader {
        kind,
        version: (!version.is_empty()).then(|| version.to_string()),
    }
}

/// Files on Modrinth's CDN carry their ids in the URL, which is what lets a
/// pack's mods be updated later like any other.
fn modrinth_source(url: &str) -> Source {
    let parts: Vec<&str> = url.split('/').collect();
    match parts.as_slice() {
        [
            "https:",
            "",
            "cdn.modrinth.com",
            "data",
            project,
            "versions",
            version,
            ..,
        ] => Source::Modrinth {
            project: project.to_string(),
            version: version.to_string(),
        },
        _ => Source::Manual,
    }
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn read_entry<T: serde::de::DeserializeOwned>(pack: &Path, name: &str) -> Result<T> {
    let mut archive = zip::ZipArchive::new(fs::File::open(pack)?)?;
    let entry = archive.by_name(name)?;
    Ok(serde_json::from_reader(entry)?)
}

/// Unpacks the override folders, refusing any entry that would land outside.
fn extract_overrides(pack: &Path, prefixes: &[&str], target: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(pack)?)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.to_string();
        // An empty prefix means the whole archive.
        let inside = |prefix: &&str| {
            if prefix.is_empty() {
                Some(name.as_str())
            } else {
                name.strip_prefix(prefix)?.strip_prefix('/')
            }
        };
        let Some(relative) = prefixes.iter().find_map(inside) else {
            continue;
        };
        let dest = target.join(safe_relative(relative)?);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        io::copy(&mut entry, &mut fs::File::create(dest)?)?;
    }
    Ok(())
}

impl Core {
    /// Installs a modpack from Modrinth or CurseForge as a new instance.
    pub async fn install_pack(
        &self,
        source: ContentSource,
        project: &str,
        minecraft: Option<&str>,
    ) -> Result<PackResult> {
        let task = Task::start(&self.events, "Installing modpack");
        let outcome = self
            .fetch_and_install_pack(source, project, minecraft, &task)
            .await;
        conclude(task, outcome)
    }

    /// Installs a `.mrpack` or CurseForge `.zip` the player already has.
    pub async fn install_pack_file(&self, path: &Path) -> Result<PackResult> {
        let task = Task::start(&self.events, "Installing modpack");
        let outcome = self.install_from_archive(path, None, None, &task).await;
        conclude(task, outcome)
    }

    async fn fetch_and_install_pack(
        &self,
        source: ContentSource,
        project: &str,
        minecraft: Option<&str>,
        task: &Task,
    ) -> Result<PackResult> {
        task.progress(0, 1, "Finding the pack");
        if source == ContentSource::Ftb {
            let (tree, origin, icon) = self.ftb_tree(project, task).await?;
            return self.install_tree(tree, Some(origin), icon, task).await;
        }
        if source == ContentSource::AtLauncher {
            let (tree, origin, icon) = self.atl_tree(project, task).await?;
            return self.install_tree(tree, Some(origin), icon, task).await;
        }
        let (path, origin, icon) = self.download_pack(source, project, minecraft).await?;
        let result = self
            .install_from_archive(&path, Some(origin), icon, task)
            .await;
        let _ = fs::remove_file(&path);
        result
    }

    /// Fetches the newest suitable version of a pack.
    async fn download_pack(
        &self,
        source: ContentSource,
        project: &str,
        minecraft: Option<&str>,
    ) -> Result<(std::path::PathBuf, PackOrigin, Option<String>)> {
        let staged = self.paths.cache().join("downloads");
        match source {
            ContentSource::Modrinth => {
                let details = modrinth::project(&self.net, project).await?;
                let versions = self.pack_versions(&details.id, minecraft).await?;
                let version = modrinth::pick(&versions)
                    .ok_or_else(|| Error::NotFound(format!("a version of {}", details.title)))?;
                let file = version
                    .primary_file()
                    .ok_or_else(|| Error::NotFound(format!("a file for {}", details.title)))?;
                let path = staged.join(&file.filename);
                self.net
                    .fetch(&Fetch {
                        url: file.url.clone(),
                        dest: path.clone(),
                        hash: Expected::Sha512(file.hashes.sha512.clone()),
                        size: Some(file.size),
                    })
                    .await?;
                let origin = PackOrigin {
                    source,
                    project: details.id.clone(),
                    version_id: version.id.clone(),
                    version: version.version_number.clone(),
                };
                Ok((path, origin, details.icon_url))
            }
            ContentSource::Ftb | ContentSource::AtLauncher => {
                Err(Error::Other("these packs aren't single files".into()))
            }
            ContentSource::CurseForge => {
                let id: u64 = project
                    .parse()
                    .map_err(|_| Error::NotFound(format!("CurseForge project {project}")))?;
                let details = curseforge::project(&self.net, id).await?;
                let files = curseforge::files(
                    &self.net,
                    id,
                    minecraft.unwrap_or(""),
                    LoaderKind::Vanilla,
                    ContentKind::Modpack,
                )
                .await?;
                let file = curseforge::pick(&files)
                    .ok_or_else(|| Error::NotFound(format!("a version of {}", details.name)))?;
                let url = file.download_url.clone().ok_or_else(|| {
                    Error::Other(format!(
                        "{}'s author doesn't allow downloads through other apps.",
                        details.name
                    ))
                })?;
                let path = staged.join(&file.file_name);
                self.net
                    .fetch(&Fetch {
                        url,
                        dest: path.clone(),
                        hash: file.sha1().map_or(Expected::Nothing, Expected::Sha1),
                        size: Some(file.file_length),
                    })
                    .await?;
                let origin = PackOrigin {
                    source,
                    project: project.to_string(),
                    version_id: file.id.to_string(),
                    version: file.display_name.clone(),
                };
                Ok((path, origin, details.logo.map(|l| l.url)))
            }
        }
    }

    async fn pack_versions(
        &self,
        project: &str,
        minecraft: Option<&str>,
    ) -> Result<Vec<modrinth::Version>> {
        match minecraft {
            Some(minecraft) => {
                modrinth::versions(
                    &self.net,
                    project,
                    minecraft,
                    LoaderKind::Vanilla,
                    ContentKind::Modpack,
                )
                .await
            }
            None => {
                self.net
                    .json(&format!(
                        "https://api.modrinth.com/v2/project/{project}/version"
                    ))
                    .await
            }
        }
    }

    async fn read_pack(&self, path: &Path, task: &Task) -> Result<PackTree> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let pack = path.to_path_buf();
        if name.ends_with(".mrpack") {
            let index: MrpackIndex =
                blocking(move || read_entry(&pack, "modrinth.index.json")).await?;
            self.read_mrpack(path, index, task).await
        } else {
            let manifest: CurseManifest =
                blocking(move || read_entry(&pack, "manifest.json")).await?;
            self.read_curse_pack(path, manifest, task).await
        }
    }

    async fn read_mrpack(&self, path: &Path, index: MrpackIndex, task: &Task) -> Result<PackTree> {
        let (minecraft, loader) = mrpack_loader(&index.dependencies)?;

        let mut wanted = Vec::new();
        for file in index.files {
            if file
                .env
                .as_ref()
                .is_some_and(|env| env.client == "unsupported")
            {
                continue;
            }
            safe_relative(&file.path)?;
            if file.downloads.iter().any(|url| !host_allowed(url)) {
                return Err(Error::Other(format!(
                    "{} wants a file from a host modpacks may not use",
                    index.name
                )));
            }
            wanted.push(file);
        }

        let staged = self.staging_dir();
        let fetches: Vec<Fetch> = wanted
            .iter()
            .filter_map(|file| {
                Some(Fetch {
                    url: file.downloads.first()?.clone(),
                    dest: staged.join(&file.hashes.sha512[..32]),
                    hash: Expected::Sha512(file.hashes.sha512.clone()),
                    size: Some(file.file_size),
                })
            })
            .collect();
        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading pack files")
            })
            .await?;

        task.progress(0, 1, "Storing pack files");
        let mut files = BTreeMap::new();
        for file in &wanted {
            let stored = self
                .store
                .add(&staged.join(&file.hashes.sha512[..32]), Placement::Link)?;
            let source = file
                .downloads
                .first()
                .map_or(Source::Manual, |url| modrinth_source(url));
            files.insert(
                file.path.clone(),
                TreeFile {
                    path: file.path.clone(),
                    size: stored.size,
                    hash: stored.hash,
                    source,
                    remote_hash: Some(file.hashes.sha512.clone()),
                    name: None,
                    version: None,
                    icon: None,
                },
            );
        }
        let _ = fs::remove_dir_all(&staged);
        self.add_overrides(
            path,
            vec!["overrides".into(), "client-overrides".into()],
            &mut files,
        )
        .await?;

        Ok(PackTree {
            name: index.name,
            minecraft,
            loader,
            files: files.into_values().collect(),
            manual: Vec::new(),
        })
    }

    async fn read_curse_pack(
        &self,
        path: &Path,
        manifest: CurseManifest,
        task: &Task,
    ) -> Result<PackTree> {
        let loader = manifest
            .minecraft
            .mod_loaders
            .first()
            .map_or_else(Loader::default, |l| curse_loader(&l.id));
        let wanted: Vec<&CurseFileRef> = manifest.files.iter().filter(|f| f.required).collect();
        let ids: Vec<u64> = wanted.iter().map(|f| f.file_id).collect();
        let projects: Vec<u64> = wanted.iter().map(|f| f.project_id).collect();

        task.progress(0, 1, "Reading the pack");
        let remote = curseforge::files_by_id(&self.net, &ids).await?;
        let details = curseforge::mods_by_id(&self.net, &projects).await?;
        let names: BTreeMap<u64, &curseforge::Mod> = details.iter().map(|m| (m.id, m)).collect();

        let staged = self.staging_dir();
        let mut fetches = Vec::new();
        let mut manual = Vec::new();
        let mut ready = Vec::new();
        // Many mods an author keeps off third-party apps on CurseForge are on
        // Modrinth too, as the very same file, so those need nothing from the player.
        let blocked: Vec<String> = remote
            .iter()
            .filter(|file| file.download_url.is_none())
            .filter_map(curseforge::File::sha1)
            .collect();
        let mirrored = if blocked.is_empty() {
            Default::default()
        } else {
            modrinth::versions_for_sha1(&self.net, &blocked)
                .await
                .unwrap_or_default()
        };
        let mut from_modrinth = BTreeMap::new();

        for file in remote {
            safe_relative(&file.file_name)?;
            let mirror = file.sha1().and_then(|sha1| {
                let version = mirrored.get(&sha1)?;
                let found = version
                    .files
                    .iter()
                    .find(|f| f.hashes.sha1.as_ref() == Some(&sha1))?;
                Some((version.clone(), found.clone()))
            });
            match (&file.download_url, mirror) {
                (Some(url), _) => {
                    fetches.push(Fetch {
                        url: url.clone(),
                        dest: staged.join(&file.file_name),
                        hash: file.sha1().map_or(Expected::Nothing, Expected::Sha1),
                        size: Some(file.file_length),
                    });
                    ready.push(file);
                }
                (None, Some((version, found))) => {
                    fetches.push(Fetch {
                        url: found.url.clone(),
                        dest: staged.join(&file.file_name),
                        hash: Expected::Sha512(found.hashes.sha512.clone()),
                        size: Some(found.size),
                    });
                    from_modrinth.insert(file.id, (version, found));
                    ready.push(file);
                }
                (None, None) => {
                    let project = names.get(&file.mod_id);
                    manual.push(ManualFile {
                        name: project.map_or(file.display_name.clone(), |p| p.name.clone()),
                        page: project
                            .and_then(|p| p.links.as_ref()?.website_url.clone())
                            .unwrap_or_default(),
                        file: file.file_name.clone(),
                        size: file.file_length,
                        sha1: file.sha1(),
                        project: file.mod_id,
                        file_id: file.id,
                    });
                }
            }
        }
        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading pack files")
            })
            .await?;

        task.progress(0, 1, "Storing pack files");
        let mut files = BTreeMap::new();
        for file in ready {
            let stored = self
                .store
                .add(&staged.join(&file.file_name), Placement::Link)?;
            let path = format!("mods/{}", file.file_name);
            let project = names.get(&file.mod_id);
            files.insert(
                path.clone(),
                TreeFile {
                    path,
                    size: stored.size,
                    hash: stored.hash,
                    source: from_modrinth.get(&file.id).map_or(
                        Source::CurseForge {
                            project: file.mod_id,
                            file: file.id,
                        },
                        |(version, _)| Source::Modrinth {
                            project: version.project_id.clone(),
                            version: version.id.clone(),
                        },
                    ),
                    remote_hash: from_modrinth
                        .get(&file.id)
                        .map(|(_, found)| found.hashes.sha512.clone()),
                    name: project.map(|p| p.name.clone()),
                    version: Some(file.display_name.clone()),
                    icon: project
                        .and_then(|p| p.logo.as_ref())
                        .map(|logo| logo.url.clone()),
                },
            );
        }
        let _ = fs::remove_dir_all(&staged);
        self.add_overrides(path, vec![manifest.overrides.clone()], &mut files)
            .await?;

        Ok(PackTree {
            name: manifest.name,
            minecraft: manifest.minecraft.version,
            loader,
            files: files.into_values().collect(),
            manual,
        })
    }

    fn staging_dir(&self) -> std::path::PathBuf {
        self.paths
            .cache()
            .join("staging")
            .join(uuid::Uuid::new_v4().simple().to_string())
    }

    /// Stores the override folders. Later folders win over earlier ones.
    async fn add_overrides(
        &self,
        pack: &Path,
        prefixes: Vec<String>,
        files: &mut BTreeMap<String, TreeFile>,
    ) -> Result<()> {
        let staged = self.staging_dir();
        let (archive, target) = (pack.to_path_buf(), staged.clone());
        blocking(move || {
            let prefixes: Vec<&str> = prefixes.iter().map(String::as_str).collect();
            extract_overrides(&archive, &prefixes, &target)
        })
        .await?;
        if staged.is_dir() {
            for entry in WalkDir::new(&staged)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let relative = entry
                    .path()
                    .strip_prefix(&staged)
                    .map_err(|e| Error::Other(e.to_string()))?;
                let relative = relative.to_string_lossy().replace('\\', "/");
                let stored = self.store.add(entry.path(), Placement::Copy)?;
                files.insert(
                    relative.clone(),
                    TreeFile {
                        path: relative,
                        size: stored.size,
                        hash: stored.hash,
                        source: Source::Manual,
                        remote_hash: None,
                        name: None,
                        version: None,
                        icon: None,
                    },
                );
            }
        }
        let _ = fs::remove_dir_all(&staged);
        Ok(())
    }

    /// Downloads an ATLauncher pack's newest version that Waymark can install.
    async fn atl_tree(
        &self,
        project: &str,
        task: &Task,
    ) -> Result<(PackTree, PackOrigin, Option<String>)> {
        let pack = self.atl_pack(project).await?;
        let usable = atlauncher::usable(&pack);
        let version = usable.first().ok_or_else(|| {
            Error::Other(format!(
                "{} is too old for Waymark to install yet",
                pack.name
            ))
        })?;
        let config = self.atl_config(&pack.safe_name, &version.version).await?;
        let (minecraft, loader) = atlauncher::game_of(&config)?;

        let staged = self.staging_dir();
        let mut fetches = vec![Fetch {
            url: format!(
                "{}/packs/{}/versions/{}/Configs.zip",
                atlauncher::CDN,
                pack.safe_name,
                version.version
            ),
            dest: staged.join("configs.zip"),
            hash: Expected::Nothing,
            size: None,
        }];
        let mut wanted = Vec::new();
        let mut manual = Vec::new();
        for (position, item) in config.mods.iter().filter(|item| item.wanted()).enumerate() {
            let Some(placement) = item.placement()? else {
                continue;
            };
            match item.address() {
                Some(url) => {
                    let dest = staged.join(position.to_string());
                    fetches.push(Fetch {
                        url,
                        dest: dest.clone(),
                        hash: Expected::Nothing,
                        size: item.filesize,
                    });
                    wanted.push((dest, placement));
                }
                None => manual.push(ManualFile {
                    name: item.name.clone(),
                    page: item.url.clone(),
                    file: item.file.clone(),
                    size: item.filesize.unwrap_or(0),
                    sha1: None,
                    project: 0,
                    file_id: 0,
                }),
            }
        }
        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading pack files")
            })
            .await?;

        task.progress(0, 1, "Storing pack files");
        let mut files = BTreeMap::new();
        self.add_archive(&staged.join("configs.zip"), "", &mut files)
            .await?;
        for (path, placement) in wanted {
            match placement {
                atlauncher::Placement::Unpack(under) => {
                    self.add_archive(&path, &under, &mut files).await?
                }
                atlauncher::Placement::File(location) => {
                    let stored = self.store.add(&path, Placement::for_path(&location))?;
                    files.insert(
                        location.clone(),
                        TreeFile {
                            path: location,
                            size: stored.size,
                            hash: stored.hash,
                            source: Source::Manual,
                            remote_hash: None,
                            name: None,
                            version: None,
                            icon: None,
                        },
                    );
                }
            }
        }
        let _ = fs::remove_dir_all(&staged);
        let origin = PackOrigin {
            source: ContentSource::AtLauncher,
            project: project.to_string(),
            version_id: version.version.clone(),
            version: version.version.clone(),
        };
        let icon = atlauncher::page(&pack).icon;
        Ok((
            PackTree {
                name: pack.name,
                minecraft,
                loader,
                files: files.into_values().collect(),
                manual,
            },
            origin,
            icon,
        ))
    }

    /// Stores everything in a zip, under a folder of the game directory.
    async fn add_archive(
        &self,
        archive: &Path,
        under: &str,
        files: &mut BTreeMap<String, TreeFile>,
    ) -> Result<()> {
        let staged = self.staging_dir();
        let (from, to) = (archive.to_path_buf(), staged.clone());
        blocking(move || extract_overrides(&from, &[""], &to)).await?;
        if staged.is_dir() {
            for entry in WalkDir::new(&staged)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let relative = entry
                    .path()
                    .strip_prefix(&staged)
                    .map_err(|e| Error::Other(e.to_string()))?;
                let relative = relative.to_string_lossy().replace('\\', "/");
                let path = if under.is_empty() {
                    relative
                } else {
                    format!("{under}/{relative}")
                };
                let stored = self.store.add(entry.path(), Placement::Copy)?;
                files.insert(
                    path.clone(),
                    TreeFile {
                        path,
                        size: stored.size,
                        hash: stored.hash,
                        source: Source::Manual,
                        remote_hash: None,
                        name: None,
                        version: None,
                        icon: None,
                    },
                );
            }
        }
        let _ = fs::remove_dir_all(&staged);
        Ok(())
    }

    /// Downloads every file in a Feed The Beast pack's newest version.
    async fn ftb_tree(
        &self,
        project: &str,
        task: &Task,
    ) -> Result<(PackTree, PackOrigin, Option<String>)> {
        let pack = ftb::pack(&self.net, project).await?;
        let version = ftb::newest(&pack)
            .ok_or_else(|| Error::NotFound(format!("a version of {}", pack.name)))?;
        let listing = ftb::version_files(&self.net, project, &version.id.to_string()).await?;
        let (minecraft, loader) = ftb::game_of(&listing.targets)
            .ok_or_else(|| Error::Other(format!("{} doesn't say what game it's for", pack.name)))?;

        let staged = self.staging_dir();
        let mut wanted = Vec::new();
        let mut manual = Vec::new();
        for (position, file) in listing
            .files
            .iter()
            .filter(|file| !file.serveronly)
            .enumerate()
        {
            let path = file.location()?;
            if file.url.is_empty() {
                manual.push(ManualFile {
                    name: file.name.clone(),
                    page: String::new(),
                    file: file.name.clone(),
                    size: file.size,
                    sha1: Some(file.sha1.clone()),
                    project: file.curseforge.as_ref().map_or(0, |cf| cf.project),
                    file_id: file.curseforge.as_ref().map_or(0, |cf| cf.file),
                });
                continue;
            }
            if !file.trusted() {
                return Err(Error::Other(format!(
                    "refused a file from an unknown host: {}",
                    file.url
                )));
            }
            wanted.push((position, path, file));
        }
        let fetches = wanted
            .iter()
            .map(|(position, _, file)| Fetch {
                url: file.url.clone(),
                dest: staged.join(position.to_string()),
                hash: if file.sha1.is_empty() {
                    Expected::Nothing
                } else {
                    Expected::Sha1(file.sha1.clone())
                },
                size: None,
            })
            .collect();
        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading pack files")
            })
            .await?;

        task.progress(0, 1, "Storing pack files");
        let mut files = Vec::with_capacity(wanted.len());
        for (position, path, file) in wanted {
            let stored = self.store.add(
                &staged.join(position.to_string()),
                Placement::for_path(&path),
            )?;
            files.push(TreeFile {
                size: stored.size,
                hash: stored.hash,
                source: file
                    .curseforge
                    .as_ref()
                    .map_or(Source::Manual, |cf| Source::CurseForge {
                        project: cf.project,
                        file: cf.file,
                    }),
                remote_hash: None,
                name: None,
                version: None,
                icon: None,
                path,
            });
        }
        let _ = fs::remove_dir_all(&staged);
        let origin = PackOrigin {
            source: ContentSource::Ftb,
            project: project.to_string(),
            version_id: version.id.to_string(),
            version: version.name.clone(),
        };
        let icon = pack
            .art
            .iter()
            .find(|art| art.kind == "square")
            .map(|art| art.url.clone());
        Ok((
            PackTree {
                name: pack.name,
                minecraft,
                loader,
                files,
                manual,
            },
            origin,
            icon,
        ))
    }

    async fn install_from_archive(
        &self,
        path: &Path,
        origin: Option<PackOrigin>,
        icon: Option<String>,
        task: &Task,
    ) -> Result<PackResult> {
        let tree = self.read_pack(path, task).await?;
        self.install_tree(tree, origin, icon, task).await
    }

    async fn install_tree(
        &self,
        tree: PackTree,
        origin: Option<PackOrigin>,
        icon: Option<String>,
        task: &Task,
    ) -> Result<PackResult> {
        let version_id = self
            .install_loader(
                tree.loader.kind,
                &tree.minecraft,
                tree.loader.version.as_deref().unwrap_or(""),
                task,
            )
            .await?;
        let mut instance = self.register_instance(
            &tree.name,
            &tree.minecraft,
            tree.loader.clone(),
            version_id,
            Some(tree.name.clone()),
        )?;
        instance.icon = icon;
        self.save_instance(&instance)?;

        task.progress(0, 1, "Assembling the instance");
        self.apply_tree(&instance, &tree, None, origin.as_ref())?;
        let mut lock = self.lockfile(&instance.id)?;
        task.progress(0, 1, "Reading mod details");
        self.describe_pack_mods(&mut lock, &self.paths.game(&instance.id))
            .await;
        lock.save(&self.paths.instance(&instance.id))?;
        self.trail(&instance.id)
            .mark("Installed from modpack", WaymarkKind::Import)?;
        self.save_manual_files(&instance.id, &tree.manual)?;
        Ok(PackResult {
            instance,
            manual: tree.manual,
        })
    }

    fn save_manual_files(&self, id: &str, files: &[ManualFile]) -> Result<()> {
        let path = self.paths.instance(id).join(MANUAL_FILES);
        if files.is_empty() {
            let _ = fs::remove_file(path);
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec(files)?)?;
        Ok(())
    }

    /// Pack files still waiting on the player to download them.
    pub fn manual_files(&self, id: &str) -> Result<Vec<ManualFile>> {
        match fs::read(self.paths.instance(id).join(MANUAL_FILES)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Looks through a downloads folder for what's still needed, and installs
    /// whatever matches by hash. Returns what's left.
    pub fn collect_manual_files(&self, id: &str, downloads: &Path) -> Result<Vec<ManualFile>> {
        let waiting = self.manual_files(id)?;
        if waiting.is_empty() {
            return Ok(waiting);
        }
        let sizes: Vec<(u64, std::path::PathBuf)> = fs::read_dir(downloads)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| Some((entry.metadata().ok()?.len(), entry.path())))
                    .collect()
            })
            .unwrap_or_default();
        let mut lock = self.lockfile(id)?;
        let game = self.paths.game(id);
        let mut left = Vec::new();
        let mut found = 0;
        for file in waiting {
            let wanted = file.sha1.as_deref().map(str::to_ascii_lowercase);
            let matching = sizes.iter().find(|(size, path)| {
                *size == file.size && wanted.is_some() && sha1_of(path) == wanted
            });
            let Some((_, path)) = matching else {
                left.push(file);
                continue;
            };
            let stored = self.store.add(path, Placement::Copy)?;
            self.store.place(
                &stored.hash,
                &game.join("mods").join(&file.file),
                Placement::Link,
            )?;
            lock.mods.retain(|entry| entry.file != file.file);
            lock.mods.push(LockedMod {
                name: file.name.clone(),
                file: file.file.clone(),
                hash: stored.hash,
                size: stored.size,
                enabled: true,
                source: Source::CurseForge {
                    project: file.project,
                    file: file.file_id,
                },
                origin: Origin::Manual,
                side: Side::Both,
                version: None,
                icon: None,
                group: None,
                tags: Vec::new(),
                requires: Vec::new(),
                remote_hash: None,
            });
            found += 1;
        }
        if found > 0 {
            lock.save(&self.paths.instance(id))?;
            self.save_manual_files(id, &left)?;
            self.trail(id).mark(
                &format!("Picked up {found} downloaded files"),
                WaymarkKind::Install,
            )?;
        }
        Ok(left)
    }

    /// The store project an instance's modpack came from, as `source:id`.
    pub fn pack_project(&self, id: &str) -> Option<String> {
        let origin = self.pack_state(id)?.origin;
        let source = serde_json::to_value(origin.source).ok()?;
        Some(format!("{}:{}", source.as_str()?, origin.project))
    }

    fn pack_state(&self, id: &str) -> Option<PackState> {
        serde_json::from_slice(&fs::read(self.paths.instance(id).join(PACK_STATE)).ok()?).ok()
    }

    /// Looks for a newer version of the pack this instance came from.
    pub async fn check_pack_update(&self, id: &str) -> Result<Option<PackUpdate>> {
        let Some(state) = self.pack_state(id) else {
            return Ok(None);
        };
        let origin = &state.origin;
        let latest = match origin.source {
            ContentSource::Modrinth => {
                let versions = self.pack_versions(&origin.project, None).await?;
                let Some(newest) = modrinth::pick(&versions) else {
                    return Ok(None);
                };
                // Versions arrive newest first, so a pick after ours isn't an update.
                let ours = versions.iter().position(|v| v.id == origin.version_id);
                let theirs = versions.iter().position(|v| v.id == newest.id);
                if ours.is_some_and(|o| theirs.is_none_or(|t| o <= t)) {
                    return Ok(None);
                }
                newest.version_number.clone()
            }
            ContentSource::AtLauncher => {
                let pack = self.atl_pack(&origin.project).await?;
                let usable = atlauncher::usable(&pack);
                let Some(newest) = usable.first() else {
                    return Ok(None);
                };
                if newest.version == origin.version_id {
                    return Ok(None);
                }
                newest.version.clone()
            }
            ContentSource::Ftb => {
                let pack = ftb::pack(&self.net, &origin.project).await?;
                let Some(newest) = ftb::newest(&pack) else {
                    return Ok(None);
                };
                if origin
                    .version_id
                    .parse::<u64>()
                    .is_ok_and(|ours| ours >= newest.id)
                {
                    return Ok(None);
                }
                newest.name.clone()
            }
            ContentSource::CurseForge => {
                let id: u64 = origin
                    .project
                    .parse()
                    .map_err(|_| Error::NotFound("that pack".into()))?;
                let files =
                    curseforge::files(&self.net, id, "", LoaderKind::Vanilla, ContentKind::Modpack)
                        .await?;
                let Some(newest) = curseforge::pick(&files) else {
                    return Ok(None);
                };
                if newest.id.to_string() == origin.version_id {
                    return Ok(None);
                }
                newest.display_name.clone()
            }
        };
        Ok(Some(PackUpdate {
            name: state.name,
            current: origin.version.clone(),
            latest,
        }))
    }

    /// Updates an instance to the newest version of its pack. The player's own
    /// mods, edited files and removals survive, and one waymark undoes it all.
    pub async fn update_pack(&self, id: &str) -> Result<PackUpdateReport> {
        let state = self
            .pack_state(id)
            .ok_or_else(|| Error::NotFound("a modpack for this instance".into()))?;
        if self.game_running(id) {
            return Err(Error::Other(
                "close the game before updating this pack".into(),
            ));
        }
        let task = Task::start(&self.events, format!("Updating {}", state.name));
        let outcome = self.run_pack_update(id, state, &task).await;
        match outcome {
            Ok(report) => {
                task.finish();
                Ok(report)
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    async fn run_pack_update(
        &self,
        id: &str,
        state: PackState,
        task: &Task,
    ) -> Result<PackUpdateReport> {
        task.progress(0, 1, "Downloading the new version");
        let (tree, origin) = if state.origin.source == ContentSource::Ftb {
            let (tree, origin, _) = self.ftb_tree(&state.origin.project, task).await?;
            (tree, origin)
        } else if state.origin.source == ContentSource::AtLauncher {
            let (tree, origin, _) = self.atl_tree(&state.origin.project, task).await?;
            (tree, origin)
        } else {
            let (path, origin, _) = self
                .download_pack(state.origin.source, &state.origin.project, None)
                .await?;
            let tree = self.read_pack(&path, task).await;
            let _ = fs::remove_file(&path);
            (tree?, origin)
        };

        self.trail(id).mark(
            &format!("Before updating {}", state.name),
            WaymarkKind::Automatic,
        )?;

        let mut instance = self.instance(id)?;
        let minecraft_changed = instance.minecraft != tree.minecraft;
        if minecraft_changed || instance.loader != tree.loader {
            instance.version_id = self
                .install_loader(
                    tree.loader.kind,
                    &tree.minecraft,
                    tree.loader.version.as_deref().unwrap_or(""),
                    task,
                )
                .await?;
            instance.minecraft = tree.minecraft.clone();
            instance.loader = tree.loader.clone();
            self.save_instance(&instance)?;
        }

        task.progress(0, 1, "Merging your changes");
        let mut report = self.apply_tree(&instance, &tree, Some(&state), Some(&origin))?;
        report.minecraft_changed = minecraft_changed;
        self.save_manual_files(id, &tree.manual)?;
        report.manual = tree.manual;
        report.version = origin.version.clone();

        let mut lock = self.lockfile(id)?;
        lock.minecraft = instance.minecraft.clone();
        lock.loader = instance.loader.clone();
        self.describe_pack_mods(&mut lock, &self.paths.game(id))
            .await;
        lock.save(&self.paths.instance(id))?;
        self.trail(id).mark(
            &format!("Updated {} to {}", state.name, origin.version),
            WaymarkKind::Update,
        )?;
        Ok(report)
    }

    /// Merges a pack tree onto an instance and records it as the new base.
    fn apply_tree(
        &self,
        instance: &Instance,
        tree: &PackTree,
        base: Option<&PackState>,
        origin: Option<&PackOrigin>,
    ) -> Result<PackUpdateReport> {
        let game = self.paths.game(&instance.id);
        let instance_dir = self.paths.instance(&instance.id);
        let mut lock = self.lockfile(&instance.id)?;
        lock.reconcile(&game, &self.store)?;

        let theirs: BTreeMap<String, &TreeFile> = tree.files.iter().map(|f| (f.key(), f)).collect();
        let empty = BTreeMap::new();
        let base_files = base.map_or(&empty, |state| &state.files);

        // Where each file is on disk now. Mods are found through the lockfile,
        // because a mod the player updated themselves has a new file name.
        let mut on_disk: BTreeMap<String, (String, bool)> = BTreeMap::new();
        for entry in &lock.mods {
            if let Some(key) = project_key(&entry.source) {
                on_disk.insert(key, (format!("mods/{}", entry.file), !entry.enabled));
            }
        }
        for (key, path) in base_files
            .iter()
            .map(|(k, f)| (k, &f.path))
            .chain(theirs.iter().map(|(k, f)| (k, &f.path)))
        {
            if on_disk.contains_key(key) {
                continue;
            }
            if game.join(path).is_file() {
                on_disk.insert(key.clone(), (path.clone(), false));
            } else if game.join(format!("{path}.disabled")).is_file() {
                on_disk.insert(key.clone(), (path.clone(), true));
            }
        }

        let mut ours = BTreeMap::new();
        for (key, (path, disabled)) in &on_disk {
            let actual = if *disabled {
                format!("{path}.disabled")
            } else {
                path.clone()
            };
            if let Ok(stored) = hash_file(&game.join(actual)) {
                ours.insert(key.clone(), stored.hash);
            }
        }
        let base_hashes: BTreeMap<String, String> = base_files
            .iter()
            .map(|(k, f)| (k.clone(), f.hash.clone()))
            .collect();
        let their_hashes: BTreeMap<String, String> = theirs
            .iter()
            .map(|(k, f)| (k.clone(), f.hash.clone()))
            .collect();
        let merge = plan_merge(&base_hashes, &ours, &their_hashes);

        let pack_name = tree.name.clone();
        let mut report = PackUpdateReport::default();
        for key in &merge.delete {
            if let Some((path, disabled)) = on_disk.get(key) {
                let actual = if *disabled {
                    format!("{path}.disabled")
                } else {
                    path.clone()
                };
                let _ = fs::remove_file(game.join(&actual));
                lock.mods.retain(|m| format!("mods/{}", m.file) != *path);
                report.removed += 1;
            }
        }
        for key in &merge.place {
            let Some(file) = theirs.get(key) else {
                continue;
            };
            let previous = on_disk.get(key).cloned();
            let disabled = previous.as_ref().is_some_and(|(_, off)| *off);
            if let Some((path, off)) = &previous {
                let actual = if *off {
                    format!("{path}.disabled")
                } else {
                    path.clone()
                };
                let _ = fs::remove_file(game.join(actual));
                report.updated += 1;
            } else {
                report.added += 1;
            }
            let target = if disabled {
                format!("{}.disabled", file.path)
            } else {
                file.path.clone()
            };
            self.store.place(
                &file.hash,
                &game.join(safe_relative(&target)?),
                Placement::for_path(&file.path),
            )?;

            if !file.path.starts_with("mods/") {
                continue;
            }
            let old = previous.as_ref().and_then(|(path, _)| {
                let name = file_name(path);
                lock.mods.iter().position(|m| m.file == name)
            });
            let entry = pack_mod(file, &pack_name);
            match old {
                Some(index) => {
                    let current = &mut lock.mods[index];
                    current.file = entry.file;
                    current.hash = entry.hash;
                    current.size = entry.size;
                    current.source = entry.source;
                    current.remote_hash = entry.remote_hash;
                    current.version = entry.version;
                    current.name = entry.name;
                    current.icon = entry.icon.or(current.icon.take());
                }
                None => {
                    let mut entry = entry;
                    entry.enabled = !disabled;
                    lock.mods.push(entry);
                }
            }
        }

        let describe = |keys: &[String]| -> Vec<String> {
            keys.iter()
                .filter_map(|key| {
                    let path = on_disk
                        .get(key)
                        .map(|(p, _)| p.clone())
                        .or_else(|| base_files.get(key).map(|f| f.path.clone()))?;
                    Some(path)
                })
                .collect()
        };
        report.kept = describe(&merge.kept);
        report.stayed_removed = describe(&merge.stayed_removed);

        // Mods the pack kept unchanged but this instance has never tracked.
        for file in tree.files.iter().filter(|f| f.path.starts_with("mods/")) {
            let name = file_name(&file.path);
            let known = lock.mods.iter().any(|m| m.file == name);
            if !known && game.join(&file.path).is_file() {
                lock.mods.push(pack_mod(file, &pack_name));
            }
        }
        lock.reconcile(&game, &self.store)?;
        lock.save(&instance_dir)?;

        if let Some(origin) = origin {
            let files = theirs
                .iter()
                .map(|(key, file)| {
                    (
                        key.clone(),
                        InstalledFile {
                            path: file.path.clone(),
                            hash: file.hash.clone(),
                        },
                    )
                })
                .collect();
            let state = PackState {
                name: pack_name,
                origin: origin.clone(),
                files,
            };
            fs::write(instance_dir.join(PACK_STATE), serde_json::to_vec(&state)?)?;
        }
        Ok(report)
    }

    /// Gives pack mods their real names, versions and icons. Packs only list
    /// file paths, so these come from Modrinth in bulk, then from the jars
    /// themselves for anything Modrinth doesn't know. A pack still works if
    /// this fails, so errors just leave the plain file name in place.
    async fn describe_pack_mods(&self, lock: &mut Lockfile, game: &Path) {
        let mut projects = Vec::new();
        let mut versions = Vec::new();
        for entry in &lock.mods {
            if let Source::Modrinth { project, version } = &entry.source {
                projects.push(project.clone());
                versions.push(version.clone());
            }
        }
        let mut known_projects = BTreeMap::new();
        let mut known_versions = BTreeMap::new();
        for chunk in projects.chunks(LOOKUP_BATCH) {
            if let Ok(found) = modrinth::projects(&self.net, chunk).await {
                known_projects.extend(found.into_iter().map(|p| (p.id.clone(), p)));
            }
        }
        for chunk in versions.chunks(LOOKUP_BATCH) {
            if let Ok(found) = modrinth::versions_by_id(&self.net, chunk).await {
                known_versions.extend(found.into_iter().map(|v| (v.id.clone(), v.version_number)));
            }
        }
        for entry in &mut lock.mods {
            if let Source::Modrinth { project, version } = &entry.source {
                if let Some(details) = known_projects.get(project) {
                    entry.name = details.title.clone();
                    entry.icon = details.icon_url.clone();
                    entry.side = side_from(details);
                }
                if let Some(number) = known_versions.get(version) {
                    entry.version = Some(number.clone());
                }
            }
            if entry.version.is_none()
                && let Some(info) = crate::modinfo::read(&game.join("mods").join(entry.disk_name()))
            {
                if let (Source::Manual, Some(name)) = (&entry.source, info.name) {
                    entry.name = name;
                }
                entry.version = info.version;
            }
        }
    }
}

fn sha1_of(path: &Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(hex::encode(hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

fn pack_mod(file: &TreeFile, pack: &str) -> LockedMod {
    let name = file_name(&file.path);
    LockedMod {
        name: file
            .name
            .clone()
            .unwrap_or_else(|| name.trim_end_matches(".jar").replace(['-', '_'], " ")),
        file: name,
        hash: file.hash.clone(),
        size: file.size,
        enabled: true,
        source: file.source.clone(),
        origin: Origin::Pack {
            name: pack.to_string(),
        },
        side: Side::Both,
        version: file.version.clone(),
        icon: file.icon.clone(),
        group: None,
        tags: Vec::new(),
        requires: Vec::new(),
        remote_hash: file.remote_hash.clone(),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::Other(e.to_string()))?
}

fn conclude(task: Task, outcome: Result<PackResult>) -> Result<PackResult> {
    match outcome {
        Ok(result) => {
            task.finish();
            Ok(result)
        }
        Err(error) => {
            task.fail(error.to_string());
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn dependencies(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        dependencies(pairs)
    }

    #[test]
    fn only_the_format_s_allowed_hosts_are_accepted() {
        assert!(host_allowed(
            "https://cdn.modrinth.com/data/a/versions/b/c.jar"
        ));
        assert!(host_allowed(
            "https://github.com/x/y/releases/download/1/z.jar"
        ));
        assert!(!host_allowed("http://cdn.modrinth.com/data/a.jar"));
        assert!(!host_allowed("https://cdn.modrinth.com.evil.example/a.jar"));
        assert!(!host_allowed("https://evil.example/cdn.modrinth.com/a.jar"));
    }

    #[test]
    fn downloaded_files_are_picked_up_by_hash_whatever_they_are_called() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().join("data"), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let downloads = dir.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        let wanted = |name: &str, sha1: &str, size: u64| ManualFile {
            name: name.into(),
            page: "https://www.curseforge.com/minecraft/mc-mods/x".into(),
            file: format!("{name}.jar"),
            size,
            sha1: Some(sha1.into()),
            project: 1,
            file_id: 2,
        };
        // A browser renames a repeat download, and a same-sized stranger mustn't count.
        fs::write(downloads.join("real (1).jar"), b"the real jar").unwrap();
        fs::write(downloads.join("stranger.jar"), b"not that jar").unwrap();
        let sha1 = sha1_of(&downloads.join("real (1).jar")).unwrap();
        core.save_manual_files(
            &instance.id,
            &[wanted("real", &sha1, 12), wanted("other", "00", 12)],
        )
        .unwrap();

        let left = core.collect_manual_files(&instance.id, &downloads).unwrap();

        assert_eq!(left.len(), 1);
        assert_eq!(left[0].name, "other");
        assert_eq!(core.manual_files(&instance.id).unwrap().len(), 1);
        assert!(
            core.paths
                .game(&instance.id)
                .join("mods/real.jar")
                .is_file()
        );
        assert!(
            downloads.join("real (1).jar").is_file(),
            "the player's download stays where it is"
        );
        let lock = core.lockfile(&instance.id).unwrap();
        assert_eq!(
            lock.mods[0].source,
            Source::CurseForge {
                project: 1,
                file: 2
            }
        );
    }

    #[test]
    fn reads_the_loader_from_mrpack_dependencies() {
        let (minecraft, loader) = mrpack_loader(&dependencies(&[
            ("minecraft", "1.21.4"),
            ("fabric-loader", "0.16.9"),
        ]))
        .unwrap();
        assert_eq!(minecraft, "1.21.4");
        assert_eq!(
            loader,
            Loader {
                kind: LoaderKind::Fabric,
                version: Some("0.16.9".into())
            }
        );
    }

    #[test]
    fn a_vanilla_pack_has_no_loader() {
        let (_, loader) = mrpack_loader(&dependencies(&[("minecraft", "1.21.4")])).unwrap();
        assert_eq!(loader.kind, LoaderKind::Vanilla);
    }

    #[test]
    fn a_pack_without_a_minecraft_version_is_rejected() {
        assert!(mrpack_loader(&dependencies(&[("fabric-loader", "0.16.9")])).is_err());
    }

    #[test]
    fn reads_curseforge_loader_ids() {
        assert_eq!(curse_loader("neoforge-21.1.77").kind, LoaderKind::NeoForge);
        assert_eq!(curse_loader("forge-47.3.0").version.unwrap(), "47.3.0");
    }

    #[test]
    fn recognises_modrinth_cdn_links() {
        let source =
            modrinth_source("https://cdn.modrinth.com/data/AANobbMI/versions/abc123/sodium.jar");
        assert_eq!(
            source,
            Source::Modrinth {
                project: "AANobbMI".into(),
                version: "abc123".into()
            }
        );
        assert_eq!(
            modrinth_source("https://github.com/x/y.jar"),
            Source::Manual
        );
    }

    #[test]
    fn overrides_cannot_escape_their_folder() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("evil.mrpack");
        let mut zip = zip::ZipWriter::new(fs::File::create(&pack).unwrap());
        zip.start_file(
            "overrides/../../escaped.txt",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"gotcha").unwrap();
        zip.finish().unwrap();

        let game = dir.path().join("instance/game");
        fs::create_dir_all(&game).unwrap();
        assert!(extract_overrides(&pack, &["overrides"], &game).is_err());
        assert!(!dir.path().join("escaped.txt").exists());
    }

    #[test]
    fn overrides_land_in_their_folder() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("good.mrpack");
        let mut zip = zip::ZipWriter::new(fs::File::create(&pack).unwrap());
        zip.start_file(
            "overrides/config/a.toml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"setting = true").unwrap();
        zip.start_file(
            "modrinth.index.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();

        let game = dir.path().join("game");
        extract_overrides(&pack, &["overrides"], &game).unwrap();
        assert_eq!(
            fs::read(game.join("config/a.toml")).unwrap(),
            b"setting = true"
        );
        assert!(!game.join("modrinth.index.json").exists());
    }

    #[test]
    fn a_fresh_install_places_everything() {
        let merge = plan_merge(&map(&[]), &map(&[]), &map(&[("a", "1"), ("b", "2")]));
        assert_eq!(merge.place, ["a", "b"]);
    }

    #[test]
    fn an_untouched_file_takes_the_new_version() {
        let merge = plan_merge(
            &map(&[("a", "1")]),
            &map(&[("a", "1")]),
            &map(&[("a", "2")]),
        );
        assert_eq!(merge.place, ["a"]);
        assert!(merge.kept.is_empty());
    }

    #[test]
    fn an_edited_file_is_kept_when_the_pack_also_changed_it() {
        let merge = plan_merge(
            &map(&[("config", "1")]),
            &map(&[("config", "mine")]),
            &map(&[("config", "2")]),
        );
        assert_eq!(merge.kept, ["config"]);
        assert!(merge.place.is_empty());
    }

    #[test]
    fn an_edited_file_stays_quietly_when_the_pack_did_not_change_it() {
        let merge = plan_merge(
            &map(&[("config", "1")]),
            &map(&[("config", "mine")]),
            &map(&[("config", "1")]),
        );
        assert_eq!(merge, Merge::default());
    }

    #[test]
    fn a_file_the_player_removed_stays_removed() {
        let merge = plan_merge(&map(&[("a", "1")]), &map(&[]), &map(&[("a", "2")]));
        assert_eq!(merge.stayed_removed, ["a"]);
        assert!(merge.place.is_empty());
    }

    #[test]
    fn a_file_dropped_from_the_pack_is_deleted_if_untouched() {
        let merge = plan_merge(&map(&[("a", "1")]), &map(&[("a", "1")]), &map(&[]));
        assert_eq!(merge.delete, ["a"]);
    }

    #[test]
    fn a_file_dropped_from_the_pack_is_kept_if_edited() {
        let merge = plan_merge(&map(&[("a", "1")]), &map(&[("a", "mine")]), &map(&[]));
        assert_eq!(merge.kept, ["a"]);
        assert!(merge.delete.is_empty());
    }

    #[test]
    fn identical_changes_on_both_sides_need_no_work() {
        let merge = plan_merge(
            &map(&[("a", "1")]),
            &map(&[("a", "2")]),
            &map(&[("a", "2")]),
        );
        assert_eq!(merge, Merge::default());
    }

    #[test]
    fn a_new_pack_file_never_overwrites_one_the_player_already_has() {
        let merge = plan_merge(&map(&[]), &map(&[("a", "mine")]), &map(&[("a", "2")]));
        assert_eq!(merge.kept, ["a"]);
        assert!(merge.place.is_empty());
    }

    #[test]
    fn the_player_s_own_mods_are_never_considered() {
        let merge = plan_merge(
            &map(&[("pack", "1")]),
            &map(&[("pack", "1"), ("mine", "9")]),
            &map(&[("pack", "1")]),
        );
        assert_eq!(merge, Merge::default());
    }

    fn tree_file(path: &str, content: &str, core: &Core, dir: &Path, source: Source) -> TreeFile {
        let scratch = dir.join("scratch-file");
        fs::write(&scratch, content).unwrap();
        let stored = core.store.add(&scratch, Placement::Copy).unwrap();
        TreeFile {
            path: path.into(),
            size: stored.size,
            hash: stored.hash,
            source,
            remote_hash: None,
            name: None,
            version: None,
            icon: None,
        }
    }

    fn origin(version: &str) -> PackOrigin {
        PackOrigin {
            source: ContentSource::Modrinth,
            project: "pack".into(),
            version_id: version.into(),
            version: version.into(),
        }
    }

    fn tree(files: Vec<TreeFile>) -> PackTree {
        PackTree {
            name: "Pack".into(),
            minecraft: "1.21.4".into(),
            loader: Loader::default(),
            files,
            manual: Vec::new(),
        }
    }

    fn modrinth(project: &str, version: &str) -> Source {
        Source::Modrinth {
            project: project.into(),
            version: version.into(),
        }
    }

    #[test]
    fn an_update_keeps_the_player_s_additions_edits_and_switches() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let game = core.paths.game(&instance.id);

        let first = tree(vec![
            tree_file(
                "mods/sodium-1.jar",
                "sodium one",
                &core,
                dir.path(),
                modrinth("sodium", "1"),
            ),
            tree_file(
                "mods/lithium-1.jar",
                "lithium one",
                &core,
                dir.path(),
                modrinth("lithium", "1"),
            ),
            tree_file(
                "config/pack.toml",
                "pack config",
                &core,
                dir.path(),
                Source::Manual,
            ),
            tree_file(
                "config/mine.toml",
                "original",
                &core,
                dir.path(),
                Source::Manual,
            ),
        ]);
        core.apply_tree(&instance, &first, None, Some(&origin("1")))
            .unwrap();

        // The player adds a mod, edits a config, and switches a pack mod off.
        fs::write(game.join("mods/own.jar"), b"my own mod").unwrap();
        fs::write(game.join("config/mine.toml"), b"edited by me").unwrap();
        let mut lock = core.lockfile(&instance.id).unwrap();
        lock.reconcile(&game, &core.store).unwrap();
        lock.save(&core.paths.instance(&instance.id)).unwrap();
        let lithium = lock
            .mods
            .iter()
            .find(|m| m.file.starts_with("lithium"))
            .unwrap()
            .hash
            .clone();
        core.set_mod_enabled(&instance.id, &lithium, false).unwrap();

        let second = tree(vec![
            tree_file(
                "mods/sodium-2.jar",
                "sodium two",
                &core,
                dir.path(),
                modrinth("sodium", "2"),
            ),
            tree_file(
                "mods/lithium-2.jar",
                "lithium two",
                &core,
                dir.path(),
                modrinth("lithium", "2"),
            ),
            tree_file(
                "config/pack.toml",
                "new pack config",
                &core,
                dir.path(),
                Source::Manual,
            ),
            tree_file(
                "config/mine.toml",
                "pack changed this too",
                &core,
                dir.path(),
                Source::Manual,
            ),
        ]);
        let state = core.pack_state(&instance.id).unwrap();
        let report = core
            .apply_tree(&instance, &second, Some(&state), Some(&origin("2")))
            .unwrap();

        assert_eq!(report.updated, 3);
        assert_eq!(report.kept, ["config/mine.toml"]);
        assert_eq!(
            fs::read(game.join("mods/sodium-2.jar")).unwrap(),
            b"sodium two"
        );
        assert!(!game.join("mods/sodium-1.jar").exists());
        assert_eq!(fs::read(game.join("mods/own.jar")).unwrap(), b"my own mod");
        assert_eq!(
            fs::read(game.join("config/mine.toml")).unwrap(),
            b"edited by me"
        );
        assert_eq!(
            fs::read(game.join("config/pack.toml")).unwrap(),
            b"new pack config"
        );
        assert!(
            game.join("mods/lithium-2.jar.disabled").is_file(),
            "a switched-off mod stays off across versions"
        );

        let lock = core.lockfile(&instance.id).unwrap();
        assert_eq!(lock.mods.len(), 3);
        assert!(
            lock.mods
                .iter()
                .any(|m| m.file == "own.jar" && m.origin == Origin::Manual)
        );
        assert!(
            lock.mods
                .iter()
                .any(|m| m.file == "lithium-2.jar" && !m.enabled)
        );
    }

    #[test]
    fn a_mod_removed_by_the_player_does_not_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();

        let first = tree(vec![tree_file(
            "mods/ads-1.jar",
            "one",
            &core,
            dir.path(),
            modrinth("ads", "1"),
        )]);
        core.apply_tree(&instance, &first, None, Some(&origin("1")))
            .unwrap();
        let hash = core.lockfile(&instance.id).unwrap().mods[0].hash.clone();
        core.remove_mod(&instance.id, &hash).unwrap();

        let second = tree(vec![tree_file(
            "mods/ads-2.jar",
            "two",
            &core,
            dir.path(),
            modrinth("ads", "2"),
        )]);
        let state = core.pack_state(&instance.id).unwrap();
        let report = core
            .apply_tree(&instance, &second, Some(&state), Some(&origin("2")))
            .unwrap();

        assert_eq!(report.stayed_removed.len(), 1);
        assert!(
            !core
                .paths
                .game(&instance.id)
                .join("mods/ads-2.jar")
                .exists()
        );
    }

    #[test]
    fn a_mod_the_player_updated_themselves_is_left_at_their_version() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let game = core.paths.game(&instance.id);

        let first = tree(vec![tree_file(
            "mods/iris-1.jar",
            "one",
            &core,
            dir.path(),
            modrinth("iris", "1"),
        )]);
        core.apply_tree(&instance, &first, None, Some(&origin("1")))
            .unwrap();

        fs::remove_file(game.join("mods/iris-1.jar")).unwrap();
        fs::write(game.join("mods/iris-9.jar"), b"newer than the pack").unwrap();
        let mut lock = core.lockfile(&instance.id).unwrap();
        lock.mods[0].file = "iris-9.jar".into();
        lock.save(&core.paths.instance(&instance.id)).unwrap();

        let second = tree(vec![tree_file(
            "mods/iris-2.jar",
            "two",
            &core,
            dir.path(),
            modrinth("iris", "2"),
        )]);
        let state = core.pack_state(&instance.id).unwrap();
        let report = core
            .apply_tree(&instance, &second, Some(&state), Some(&origin("2")))
            .unwrap();

        assert_eq!(report.kept, ["mods/iris-9.jar"]);
        assert!(game.join("mods/iris-9.jar").is_file());
        assert!(!game.join("mods/iris-2.jar").exists());
    }
}

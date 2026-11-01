//! Installing, removing and updating what's in an instance, from either source.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Core;
use crate::curseforge;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::install::safe_relative;
use crate::instance::Instance;
use crate::lock::{LockedMod, Lockfile, Origin, SHELVES, Side, Source, content_folder};
use crate::modrinth;
use crate::net::{Expected, Fetch};
use crate::store::Placement;
use crate::waymarks::{WaymarkKind, WaymarkSummary};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    Modrinth,
    CurseForge,
    /// Feed The Beast, which only has modpacks.
    Ftb,
    /// ATLauncher, which only has modpacks too.
    #[serde(rename = "atlauncher")]
    AtLauncher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Mod,
    Modpack,
    ResourcePack,
    Shader,
    DataPack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    #[serde(default)]
    pub text: String,
    pub kind: ContentKind,
    #[serde(default)]
    pub minecraft: Option<String>,
    #[serde(default)]
    pub loader: Option<crate::lock::LoaderKind>,
    #[serde(default)]
    pub sort: Sort,
    /// Every one of these must match. They're ids from `search_categories`.
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "page_size")]
    pub limit: u32,
}

fn page_size() -> u32 {
    24
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentHit {
    pub source: ContentSource,
    pub id: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub author: String,
    pub downloads: u64,
    pub icon: Option<String>,
    pub banner: Option<String>,
    pub kind: ContentKind,
    pub categories: Vec<String>,
}

/// Something a search can be narrowed to.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCategory {
    pub id: String,
    pub name: String,
    pub group: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentPage {
    pub hits: Vec<ContentHit>,
    pub total: u64,
}

/// One file to update. Without a `version`, it goes to the newest.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePick {
    pub hash: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// A file about to take another's place.
struct Replacement {
    kind: ContentKind,
    old_hash: String,
    file: String,
    url: String,
    expected: Expected,
    size: u64,
    version: String,
    source: Source,
    remote_hash: Option<String>,
    needs: Vec<String>,
}

/// What the player asked to install.
struct Wanted<'a> {
    source: ContentSource,
    project: &'a str,
    kind: ContentKind,
    version: Option<&'a str>,
    world: Option<&'a str>,
}

/// A file about to be installed.
struct Planned {
    kind: ContentKind,
    name: String,
    file: String,
    url: String,
    hash: Expected,
    size: u64,
    source: Source,
    origin: Origin,
    version: String,
    icon: Option<String>,
    side: Side,
    project_key: String,
    requires: Vec<String>,
    remote_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    pub kind: ContentKind,
    pub hash: String,
    pub name: String,
    pub source: ContentSource,
    pub project: String,
    pub current: Option<String>,
    pub latest: String,
    /// Published too recently to install yet, under the release delay setting.
    pub held: bool,
}

pub(crate) fn project_key(source: &Source) -> Option<String> {
    match source {
        Source::Modrinth { project, .. } => Some(format!("modrinth:{project}")),
        Source::CurseForge { project, .. } => Some(format!("curseforge:{project}")),
        Source::Manual => None,
    }
}

/// What's already here, so nothing is installed twice. A data pack only counts
/// in its own world, because each world can have its own copy.
fn installed_keys(lock: &Lockfile, kind: ContentKind, world: Option<&str>) -> HashSet<String> {
    lock.everything()
        .filter(|(shelf, entry)| {
            *shelf != ContentKind::DataPack
                || (kind == ContentKind::DataPack && entry.group.as_deref() == world)
        })
        .filter_map(|(_, entry)| project_key(&entry.source))
        .collect()
}

pub(crate) fn side_from(project: &modrinth::Project) -> Side {
    let client_only = project.server_side.as_deref() == Some("unsupported");
    let server_only = project.client_side.as_deref() == Some("unsupported");
    match (client_only, server_only) {
        (true, false) => Side::Client,
        (false, true) => Side::Server,
        _ => Side::Both,
    }
}

impl Core {
    pub async fn search_content(
        &self,
        source: ContentSource,
        query: &SearchQuery,
    ) -> Result<ContentPage> {
        match source {
            ContentSource::Modrinth => modrinth::search(&self.net, query).await,
            ContentSource::CurseForge => curseforge::search(&self.net, query).await,
            ContentSource::Ftb => crate::ftb::search(&self.net, query).await,
            ContentSource::AtLauncher => self.atl_search(query).await,
        }
    }

    pub async fn search_categories(
        &self,
        source: ContentSource,
        kind: ContentKind,
    ) -> Result<Vec<SearchCategory>> {
        match source {
            ContentSource::Modrinth => modrinth::categories(&self.net, kind).await,
            ContentSource::CurseForge => curseforge::categories(&self.net, kind).await,
            ContentSource::Ftb | ContentSource::AtLauncher => Ok(Vec::new()),
        }
    }

    /// Every project in an instance, as `source:id`.
    pub fn installed_projects(&self, id: &str) -> Result<Vec<String>> {
        Ok(self
            .lockfile(id)?
            .everything()
            .filter_map(|(_, entry)| project_key(&entry.source))
            .collect())
    }

    /// One kind of content in an instance, after noticing changes made by hand.
    pub fn instance_content(&self, id: &str, kind: ContentKind) -> Result<Vec<LockedMod>> {
        let mut lock = self.lockfile(id)?;
        if lock.reconcile(&self.paths.game(id), &self.store)? {
            lock.save(&self.paths.instance(id))?;
        }
        Ok(lock.shelf(kind).to_vec())
    }

    /// A world's folder name, checked so a data pack can't land anywhere else.
    fn world_folder(&self, id: &str, world: Option<&str>) -> Result<String> {
        let world = world.ok_or_else(|| {
            Error::Other("data packs install into a world, so choose one first".into())
        })?;
        let single = safe_relative(world)?.components().count() == 1;
        if !single || !self.paths.game(id).join("saves").join(world).is_dir() {
            return Err(Error::NotFound(format!("the world {world}")));
        }
        Ok(world.to_string())
    }

    /// A world's data packs.
    pub fn data_packs(&self, id: &str, world: &str) -> Result<Vec<LockedMod>> {
        let packs = self.instance_content(id, ContentKind::DataPack)?;
        Ok(packs
            .into_iter()
            .filter(|pack| pack.group.as_deref() == Some(world))
            .collect())
    }

    /// Installs a project and everything it requires, as one waymark. Without a
    /// `version`, the best one for the instance is chosen.
    pub async fn install_content(
        &self,
        id: &str,
        source: ContentSource,
        project: &str,
        kind: ContentKind,
        version: Option<&str>,
        world: Option<&str>,
    ) -> Result<Option<WaymarkSummary>> {
        let instance = self.instance(id)?;
        let task = Task::start(&self.events, format!("Installing to {}", instance.name));
        let wanted = Wanted {
            source,
            project,
            kind,
            version,
            world,
        };
        match self.install_planned(&instance, &wanted, &task).await {
            Ok(mark) => {
                task.finish();
                Ok(mark)
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    async fn install_planned(
        &self,
        instance: &Instance,
        wanted: &Wanted<'_>,
        task: &Task,
    ) -> Result<Option<WaymarkSummary>> {
        let Wanted {
            source,
            project,
            kind,
            version,
            ..
        } = *wanted;
        let world = match kind {
            ContentKind::Modpack => {
                return Err(Error::Other("modpacks install as a new instance".into()));
            }
            ContentKind::DataPack => Some(self.world_folder(&instance.id, wanted.world)?),
            _ => None,
        };
        let mut lock = self.lockfile(&instance.id)?;
        let already = installed_keys(&lock, kind, world.as_deref());
        task.progress(0, 1, "Working out dependencies");
        let planned = match source {
            ContentSource::Modrinth => {
                self.plan_modrinth(instance, &already, project, kind, version)
                    .await?
            }
            ContentSource::CurseForge => {
                self.plan_curseforge(instance, &already, project, kind, version)
                    .await?
            }
            ContentSource::Ftb | ContentSource::AtLauncher => {
                return Err(Error::Other("this store only has modpacks".into()));
            }
        };
        if planned.is_empty() {
            return Ok(None);
        }

        let staged = self.paths.cache().join("downloads");
        let fetches: Vec<Fetch> = planned
            .iter()
            .map(|p| Fetch {
                url: p.url.clone(),
                dest: staged.join(&p.file),
                hash: p.hash.clone(),
                size: Some(p.size),
            })
            .collect();
        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading")
            })
            .await?;

        let primary = planned.first().map(|p| p.name.clone()).unwrap_or_default();
        let extras = planned.len() - 1;
        let game = self.paths.game(&instance.id);
        let mut stored: HashMap<String, String> = HashMap::new();
        for plan in &planned {
            let path = staged.join(&plan.file);
            let file = self.store.add(&path, Placement::Link)?;
            let _ = fs::remove_file(&path);
            stored.insert(plan.project_key.clone(), file.hash.clone());
            let entry = LockedMod {
                name: plan.name.clone(),
                file: plan.file.clone(),
                hash: file.hash,
                size: file.size,
                enabled: true,
                source: plan.source.clone(),
                origin: plan.origin.clone(),
                side: plan.side,
                version: Some(plan.version.clone()),
                icon: plan.icon.clone(),
                group: world.clone().filter(|_| plan.kind == ContentKind::DataPack),
                tags: Vec::new(),
                requires: Vec::new(),
                remote_hash: plan.remote_hash.clone(),
            };
            let folder = content_folder(plan.kind, &entry);
            self.store.place(
                &entry.hash,
                &game.join(folder).join(&plan.file),
                Placement::Link,
            )?;
            // The same file may already be there by hand, with nothing known about it.
            lock.shelf_mut(plan.kind)
                .retain(|m| m.file != plan.file || m.group != entry.group);
            lock.shelf_mut(plan.kind).push(entry);
        }
        link_requirements(&mut lock, &planned, &stored);
        lock.save(&self.paths.instance(&instance.id))?;

        let label = match extras {
            0 => format!("Installed {primary}"),
            1 => format!("Installed {primary} and 1 dependency"),
            n => format!("Installed {primary} and {n} dependencies"),
        };
        self.trail(&instance.id).mark(&label, WaymarkKind::Install)
    }

    async fn plan_modrinth(
        &self,
        instance: &Instance,
        already: &HashSet<String>,
        project: &str,
        kind: ContentKind,
        wanted: Option<&str>,
    ) -> Result<Vec<Planned>> {
        let mut planned = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([(project.to_string(), None::<String>)]);
        while let Some((id, needed_by)) = queue.pop_front() {
            if !seen.insert(id.clone()) || already.contains(&format!("modrinth:{id}")) {
                continue;
            }
            let details = modrinth::project(&self.net, &id).await?;
            let key = format!("modrinth:{}", details.id);
            if already.contains(&key) {
                continue;
            }
            let dependency_kind = if needed_by.is_some() {
                ContentKind::Mod
            } else {
                kind
            };
            let versions = modrinth::versions(
                &self.net,
                &details.id,
                &instance.minecraft,
                instance.loader.kind,
                dependency_kind,
            )
            .await?;
            // Only versions made for this instance are in the list, so asking
            // for anything else fails here instead of breaking the game.
            let chosen = match wanted.filter(|_| needed_by.is_none()) {
                Some(wanted) => versions.iter().find(|v| v.id == wanted),
                None => modrinth::pick(&versions),
            };
            let version = chosen.ok_or_else(|| {
                Error::NotFound(format!(
                    "a version of {} for Minecraft {}",
                    details.title, instance.minecraft
                ))
            })?;
            let file = version
                .primary_file()
                .ok_or_else(|| Error::NotFound(format!("a file for {}", details.title)))?;
            let requires = version.required_projects();
            for dependency in &requires {
                queue.push_back((dependency.clone(), Some(details.title.clone())));
            }
            planned.push(Planned {
                kind: dependency_kind,
                name: details.title.clone(),
                file: file.filename.clone(),
                url: file.url.clone(),
                hash: Expected::Sha512(file.hashes.sha512.clone()),
                size: file.size,
                source: Source::Modrinth {
                    project: details.id.clone(),
                    version: version.id.clone(),
                },
                origin: needed_by.map_or(Origin::Manual, |of| Origin::Dependency { of: vec![of] }),
                version: version.version_number.clone(),
                icon: details.icon_url.clone(),
                side: side_from(&details),
                project_key: key,
                requires: requires.iter().map(|r| format!("modrinth:{r}")).collect(),
                remote_hash: Some(file.hashes.sha512.clone()),
            });
        }
        Ok(planned)
    }

    async fn plan_curseforge(
        &self,
        instance: &Instance,
        already: &HashSet<String>,
        project: &str,
        kind: ContentKind,
        wanted: Option<&str>,
    ) -> Result<Vec<Planned>> {
        let first: u64 = project
            .parse()
            .map_err(|_| Error::NotFound(format!("CurseForge project {project}")))?;
        let mut planned = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([(first, None::<String>)]);
        while let Some((id, needed_by)) = queue.pop_front() {
            let key = format!("curseforge:{id}");
            if !seen.insert(id) || already.contains(&key) {
                continue;
            }
            let details = curseforge::project(&self.net, id).await?;
            let dependency_kind = if needed_by.is_some() {
                ContentKind::Mod
            } else {
                kind
            };
            let files = match wanted.filter(|_| needed_by.is_none()) {
                // The list stops at 50 files, so a chosen file is fetched directly.
                Some(wanted) => {
                    let file: u64 = wanted
                        .parse()
                        .map_err(|_| Error::NotFound(format!("CurseForge file {wanted}")))?;
                    let found = curseforge::files_by_id(&self.net, &[file]).await?;
                    found
                        .into_iter()
                        .filter(|f| f.mod_id == id && f.game_versions.contains(&instance.minecraft))
                        .collect()
                }
                None => {
                    curseforge::files(
                        &self.net,
                        id,
                        &instance.minecraft,
                        instance.loader.kind,
                        dependency_kind,
                    )
                    .await?
                }
            };
            let file = curseforge::pick(&files).ok_or_else(|| {
                Error::NotFound(format!(
                    "a version of {} for Minecraft {}",
                    details.name, instance.minecraft
                ))
            })?;
            let url = file
                .download_url
                .clone()
                .ok_or_else(|| manual_download(&details))?;
            let requires = file.required_mods();
            for dependency in &requires {
                queue.push_back((*dependency, Some(details.name.clone())));
            }
            planned.push(Planned {
                kind: dependency_kind,
                name: details.name.clone(),
                file: file.file_name.clone(),
                url,
                hash: file.sha1().map_or(Expected::Nothing, Expected::Sha1),
                size: file.file_length,
                source: Source::CurseForge {
                    project: id,
                    file: file.id,
                },
                origin: needed_by.map_or(Origin::Manual, |of| Origin::Dependency { of: vec![of] }),
                version: file.display_name.clone(),
                icon: details.logo.as_ref().map(|l| l.url.clone()),
                side: Side::Both,
                project_key: key,
                requires: requires.iter().map(|r| format!("curseforge:{r}")).collect(),
                remote_hash: None,
            });
        }
        Ok(planned)
    }

    pub fn set_mod_enabled(&self, id: &str, hash: &str, enabled: bool) -> Result<Vec<LockedMod>> {
        self.set_content_enabled(id, ContentKind::Mod, hash, None, enabled)
    }

    /// Switches a file on or off by renaming it. A hash can appear in several
    /// worlds, so a data pack is found by its world too.
    pub fn set_content_enabled(
        &self,
        id: &str,
        kind: ContentKind,
        hash: &str,
        world: Option<&str>,
        enabled: bool,
    ) -> Result<Vec<LockedMod>> {
        let mut lock = self.lockfile(id)?;
        let game = self.paths.game(id);
        let entry = lock
            .shelf_mut(kind)
            .iter_mut()
            .find(|m| m.hash == hash && (world.is_none() || m.group.as_deref() == world))
            .ok_or_else(|| Error::NotFound("that file".into()))?;
        if entry.enabled != enabled {
            let folder = game.join(content_folder(kind, entry));
            let from = folder.join(entry.disk_name());
            entry.enabled = enabled;
            fs::rename(from, folder.join(entry.disk_name()))?;
            lock.save(&self.paths.instance(id))?;
        }
        Ok(lock.shelf(kind).to_vec())
    }

    /// Removes packs as one waymark. Mods go through `remove_mods`, which also
    /// clears out dependencies nothing needs any more.
    pub fn remove_content(
        &self,
        id: &str,
        kind: ContentKind,
        hashes: &[String],
        world: Option<&str>,
    ) -> Result<Option<WaymarkSummary>> {
        if kind == ContentKind::Mod {
            return self.remove_mods(id, hashes);
        }
        let mut lock = self.lockfile(id)?;
        let game = self.paths.game(id);
        let chosen = |m: &LockedMod| {
            hashes.contains(&m.hash) && (world.is_none() || m.group.as_deref() == world)
        };
        let mut names = Vec::new();
        for entry in lock.shelf(kind).iter().filter(|m| chosen(m)) {
            let path = game
                .join(content_folder(kind, entry))
                .join(entry.disk_name());
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
            names.push(entry.name.clone());
        }
        if names.is_empty() {
            return Err(Error::NotFound("that file".into()));
        }
        lock.shelf_mut(kind).retain(|m| !chosen(m));
        lock.save(&self.paths.instance(id))?;
        let label = match names.as_slice() {
            [name] => format!("Removed {name}"),
            many => format!("Removed {} packs", many.len()),
        };
        self.trail(id).mark(&label, WaymarkKind::Remove)
    }

    /// Removes a mod, plus any dependency nothing else needs any more.
    pub fn remove_mod(&self, id: &str, hash: &str) -> Result<Option<WaymarkSummary>> {
        self.remove_mods(id, &[hash.to_string()])
    }

    /// Removes several mods as one waymark, so one return undoes them all.
    pub fn remove_mods(&self, id: &str, hashes: &[String]) -> Result<Option<WaymarkSummary>> {
        let mut lock = self.lockfile(id)?;
        let names: Vec<String> = lock
            .mods
            .iter()
            .filter(|m| hashes.contains(&m.hash))
            .map(|m| m.name.clone())
            .collect();
        if names.is_empty() {
            return Err(Error::NotFound("that mod".into()));
        }
        let mods_dir = self.paths.game(id).join("mods");
        let mut removing = names.clone();
        let mut removed = 0;
        while let Some(gone) = removing.pop() {
            let mut kept = Vec::with_capacity(lock.mods.len());
            for mut entry in std::mem::take(&mut lock.mods) {
                let orphaned = match &mut entry.origin {
                    Origin::Dependency { of } => {
                        of.retain(|parent| *parent != gone);
                        of.is_empty()
                    }
                    _ => false,
                };
                if entry.name == gone || orphaned {
                    let _ = fs::remove_file(mods_dir.join(entry.disk_name()));
                    removed += 1;
                    if entry.name != gone {
                        removing.push(entry.name.clone());
                    }
                } else {
                    kept.push(entry);
                }
            }
            lock.mods = kept;
        }
        lock.save(&self.paths.instance(id))?;
        let label = match (names.as_slice(), removed - names.len()) {
            ([name], 0) => format!("Removed {name}"),
            ([name], 1) => format!("Removed {name} and 1 unused dependency"),
            ([name], extra) => format!("Removed {name} and {extra} unused dependencies"),
            _ => format!("Removed {removed} mods"),
        };
        self.trail(id).mark(&label, WaymarkKind::Remove)
    }

    /// Checks each kind of content from Modrinth in one request. Versions newer
    /// than the release delay are reported as held: if a popular mod's author
    /// account is hijacked, a few days' delay is what keeps the malware off your
    /// machine.
    pub async fn check_updates(&self, id: &str) -> Result<Vec<ModUpdate>> {
        let instance = self.instance(id)?;
        let lock = self.lockfile(id)?;
        let delay = time::Duration::hours(self.settings()?.release_delay_hours.into());
        let now = OffsetDateTime::now_utc();
        let mut updates = Vec::new();
        for kind in SHELVES {
            let hashes: Vec<String> = lock
                .shelf(kind)
                .iter()
                .filter_map(|m| m.remote_hash.clone())
                .collect();
            if hashes.is_empty() {
                continue;
            }
            let latest = modrinth::latest_for_files(
                &self.net,
                &hashes,
                &instance.minecraft,
                instance.loader.kind,
                kind,
            )
            .await?;
            for entry in lock.shelf(kind) {
                let Some(version) = entry.remote_hash.as_ref().and_then(|h| latest.get(h)) else {
                    continue;
                };
                if version
                    .primary_file()
                    .is_none_or(|f| Some(&f.hashes.sha512) == entry.remote_hash.as_ref())
                {
                    continue;
                }
                updates.push(ModUpdate {
                    kind,
                    hash: entry.hash.clone(),
                    name: entry.name.clone(),
                    source: ContentSource::Modrinth,
                    project: version.project_id.clone(),
                    current: entry.version.clone(),
                    latest: version.version_number.clone(),
                    held: published_within(&version.date_published, delay, now),
                });
            }
        }
        // A CurseForge problem shouldn't hide what Modrinth found.
        if curseforge::available()
            && let Ok(found) = self.curseforge_updates(&instance, &lock, delay, now).await
        {
            updates.extend(found);
        }
        Ok(updates)
    }

    async fn curseforge_updates(
        &self,
        instance: &Instance,
        lock: &Lockfile,
        delay: time::Duration,
        now: OffsetDateTime,
    ) -> Result<Vec<ModUpdate>> {
        let tracked: Vec<(ContentKind, &LockedMod, u64, u64)> = lock
            .everything()
            .filter_map(|(kind, entry)| match entry.source {
                Source::CurseForge { project, file } => Some((kind, entry, project, file)),
                _ => None,
            })
            .collect();
        if tracked.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<u64> = tracked.iter().map(|(_, _, project, _)| *project).collect();
        let projects = curseforge::mods_by_id(&self.net, &ids).await?;
        let newer = |kind: ContentKind, project: u64, file: u64| {
            let details = projects.iter().find(|p| p.id == project)?;
            curseforge::latest_file(details, &instance.minecraft, instance.loader.kind, kind)
                .filter(|latest| *latest > file)
        };
        let wanted: Vec<u64> = tracked
            .iter()
            .filter_map(|(kind, _, project, file)| newer(*kind, *project, *file))
            .collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let files = curseforge::files_by_id(&self.net, &wanted).await?;
        let mut updates = Vec::new();
        for (kind, entry, project, file) in &tracked {
            let Some(latest) = newer(*kind, *project, *file) else {
                continue;
            };
            let Some(found) = files.iter().find(|f| f.id == latest) else {
                continue;
            };
            updates.push(ModUpdate {
                kind: *kind,
                hash: entry.hash.clone(),
                name: entry.name.clone(),
                source: ContentSource::CurseForge,
                project: project.to_string(),
                current: entry.version.clone(),
                latest: found.display_name.clone(),
                held: published_within(&found.file_date, delay, now),
            });
        }
        Ok(updates)
    }

    /// Updates the given files as a single waymark, so one return undoes it all.
    pub async fn apply_updates(
        &self,
        id: &str,
        picks: &[UpdatePick],
    ) -> Result<Option<WaymarkSummary>> {
        let instance = self.instance(id)?;
        let task = Task::start(&self.events, format!("Updating {}", instance.name));
        match self.update_mods(&instance, picks, &task).await {
            Ok(mark) => {
                task.finish();
                Ok(mark)
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    /// Works out which file each pick becomes. A chosen version has to be made
    /// for this instance, so a pick can't quietly break the game.
    async fn plan_replacements(
        &self,
        instance: &Instance,
        lock: &Lockfile,
        picks: &[UpdatePick],
    ) -> Result<Vec<Replacement>> {
        let (minecraft, loader) = (&instance.minecraft, instance.loader.kind);
        let mut out = Vec::new();
        for kind in SHELVES {
            let chosen: Vec<(&LockedMod, Option<&str>)> = lock
                .shelf(kind)
                .iter()
                .filter_map(|entry| {
                    Some((
                        entry,
                        picks
                            .iter()
                            .find(|pick| pick.hash == entry.hash)?
                            .version
                            .as_deref(),
                    ))
                })
                .collect();
            let from_modrinth = |entry: &LockedMod| matches!(entry.source, Source::Modrinth { .. });
            let from_curseforge =
                |entry: &LockedMod| matches!(entry.source, Source::CurseForge { .. });

            let newest: Vec<String> = chosen
                .iter()
                .filter(|(e, v)| v.is_none() && from_modrinth(e))
                .filter_map(|(e, _)| e.remote_hash.clone())
                .collect();
            let latest = if newest.is_empty() {
                HashMap::new()
            } else {
                modrinth::latest_for_files(&self.net, &newest, minecraft, loader, kind).await?
            };
            let named: Vec<String> = chosen
                .iter()
                .filter(|(e, _)| from_modrinth(e))
                .filter_map(|(_, v)| v.map(String::from))
                .collect();
            let versions = if named.is_empty() {
                Vec::new()
            } else {
                modrinth::versions_by_id(&self.net, &named).await?
            };

            let projects: Vec<u64> = chosen
                .iter()
                .filter(|(_, v)| v.is_none())
                .filter_map(|(e, _)| {
                    if let Source::CurseForge { project, .. } = e.source {
                        Some(project)
                    } else {
                        None
                    }
                })
                .collect();
            let details = if projects.is_empty() {
                Vec::new()
            } else {
                curseforge::mods_by_id(&self.net, &projects).await?
            };
            let mut wanted_files = Vec::new();
            for (entry, version) in chosen.iter().filter(|(e, _)| from_curseforge(e)) {
                let Source::CurseForge { project, .. } = entry.source else {
                    continue;
                };
                let file = match version {
                    Some(id) => id.parse().ok(),
                    None => details
                        .iter()
                        .find(|p| p.id == project)
                        .and_then(|p| curseforge::latest_file(p, minecraft, loader, kind)),
                };
                wanted_files.extend(file);
            }
            let files = if wanted_files.is_empty() {
                Vec::new()
            } else {
                curseforge::files_by_id(&self.net, &wanted_files).await?
            };

            for (entry, wanted) in chosen {
                match &entry.source {
                    Source::Modrinth { project, .. } => {
                        let version = match wanted {
                            Some(id) => versions.iter().find(|v| {
                                v.id == id
                                    && v.project_id == *project
                                    && v.game_versions.contains(minecraft)
                            }),
                            None => entry.remote_hash.as_ref().and_then(|hash| latest.get(hash)),
                        };
                        let Some(version) = version else {
                            if wanted.is_some() {
                                return Err(Error::NotFound(format!(
                                    "that version of {} for Minecraft {minecraft}",
                                    entry.name
                                )));
                            }
                            continue;
                        };
                        let Some(file) = version.primary_file() else {
                            continue;
                        };
                        out.push(Replacement {
                            kind,
                            old_hash: entry.hash.clone(),
                            file: file.filename.clone(),
                            url: file.url.clone(),
                            expected: Expected::Sha512(file.hashes.sha512.clone()),
                            size: file.size,
                            version: version.version_number.clone(),
                            source: Source::Modrinth {
                                project: project.clone(),
                                version: version.id.clone(),
                            },
                            remote_hash: Some(file.hashes.sha512.clone()),
                            needs: if wanted.is_some() {
                                version.required_projects()
                            } else {
                                Vec::new()
                            },
                        });
                    }
                    Source::CurseForge { project, .. } => {
                        let target = match wanted {
                            Some(id) => id.parse().ok(),
                            None => details
                                .iter()
                                .find(|p| p.id == *project)
                                .and_then(|p| curseforge::latest_file(p, minecraft, loader, kind)),
                        };
                        let Some(file) = target.and_then(|id| {
                            files.iter().find(|f| f.id == id && f.mod_id == *project)
                        }) else {
                            continue;
                        };
                        if !file.game_versions.contains(minecraft) {
                            return Err(Error::NotFound(format!(
                                "that version of {} for Minecraft {minecraft}",
                                entry.name
                            )));
                        }
                        let url = file.download_url.clone().ok_or_else(|| {
                            Error::Other(format!("{}'s author doesn't allow downloads through other apps, so get this version from CurseForge", entry.name))
                        })?;
                        out.push(Replacement {
                            kind,
                            old_hash: entry.hash.clone(),
                            file: file.file_name.clone(),
                            url,
                            expected: file.sha1().map_or(Expected::Nothing, Expected::Sha1),
                            size: file.file_length,
                            version: file.display_name.clone(),
                            source: Source::CurseForge {
                                project: *project,
                                file: file.id,
                            },
                            remote_hash: None,
                            needs: Vec::new(),
                        });
                    }
                    Source::Manual => {}
                }
            }
        }
        Ok(out)
    }

    async fn update_mods(
        &self,
        instance: &Instance,
        picks: &[UpdatePick],
        task: &Task,
    ) -> Result<Option<WaymarkSummary>> {
        let mut lock = self.lockfile(&instance.id)?;
        let chosen = self.plan_replacements(instance, &lock, picks).await?;
        if chosen.is_empty() {
            return Ok(None);
        }
        let staged = self.paths.cache().join("downloads");
        let fetches = chosen
            .iter()
            .map(|change| Fetch {
                url: change.url.clone(),
                dest: staged.join(&change.file),
                hash: change.expected.clone(),
                size: Some(change.size),
            })
            .collect();
        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading updates")
            })
            .await?;

        let game = self.paths.game(&instance.id);
        let count = chosen.len();
        let mut needed = Vec::new();
        for change in chosen {
            let path = staged.join(&change.file);
            let stored = self.store.add(&path, Placement::Link)?;
            let entry = lock
                .shelf_mut(change.kind)
                .iter_mut()
                .find(|m| m.hash == change.old_hash)
                .ok_or_else(|| Error::NotFound("that file".into()))?;
            let folder = game.join(content_folder(change.kind, entry));
            let _ = fs::remove_file(folder.join(entry.disk_name()));
            entry.file = change.file;
            entry.hash = stored.hash.clone();
            entry.size = stored.size;
            entry.version = Some(change.version);
            entry.remote_hash = change.remote_hash;
            entry.source = change.source;
            self.store.place(
                &stored.hash,
                &folder.join(entry.disk_name()),
                Placement::Link,
            )?;
            let _ = fs::remove_file(&path);
            needed.extend(change.needs);
        }
        lock.save(&self.paths.instance(&instance.id))?;
        let label = if count == 1 {
            "Updated 1 file".to_string()
        } else {
            format!("Updated {count} files")
        };
        let mark = self.trail(&instance.id).mark(&label, WaymarkKind::Update)?;

        // An older or newer version can need something the last one didn't.
        let have = installed_keys(&lock, ContentKind::Mod, None);
        for project in needed
            .iter()
            .filter(|project| !have.contains(&format!("modrinth:{project}")))
        {
            let wanted = Wanted {
                source: ContentSource::Modrinth,
                project,
                kind: ContentKind::Mod,
                version: None,
                world: None,
            };
            self.install_planned(instance, &wanted, task).await?;
        }
        Ok(mark)
    }

    /// Copies files from the computer into an instance, as one waymark.
    pub fn add_local_files(
        &self,
        id: &str,
        kind: ContentKind,
        paths: &[std::path::PathBuf],
    ) -> Result<Option<WaymarkSummary>> {
        let folder = self.paths.game(id).join(match kind {
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::Shader => "shaderpacks",
            _ => "mods",
        });
        fs::create_dir_all(&folder)?;
        let mut names = Vec::new();
        for path in paths {
            let name = path
                .file_name()
                .ok_or_else(|| Error::NotFound(path.display().to_string()))?;
            fs::copy(path, folder.join(name))?;
            names.push(name.to_string_lossy().into_owned());
        }
        let label = match names.as_slice() {
            [name] => format!("Installed {name}"),
            many => format!("Installed {} files", many.len()),
        };
        self.trail(id).mark(&label, WaymarkKind::Install)
    }
}

fn manual_download(project: &curseforge::Mod) -> Error {
    let page = project
        .links
        .as_ref()
        .and_then(|l| l.website_url.clone())
        .unwrap_or_default();
    Error::Other(format!(
        "{}'s author doesn't allow downloads through other apps. Get it from {page}, then drop the file into this instance.",
        project.name
    ))
}

/// Fills in each new mod's `requires` with the store hashes of what it needs.
fn link_requirements(lock: &mut Lockfile, planned: &[Planned], stored: &HashMap<String, String>) {
    let existing: HashMap<String, String> = lock
        .mods
        .iter()
        .filter_map(|m| Some((project_key(&m.source)?, m.hash.clone())))
        .collect();
    for plan in planned {
        let Some(hash) = stored.get(&plan.project_key) else {
            continue;
        };
        let needs: Vec<String> = plan
            .requires
            .iter()
            .filter_map(|key| stored.get(key).or_else(|| existing.get(key)).cloned())
            .collect();
        if let Some(entry) = lock.mods.iter_mut().find(|m| m.hash == *hash) {
            entry.requires = needs;
        }
    }
}

fn published_within(date: &str, delay: time::Duration, now: OffsetDateTime) -> bool {
    OffsetDateTime::parse(date, &Rfc3339).is_ok_and(|published| now - published < delay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{Loader, LoaderKind};

    fn locked(name: &str, origin: Origin) -> LockedMod {
        LockedMod {
            name: name.into(),
            file: format!("{name}.jar"),
            hash: format!("{name}-hash"),
            size: 1,
            enabled: true,
            source: Source::Manual,
            origin,
            side: Side::Both,
            version: None,
            icon: None,
            group: None,
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        }
    }

    fn core_with(mods: Vec<LockedMod>) -> (tempfile::TempDir, Core, String) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance(
                "Test",
                "1.21.4",
                Loader {
                    kind: LoaderKind::Fabric,
                    version: None,
                },
                "x".into(),
                None,
            )
            .unwrap();
        let mods_dir = core.paths.game(&instance.id).join("mods");
        let mut lock = core.lockfile(&instance.id).unwrap();
        for entry in &mods {
            fs::write(mods_dir.join(entry.disk_name()), entry.name.as_bytes()).unwrap();
        }
        lock.mods = mods;
        lock.save(&core.paths.instance(&instance.id)).unwrap();
        (dir, core, instance.id)
    }

    #[test]
    fn removing_a_mod_takes_its_orphaned_dependencies_with_it() {
        let (_dir, core, id) = core_with(vec![
            locked("iris", Origin::Manual),
            locked(
                "sodium",
                Origin::Dependency {
                    of: vec!["iris".into()],
                },
            ),
            locked("other", Origin::Manual),
        ]);
        core.remove_mod(&id, "iris-hash").unwrap();
        let names: Vec<String> = core
            .lockfile(&id)
            .unwrap()
            .mods
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, ["other"]);
    }

    #[test]
    fn a_dependency_stays_while_something_else_needs_it() {
        let (_dir, core, id) = core_with(vec![
            locked("iris", Origin::Manual),
            locked("indium", Origin::Manual),
            locked(
                "sodium",
                Origin::Dependency {
                    of: vec!["iris".into(), "indium".into()],
                },
            ),
        ]);
        core.remove_mod(&id, "iris-hash").unwrap();
        let lock = core.lockfile(&id).unwrap();
        assert!(lock.mods.iter().any(|m| m.name == "sodium"));
    }

    #[test]
    fn disabling_a_mod_renames_its_file() {
        let (_dir, core, id) = core_with(vec![locked("sodium", Origin::Manual)]);
        core.set_mod_enabled(&id, "sodium-hash", false).unwrap();
        let mods_dir = core.paths.game(&id).join("mods");
        assert!(mods_dir.join("sodium.jar.disabled").is_file());
        assert!(!mods_dir.join("sodium.jar").exists());
        core.set_mod_enabled(&id, "sodium-hash", true).unwrap();
        assert!(mods_dir.join("sodium.jar").is_file());
    }

    #[test]
    fn packs_switch_off_and_come_out_as_one_waymark() {
        let (_dir, core, id) = core_with(Vec::new());
        let packs = core.paths.game(&id).join("resourcepacks");
        fs::create_dir_all(packs.join("Unpacked")).unwrap();
        fs::write(packs.join("faithful.zip"), b"zip").unwrap();

        let list = core
            .instance_content(&id, ContentKind::ResourcePack)
            .unwrap();
        assert_eq!(list.len(), 2);
        let faithful = list
            .iter()
            .find(|p| p.file == "faithful.zip")
            .unwrap()
            .hash
            .clone();
        core.set_content_enabled(&id, ContentKind::ResourcePack, &faithful, None, false)
            .unwrap();
        assert!(packs.join("faithful.zip.disabled").is_file());

        let every: Vec<String> = list.iter().map(|p| p.hash.clone()).collect();
        let mark = core
            .remove_content(&id, ContentKind::ResourcePack, &every, None)
            .unwrap()
            .unwrap();
        assert_eq!(mark.label, "Removed 2 packs");
        assert!(
            core.instance_content(&id, ContentKind::ResourcePack)
                .unwrap()
                .is_empty()
        );
        assert!(!packs.join("Unpacked").exists());
    }

    #[test]
    fn data_packs_only_install_into_a_real_world() {
        let (_dir, core, id) = core_with(Vec::new());
        fs::create_dir_all(core.paths.game(&id).join("saves/Home")).unwrap();
        assert_eq!(core.world_folder(&id, Some("Home")).unwrap(), "Home");
        assert!(core.world_folder(&id, None).is_err());
        assert!(core.world_folder(&id, Some("Elsewhere")).is_err());
        assert!(core.world_folder(&id, Some("../Home")).is_err());
        assert!(core.world_folder(&id, Some("Home/datapacks")).is_err());
    }

    #[test]
    fn the_same_data_pack_can_be_in_two_worlds() {
        let mut lock = Lockfile::new("1.21.4", Loader::default());
        let mut pack = locked("terralith", Origin::Manual);
        pack.source = Source::Modrinth {
            project: "t".into(),
            version: "1".into(),
        };
        pack.group = Some("Home".into());
        lock.data_packs.push(pack);
        assert!(installed_keys(&lock, ContentKind::DataPack, Some("Home")).contains("modrinth:t"));
        assert!(installed_keys(&lock, ContentKind::DataPack, Some("Skyblock")).is_empty());
        assert!(installed_keys(&lock, ContentKind::Mod, None).is_empty());
    }

    #[test]
    fn dropped_files_land_in_the_right_folder_as_one_waymark() {
        let (dir, core, id) = core_with(Vec::new());
        let jar = dir.path().join("cool-mod.jar");
        fs::write(&jar, b"jar").unwrap();
        let mark = core
            .add_local_files(&id, ContentKind::Mod, &[jar])
            .unwrap()
            .unwrap();
        assert_eq!(mark.label, "Installed cool-mod.jar");
        assert!(core.paths.game(&id).join("mods/cool-mod.jar").is_file());
        assert_eq!(core.lockfile(&id).unwrap().mods.len(), 1);

        let pack = dir.path().join("pack.zip");
        fs::write(&pack, b"zip").unwrap();
        core.add_local_files(&id, ContentKind::ResourcePack, &[pack])
            .unwrap();
        assert_eq!(core.lockfile(&id).unwrap().resource_packs.len(), 1);
    }

    #[test]
    fn brand_new_releases_are_held_back() {
        let now = OffsetDateTime::now_utc();
        let delay = time::Duration::hours(48);
        let hour_old = (now - time::Duration::hours(1)).format(&Rfc3339).unwrap();
        let week_old = (now - time::Duration::days(7)).format(&Rfc3339).unwrap();
        assert!(published_within(&hour_old, delay, now));
        assert!(!published_within(&week_old, delay, now));
    }
}

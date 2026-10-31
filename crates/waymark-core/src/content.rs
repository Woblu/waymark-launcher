//! Adding, removing and updating what's in an instance, from either source.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Core;
use crate::curseforge;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::instance::Instance;
use crate::lock::{LockedMod, Lockfile, Origin, Side, Source};
use crate::modrinth;
use crate::net::{Expected, Fetch};
use crate::store::Placement;
use crate::waymarks::{WaymarkKind, WaymarkSummary};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    Modrinth,
    CurseForge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Mod,
    Modpack,
    ResourcePack,
    Shader,
}

impl ContentKind {
    fn folder(self) -> Result<&'static str> {
        match self {
            ContentKind::Mod => Ok("mods"),
            ContentKind::ResourcePack => Ok("resourcepacks"),
            ContentKind::Shader => Ok("shaderpacks"),
            ContentKind::Modpack => Err(Error::Other("modpacks install as a new instance".into())),
        }
    }
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentPage {
    pub hits: Vec<ContentHit>,
    pub total: u64,
}

/// A file about to be added.
struct Planned {
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
    pub hash: String,
    pub name: String,
    pub current: Option<String>,
    pub latest: String,
    /// Published too recently to install yet, under the release delay setting.
    pub held: bool,
}

fn project_key(source: &Source) -> Option<String> {
    match source {
        Source::Modrinth { project, .. } => Some(format!("modrinth:{project}")),
        Source::CurseForge { project, .. } => Some(format!("curseforge:{project}")),
        Source::Manual => None,
    }
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
    pub async fn search_content(&self, source: ContentSource, query: &SearchQuery) -> Result<ContentPage> {
        match source {
            ContentSource::Modrinth => modrinth::search(&self.net, query).await,
            ContentSource::CurseForge => curseforge::search(&self.net, query).await,
        }
    }

    /// Adds a project and everything it requires, as one waymark.
    pub async fn install_content(
        &self,
        id: &str,
        source: ContentSource,
        project: &str,
        kind: ContentKind,
    ) -> Result<Option<WaymarkSummary>> {
        let instance = self.instance(id)?;
        let task = Task::start(&self.events, format!("Adding to {}", instance.name));
        match self.install_planned(&instance, source, project, kind, &task).await {
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
        source: ContentSource,
        project: &str,
        kind: ContentKind,
        task: &Task,
    ) -> Result<Option<WaymarkSummary>> {
        let folder = kind.folder()?;
        let mut lock = self.lockfile(&instance.id)?;
        task.progress(0, 1, "Working out dependencies");
        let planned = match source {
            ContentSource::Modrinth => self.plan_modrinth(instance, &lock, project, kind).await?,
            ContentSource::CurseForge => self.plan_curseforge(instance, &lock, project, kind).await?,
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
            .fetch_all(fetches, |done, total| task.progress(done, total, "Downloading"))
            .await?;

        let primary = planned.first().map(|p| p.name.clone()).unwrap_or_default();
        let extras = planned.len() - 1;
        let game = self.paths.game(&instance.id);
        let mut stored: HashMap<String, String> = HashMap::new();
        for plan in &planned {
            let path = staged.join(&plan.file);
            let file = self.store.add(&path, Placement::Link)?;
            self.store.place(&file.hash, &game.join(folder).join(&plan.file), Placement::Link)?;
            let _ = fs::remove_file(&path);
            stored.insert(plan.project_key.clone(), file.hash.clone());
            if folder == "mods" {
                lock.mods.push(LockedMod {
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
                    group: None,
                    tags: Vec::new(),
                    requires: Vec::new(),
                    remote_hash: plan.remote_hash.clone(),
                });
            }
        }
        link_requirements(&mut lock, &planned, &stored);
        lock.save(&self.paths.instance(&instance.id))?;

        let label = match extras {
            0 => format!("Added {primary}"),
            1 => format!("Added {primary} and 1 dependency"),
            n => format!("Added {primary} and {n} dependencies"),
        };
        self.trail(&instance.id).mark(&label, WaymarkKind::Install)
    }

    async fn plan_modrinth(&self, instance: &Instance, lock: &Lockfile, project: &str, kind: ContentKind) -> Result<Vec<Planned>> {
        let already: HashSet<String> = lock.mods.iter().filter_map(|m| project_key(&m.source)).collect();
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
            let dependency_kind = if needed_by.is_some() { ContentKind::Mod } else { kind };
            let versions = modrinth::versions(&self.net, &details.id, &instance.minecraft, instance.loader.kind, dependency_kind).await?;
            let version = modrinth::pick(&versions).ok_or_else(|| {
                Error::NotFound(format!("a version of {} for Minecraft {}", details.title, instance.minecraft))
            })?;
            let file = version.primary_file().ok_or_else(|| Error::NotFound(format!("a file for {}", details.title)))?;
            let requires = version.required_projects();
            for dependency in &requires {
                queue.push_back((dependency.clone(), Some(details.title.clone())));
            }
            planned.push(Planned {
                name: details.title.clone(),
                file: file.filename.clone(),
                url: file.url.clone(),
                hash: Expected::Sha512(file.hashes.sha512.clone()),
                size: file.size,
                source: Source::Modrinth { project: details.id.clone(), version: version.id.clone() },
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

    async fn plan_curseforge(&self, instance: &Instance, lock: &Lockfile, project: &str, kind: ContentKind) -> Result<Vec<Planned>> {
        let already: HashSet<String> = lock.mods.iter().filter_map(|m| project_key(&m.source)).collect();
        let first: u64 = project.parse().map_err(|_| Error::NotFound(format!("CurseForge project {project}")))?;
        let mut planned = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([(first, None::<String>)]);
        while let Some((id, needed_by)) = queue.pop_front() {
            let key = format!("curseforge:{id}");
            if !seen.insert(id) || already.contains(&key) {
                continue;
            }
            let details = curseforge::project(&self.net, id).await?;
            let dependency_kind = if needed_by.is_some() { ContentKind::Mod } else { kind };
            let files = curseforge::files(&self.net, id, &instance.minecraft, instance.loader.kind, dependency_kind).await?;
            let file = curseforge::pick(&files).ok_or_else(|| {
                Error::NotFound(format!("a version of {} for Minecraft {}", details.name, instance.minecraft))
            })?;
            let url = file.download_url.clone().ok_or_else(|| manual_download(&details))?;
            let requires = file.required_mods();
            for dependency in &requires {
                queue.push_back((*dependency, Some(details.name.clone())));
            }
            planned.push(Planned {
                name: details.name.clone(),
                file: file.file_name.clone(),
                url,
                hash: file.sha1().map_or(Expected::Nothing, Expected::Sha1),
                size: file.file_length,
                source: Source::CurseForge { project: id, file: file.id },
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

    pub fn set_mod_enabled(&self, id: &str, hash: &str, enabled: bool) -> Result<Lockfile> {
        let mut lock = self.lockfile(id)?;
        let mods_dir = self.paths.game(id).join("mods");
        let entry = lock.mods.iter_mut().find(|m| m.hash == hash).ok_or_else(|| Error::NotFound("that mod".into()))?;
        if entry.enabled != enabled {
            let from = mods_dir.join(entry.disk_name());
            entry.enabled = enabled;
            fs::rename(from, mods_dir.join(entry.disk_name()))?;
            lock.save(&self.paths.instance(id))?;
        }
        Ok(lock)
    }

    /// Removes a mod, plus any dependency nothing else needs any more.
    pub fn remove_mod(&self, id: &str, hash: &str) -> Result<Option<WaymarkSummary>> {
        self.remove_mods(id, &[hash.to_string()])
    }

    /// Removes several mods as one waymark, so one return undoes them all.
    pub fn remove_mods(&self, id: &str, hashes: &[String]) -> Result<Option<WaymarkSummary>> {
        let mut lock = self.lockfile(id)?;
        let names: Vec<String> = lock.mods.iter().filter(|m| hashes.contains(&m.hash)).map(|m| m.name.clone()).collect();
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

    /// Checks every Modrinth mod in one request. Versions newer than the
    /// release delay are reported as held: if a popular mod's author account
    /// is hijacked, a few days' delay is what keeps the malware off your machine.
    pub async fn check_updates(&self, id: &str) -> Result<Vec<ModUpdate>> {
        let instance = self.instance(id)?;
        let lock = self.lockfile(id)?;
        let hashes: Vec<String> = lock.mods.iter().filter_map(|m| m.remote_hash.clone()).collect();
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let delay = time::Duration::hours(self.settings()?.release_delay_hours.into());
        let latest = modrinth::latest_for_files(&self.net, &hashes, &instance.minecraft, instance.loader.kind).await?;
        let now = OffsetDateTime::now_utc();
        let mut updates = Vec::new();
        for entry in &lock.mods {
            let Some(version) = entry.remote_hash.as_ref().and_then(|h| latest.get(h)) else { continue };
            if version.primary_file().is_none_or(|f| Some(&f.hashes.sha512) == entry.remote_hash.as_ref()) {
                continue;
            }
            updates.push(ModUpdate {
                hash: entry.hash.clone(),
                name: entry.name.clone(),
                current: entry.version.clone(),
                latest: version.version_number.clone(),
                held: published_within(&version.date_published, delay, now),
            });
        }
        Ok(updates)
    }

    /// Updates the given mods as a single waymark, so one return undoes it all.
    pub async fn apply_updates(&self, id: &str, hashes: &[String]) -> Result<Option<WaymarkSummary>> {
        let instance = self.instance(id)?;
        let task = Task::start(&self.events, format!("Updating {}", instance.name));
        match self.update_mods(&instance, hashes, &task).await {
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

    async fn update_mods(&self, instance: &Instance, hashes: &[String], task: &Task) -> Result<Option<WaymarkSummary>> {
        let mut lock = self.lockfile(&instance.id)?;
        let remote: Vec<String> = lock
            .mods
            .iter()
            .filter(|m| hashes.contains(&m.hash))
            .filter_map(|m| m.remote_hash.clone())
            .collect();
        if remote.is_empty() {
            return Ok(None);
        }
        let latest = modrinth::latest_for_files(&self.net, &remote, &instance.minecraft, instance.loader.kind).await?;
        let staged = self.paths.cache().join("downloads");
        let mut chosen = Vec::new();
        for entry in lock.mods.iter().filter(|m| hashes.contains(&m.hash)) {
            let Some(version) = entry.remote_hash.as_ref().and_then(|h| latest.get(h)) else { continue };
            let Some(file) = version.primary_file() else { continue };
            chosen.push((entry.hash.clone(), version.clone(), file.clone()));
        }
        let fetches = chosen
            .iter()
            .map(|(_, _, file)| Fetch {
                url: file.url.clone(),
                dest: staged.join(&file.filename),
                hash: Expected::Sha512(file.hashes.sha512.clone()),
                size: Some(file.size),
            })
            .collect();
        self.net
            .fetch_all(fetches, |done, total| task.progress(done, total, "Downloading updates"))
            .await?;

        let mods_dir = self.paths.game(&instance.id).join("mods");
        let count = chosen.len();
        for (old_hash, version, file) in chosen {
            let path = staged.join(&file.filename);
            let stored = self.store.add(&path, Placement::Link)?;
            let entry = lock.mods.iter_mut().find(|m| m.hash == old_hash).ok_or_else(|| Error::NotFound("that mod".into()))?;
            let _ = fs::remove_file(mods_dir.join(entry.disk_name()));
            entry.file = file.filename.clone();
            entry.hash = stored.hash.clone();
            entry.size = stored.size;
            entry.version = Some(version.version_number.clone());
            entry.remote_hash = Some(file.hashes.sha512.clone());
            if let Source::Modrinth { version: pinned, .. } = &mut entry.source {
                *pinned = version.id.clone();
            }
            self.store.place(&stored.hash, &mods_dir.join(entry.disk_name()), Placement::Link)?;
            let _ = fs::remove_file(&path);
        }
        lock.save(&self.paths.instance(&instance.id))?;
        let label = if count == 1 { "Updated 1 mod".to_string() } else { format!("Updated {count} mods") };
        self.trail(&instance.id).mark(&label, WaymarkKind::Update)
    }
}

fn manual_download(project: &curseforge::Mod) -> Error {
    let page = project.links.as_ref().and_then(|l| l.website_url.clone()).unwrap_or_default();
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
        let Some(hash) = stored.get(&plan.project_key) else { continue };
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
        let instance = core.register_instance("Test", "1.21.4", Loader { kind: LoaderKind::Fabric, version: None }, "x".into(), None).unwrap();
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
            locked("sodium", Origin::Dependency { of: vec!["iris".into()] }),
            locked("other", Origin::Manual),
        ]);
        core.remove_mod(&id, "iris-hash").unwrap();
        let names: Vec<String> = core.lockfile(&id).unwrap().mods.into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["other"]);
    }

    #[test]
    fn a_dependency_stays_while_something_else_needs_it() {
        let (_dir, core, id) = core_with(vec![
            locked("iris", Origin::Manual),
            locked("indium", Origin::Manual),
            locked("sodium", Origin::Dependency { of: vec!["iris".into(), "indium".into()] }),
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
    fn brand_new_releases_are_held_back() {
        let now = OffsetDateTime::now_utc();
        let delay = time::Duration::hours(48);
        let hour_old = (now - time::Duration::hours(1)).format(&Rfc3339).unwrap();
        let week_old = (now - time::Duration::days(7)).format(&Rfc3339).unwrap();
        assert!(published_within(&hour_old, delay, now));
        assert!(!published_within(&week_old, delay, now));
    }
}

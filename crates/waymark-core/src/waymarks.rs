//! The trail. A waymark records the exact state of an instance, and returning
//! to one restores it in moments. Nothing on the trail is ever rewritten: going
//! back adds a new waymark, so a return can itself be undone.
//!
//! Worlds aren't tracked here. They're large and change every few seconds,
//! so they get their own backups.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::lock::Lockfile;
use crate::store::{Placement, Store, Stored};

pub const TRACKED_DIRS: [&str; 6] = ["mods", "config", "defaultconfigs", "kubejs", "resourcepacks", "shaderpacks"];
pub const TRACKED_FILES: [&str; 3] = ["options.txt", "optionsof.txt", "optionsshaders.txt"];

const TRAIL_DIR: &str = ".waymarks";
const TRAIL_FILE: &str = "trail.json";
const SCAN_CACHE: &str = "scan-cache.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaymarkKind {
    Created,
    Manual,
    Install,
    Remove,
    Update,
    Launch,
    Import,
    Return,
    Automatic,
}

/// What the timeline shows. Kept apart from the file map so listing a long
/// trail never has to read hundreds of snapshots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaymarkSummary {
    pub id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    pub label: String,
    pub kind: WaymarkKind,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    pub files: usize,
    pub mods: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Snapshot {
    lock: Lockfile,
    files: BTreeMap<String, Stored>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CachedFile {
    size: u64,
    modified: u128,
    hash: String,
}

/// An instance's changes that no waymark records yet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

pub struct Trail {
    dir: PathBuf,
    instance: PathBuf,
    game: PathBuf,
    store: Store,
}

impl Trail {
    pub fn new(instance: &Path, store: Store) -> Self {
        Trail {
            dir: instance.join(TRAIL_DIR),
            instance: instance.to_path_buf(),
            game: instance.join("game"),
            store,
        }
    }

    pub fn list(&self) -> Result<Vec<WaymarkSummary>> {
        match fs::read(self.dir.join(TRAIL_FILE)) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Records the current state. Returns `None` if nothing changed since the
    /// last waymark, so automatic marks can be taken freely without noise.
    pub fn mark(&self, label: &str, kind: WaymarkKind) -> Result<Option<WaymarkSummary>> {
        let mut lock = Lockfile::load(&self.instance)?;
        if lock.reconcile(&self.game, &self.store)? {
            lock.save(&self.instance)?;
        }
        fs::create_dir_all(&self.dir)?;
        let files = self.scan()?;
        let mut trail = self.list()?;
        let previous = trail.last().map(|w| self.snapshot(&w.id)).transpose()?;
        let pending = diff(previous.as_ref().map(|s| &s.files), &files);
        if previous.is_some() && pending.is_empty() {
            return Ok(None);
        }

        let at = OffsetDateTime::now_utc();
        let id = format!("{}-{}", at.unix_timestamp_nanos() / 1_000_000, &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let summary = WaymarkSummary {
            id: id.clone(),
            at,
            label: label.to_string(),
            kind,
            added: pending.added,
            removed: pending.removed,
            changed: pending.changed,
            files: files.len(),
            mods: lock.mods.iter().filter(|m| m.enabled).count(),
            bytes: files.values().map(|f| f.size).sum(),
        };
        let snapshot = Snapshot { lock, files };
        write_json(&self.dir.join(format!("{id}.json")), &snapshot)?;
        trail.push(summary.clone());
        write_json(&self.dir.join(TRAIL_FILE), &trail)?;
        Ok(Some(summary))
    }

    /// What has changed on disk since the last waymark.
    pub fn pending(&self) -> Result<Pending> {
        let files = self.scan()?;
        let trail = self.list()?;
        let previous = trail.last().map(|w| self.snapshot(&w.id)).transpose()?;
        Ok(diff(previous.as_ref().map(|s| &s.files), &files))
    }

    /// Puts the instance back to how it was at waymark `id`.
    pub fn restore(&self, id: &str) -> Result<WaymarkSummary> {
        let target = self.snapshot(id)?;
        let trail = self.list()?;
        let label = trail
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.label.clone())
            .ok_or_else(|| Error::NotFound(format!("waymark {id}")))?;
        for stored in target.files.values() {
            if !self.store.contains(&stored.hash) {
                return Err(Error::Other(format!("a file from this waymark is missing from the store: {}", stored.hash)));
            }
        }

        self.mark(&format!("Before returning to \"{label}\""), WaymarkKind::Automatic)?;
        let current = self.scan()?;

        for path in current.keys() {
            if !target.files.contains_key(path) {
                fs::remove_file(self.game.join(path))?;
            }
        }
        for (path, stored) in &target.files {
            let top = path.split('/').next().unwrap_or(path);
            if is_linked(&self.game.join(top)) {
                continue;
            }
            if current.get(path).is_none_or(|have| have.hash != stored.hash) {
                self.store.place(&stored.hash, &self.game.join(path), Placement::for_path(path))?;
            }
        }
        self.prune_empty_dirs()?;
        target.lock.save(&self.instance)?;

        let returned = self.mark(&format!("Returned to \"{label}\""), WaymarkKind::Return)?;
        match returned {
            Some(summary) => Ok(summary),
            None => self.list()?.pop().ok_or_else(|| Error::NotFound("trail".into())),
        }
    }

    /// Every store hash this trail depends on, for garbage collection.
    pub fn live_hashes(&self) -> Result<Vec<String>> {
        let mut hashes = Vec::new();
        for waymark in self.list()? {
            hashes.extend(self.snapshot(&waymark.id)?.files.into_values().map(|f| f.hash));
        }
        Ok(hashes)
    }

    fn snapshot(&self, id: &str) -> Result<Snapshot> {
        let path = self.dir.join(format!("{id}.json"));
        if !path.is_file() {
            return Err(Error::NotFound(format!("waymark {id}")));
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    /// Hashes the tracked files. Size and modification time let unchanged
    /// files skip hashing entirely, and the rest are hashed across all cores,
    /// so marking a large pack takes milliseconds after the first time.
    fn scan(&self) -> Result<BTreeMap<String, Stored>> {
        let cache_path = self.dir.join(SCAN_CACHE);
        let cache: HashMap<String, CachedFile> = fs::read(&cache_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();

        let mut found = Vec::new();
        for root in TRACKED_DIRS {
            let dir = self.game.join(root);
            if dir.is_dir() && !is_linked(&dir) {
                for entry in WalkDir::new(&dir).into_iter().filter_entry(|e| !is_noise(e.file_name())) {
                    let entry = entry.map_err(std::io::Error::from)?;
                    if entry.file_type().is_file() {
                        found.push(entry.into_path());
                    }
                }
            }
        }
        for file in TRACKED_FILES {
            let path = self.game.join(file);
            if path.is_file() {
                found.push(path);
            }
        }

        let mut files = BTreeMap::new();
        let mut fresh_cache = HashMap::with_capacity(found.len());
        let mut to_hash = Vec::new();
        for path in found {
            let relative = self.relative(&path);
            let meta = fs::metadata(&path)?;
            let modified = meta.modified()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
            match cache.get(&relative) {
                Some(hit) if hit.size == meta.len() && hit.modified == modified && self.store.contains(&hit.hash) => {
                    files.insert(relative.clone(), Stored { hash: hit.hash.clone(), size: hit.size });
                    fresh_cache.insert(relative, hit.clone());
                }
                _ => to_hash.push((relative, path, modified)),
            }
        }

        for (relative, stored, modified) in self.hash_all(to_hash)? {
            fresh_cache.insert(relative.clone(), CachedFile { size: stored.size, modified, hash: stored.hash.clone() });
            files.insert(relative, stored);
        }
        write_json(&cache_path, &fresh_cache)?;
        Ok(files)
    }

    fn hash_all(&self, work: Vec<(String, PathBuf, u128)>) -> Result<Vec<(String, Stored, u128)>> {
        if work.is_empty() {
            return Ok(Vec::new());
        }
        let threads = thread::available_parallelism().map_or(4, usize::from);
        let chunk = work.len().div_ceil(threads);
        thread::scope(|scope| {
            let handles: Vec<_> = work
                .chunks(chunk)
                .map(|batch| {
                    scope.spawn(move || -> Result<Vec<(String, Stored, u128)>> {
                        batch
                            .iter()
                            .map(|(relative, path, modified)| {
                                let stored = self.store.add(path, Placement::for_path(relative))?;
                                Ok((relative.clone(), stored, *modified))
                            })
                            .collect()
                    })
                })
                .collect();
            let mut results = Vec::with_capacity(work.len());
            for handle in handles {
                results.extend(handle.join().expect("hashing thread panicked")?);
            }
            Ok(results)
        })
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.game)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn prune_empty_dirs(&self) -> Result<()> {
        for root in TRACKED_DIRS {
            let dir = self.game.join(root);
            if !dir.is_dir() || is_linked(&dir) {
                continue;
            }
            let dirs: Vec<PathBuf> = WalkDir::new(&dir)
                .min_depth(1)
                .contents_first(true)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_dir())
                .map(|e| e.into_path())
                .collect();
            for path in dirs {
                let _ = fs::remove_dir(path);
            }
        }
        Ok(())
    }
}

/// Whether a folder is a link to a shared folder, not the instance's own.
pub(crate) fn is_linked(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn is_noise(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    name.ends_with(".part") || name.ends_with(".tmp") || name == ".cache" || name == "__pycache__"
}

fn diff(previous: Option<&BTreeMap<String, Stored>>, current: &BTreeMap<String, Stored>) -> Pending {
    let Some(previous) = previous else {
        return Pending { added: current.keys().cloned().collect(), ..Pending::default() };
    };
    let mut pending = Pending::default();
    for (path, stored) in current {
        match previous.get(path) {
            None => pending.added.push(path.clone()),
            Some(old) if old.hash != stored.hash => pending.changed.push(path.clone()),
            _ => {}
        }
    }
    pending.removed = previous.keys().filter(|p| !current.contains_key(*p)).cloned().collect();
    pending
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let part = path.with_extension("json.part");
    fs::write(&part, serde_json::to_vec(value)?)?;
    fs::rename(part, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{Loader, LoaderKind};

    struct Fixture {
        _dir: tempfile::TempDir,
        game: PathBuf,
        trail: Trail,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let instance = dir.path().join("instance");
        let game = instance.join("game");
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::create_dir_all(game.join("config")).unwrap();
        Lockfile::new("1.21.4", Loader { kind: LoaderKind::Fabric, version: None }).save(&instance).unwrap();
        let trail = Trail::new(&instance, Store::new(dir.path().join("store")));
        Fixture { _dir: dir, game, trail }
    }

    #[test]
    fn nothing_new_means_no_new_waymark() {
        let f = fixture();
        fs::write(f.game.join("mods/a.jar"), b"a").unwrap();
        assert!(f.trail.mark("first", WaymarkKind::Created).unwrap().is_some());
        assert!(f.trail.mark("again", WaymarkKind::Launch).unwrap().is_none());
        assert_eq!(f.trail.list().unwrap().len(), 1);
    }

    #[test]
    fn each_waymark_records_what_changed() {
        let f = fixture();
        fs::write(f.game.join("mods/a.jar"), b"a").unwrap();
        fs::write(f.game.join("config/a.toml"), b"one").unwrap();
        f.trail.mark("first", WaymarkKind::Created).unwrap();

        fs::write(f.game.join("mods/b.jar"), b"b").unwrap();
        fs::write(f.game.join("config/a.toml"), b"two!").unwrap();
        fs::remove_file(f.game.join("mods/a.jar")).unwrap();
        let second = f.trail.mark("second", WaymarkKind::Install).unwrap().unwrap();

        assert_eq!(second.added, ["mods/b.jar"]);
        assert_eq!(second.removed, ["mods/a.jar"]);
        assert_eq!(second.changed, ["config/a.toml"]);
    }

    #[test]
    fn returning_restores_mods_and_configs_exactly() {
        let f = fixture();
        fs::write(f.game.join("mods/good.jar"), b"good").unwrap();
        fs::write(f.game.join("config/game.toml"), b"before").unwrap();
        let good = f.trail.mark("working", WaymarkKind::Manual).unwrap().unwrap();

        fs::write(f.game.join("mods/bad.jar"), b"bad").unwrap();
        fs::remove_file(f.game.join("mods/good.jar")).unwrap();
        fs::write(f.game.join("config/game.toml"), b"after").unwrap();

        f.trail.restore(&good.id).unwrap();

        assert_eq!(fs::read(f.game.join("mods/good.jar")).unwrap(), b"good");
        assert_eq!(fs::read(f.game.join("config/game.toml")).unwrap(), b"before");
        assert!(!f.game.join("mods/bad.jar").exists());
    }

    #[test]
    fn a_return_can_itself_be_undone() {
        let f = fixture();
        fs::write(f.game.join("mods/old.jar"), b"old").unwrap();
        let old = f.trail.mark("old", WaymarkKind::Manual).unwrap().unwrap();
        fs::remove_file(f.game.join("mods/old.jar")).unwrap();
        fs::write(f.game.join("mods/new.jar"), b"new").unwrap();
        let new = f.trail.mark("new", WaymarkKind::Install).unwrap().unwrap();

        f.trail.restore(&old.id).unwrap();
        assert!(!f.game.join("mods/new.jar").exists());

        f.trail.restore(&new.id).unwrap();
        assert!(f.game.join("mods/new.jar").exists());
        assert!(!f.game.join("mods/old.jar").exists());
    }

    #[test]
    fn unsaved_changes_are_kept_on_the_trail_when_returning() {
        let f = fixture();
        fs::write(f.game.join("mods/a.jar"), b"a").unwrap();
        let first = f.trail.mark("first", WaymarkKind::Manual).unwrap().unwrap();
        fs::write(f.game.join("mods/unsaved.jar"), b"unsaved").unwrap();

        f.trail.restore(&first.id).unwrap();

        let before = f.trail.list().unwrap().into_iter().find(|w| w.kind == WaymarkKind::Automatic).unwrap();
        assert_eq!(before.added, ["mods/unsaved.jar"]);
        f.trail.restore(&before.id).unwrap();
        assert!(f.game.join("mods/unsaved.jar").exists());
    }

    #[test]
    fn worlds_are_left_alone() {
        let f = fixture();
        let world = f.game.join("saves/world/level.dat");
        fs::create_dir_all(world.parent().unwrap()).unwrap();
        fs::write(&world, b"my base").unwrap();
        let first = f.trail.mark("first", WaymarkKind::Manual).unwrap().unwrap();
        assert_eq!(first.files, 0);

        fs::write(f.game.join("mods/a.jar"), b"a").unwrap();
        f.trail.restore(&first.id).unwrap();
        assert_eq!(fs::read(&world).unwrap(), b"my base");
    }

    #[test]
    fn pending_lists_changes_without_recording_them() {
        let f = fixture();
        f.trail.mark("first", WaymarkKind::Created).unwrap();
        fs::write(f.game.join("mods/a.jar"), b"a").unwrap();
        assert_eq!(f.trail.pending().unwrap().added, ["mods/a.jar"]);
        assert_eq!(f.trail.list().unwrap().len(), 1);
    }
}

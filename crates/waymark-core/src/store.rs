//! The content-addressed store. Every file is kept once, named by its BLAKE3
//! hash, no matter how many instances or waymarks refer to it. That one rule
//! is what makes snapshots cheap, and it's why a second copy of a modpack
//! costs almost no disk space.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::Result;

#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub hash: String,
    pub size: u64,
}

/// How a stored file reaches an instance. Archives are immutable in practice,
/// so hard links are safe and instant. Anything the game might edit in place,
/// like a config file, gets its own copy so the edit can't reach into the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Link,
    Copy,
}

impl Placement {
    pub fn for_path(path: &str) -> Placement {
        let lower = path.to_ascii_lowercase();
        let immutable = [".jar", ".zip", ".jar.disabled", ".zip.disabled"];
        if immutable.iter().any(|ending| lower.ends_with(ending)) {
            Placement::Link
        } else {
            Placement::Copy
        }
    }
}

pub fn hash_file(path: &Path) -> Result<Stored> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let size = hasher.update_reader(&mut file)?.count();
    Ok(Stored {
        hash: hasher.finalize().to_hex().to_string(),
        size,
    })
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    pub fn path_of(&self, hash: &str) -> PathBuf {
        self.root.join(&hash[..2]).join(hash)
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.path_of(hash).is_file()
    }

    pub fn add(&self, source: &Path, placement: Placement) -> Result<Stored> {
        let stored = hash_file(source)?;
        let target = self.path_of(&stored.hash);
        if target.is_file() {
            return Ok(stored);
        }
        fs::create_dir_all(target.parent().expect("store paths have a parent"))?;
        match placement {
            Placement::Link if fs::hard_link(source, &target).is_ok() => {}
            _ => copy_atomic(source, &target)?,
        }
        Ok(stored)
    }

    /// Puts a stored file at `dest`, replacing whatever is there.
    pub fn place(&self, hash: &str, dest: &Path, placement: Placement) -> Result<()> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::remove_file(dest) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        let source = self.path_of(hash);
        match placement {
            Placement::Link if fs::hard_link(&source, dest).is_ok() => {}
            _ => copy_atomic(&source, dest)?,
        }
        Ok(())
    }

    pub fn verify(&self, hash: &str) -> bool {
        hash_file(&self.path_of(hash)).is_ok_and(|stored| stored.hash == hash)
    }

    /// Deletes everything no waymark or instance refers to any more, and
    /// reports the bytes freed.
    pub fn collect_garbage(&self, live: &HashSet<String>) -> Result<u64> {
        let mut freed = 0;
        for entry in WalkDir::new(&self.root).min_depth(2).max_depth(2) {
            let entry = entry.map_err(io::Error::from)?;
            let Some(name) = entry.file_name().to_str() else {
                continue;
            };
            if entry.file_type().is_file() && !live.contains(name) {
                freed += entry.metadata().map_err(io::Error::from)?.len();
                fs::remove_file(entry.path())?;
            }
        }
        Ok(freed)
    }
}

fn copy_atomic(source: &Path, target: &Path) -> Result<()> {
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let part = target.with_file_name(format!("{name}.{}.part", uuid::Uuid::new_v4().simple()));
    if let Err(error) = fs::copy(source, &part).and_then(|_| fs::rename(&part, target)) {
        let _ = fs::remove_file(&part);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("store"));
        (dir, store)
    }

    #[test]
    fn identical_files_are_stored_once() {
        let (dir, store) = setup();
        let (a, b) = (dir.path().join("a.jar"), dir.path().join("b.jar"));
        fs::write(&a, b"same bytes").unwrap();
        fs::write(&b, b"same bytes").unwrap();
        assert_eq!(
            store.add(&a, Placement::Link).unwrap(),
            store.add(&b, Placement::Link).unwrap()
        );
    }

    #[test]
    fn copied_placement_cannot_alter_the_store() {
        let (dir, store) = setup();
        let source = dir.path().join("mod.toml");
        fs::write(&source, b"original").unwrap();
        let stored = store.add(&source, Placement::Copy).unwrap();
        let placed = dir.path().join("placed.toml");
        store.place(&stored.hash, &placed, Placement::Copy).unwrap();
        fs::write(&placed, b"edited by the game").unwrap();
        assert!(store.verify(&stored.hash));
    }

    #[test]
    fn linked_placement_replaces_an_existing_file() {
        let (dir, store) = setup();
        let source = dir.path().join("mod.jar");
        fs::write(&source, b"new").unwrap();
        let stored = store.add(&source, Placement::Copy).unwrap();
        let placed = dir.path().join("placed.jar");
        fs::write(&placed, b"old").unwrap();
        store.place(&stored.hash, &placed, Placement::Link).unwrap();
        assert_eq!(fs::read(&placed).unwrap(), b"new");
    }

    #[test]
    fn garbage_collection_keeps_live_files() {
        let (dir, store) = setup();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, b"keep").unwrap();
        fs::write(&b, b"drop").unwrap();
        let keep = store.add(&a, Placement::Copy).unwrap();
        let drop = store.add(&b, Placement::Copy).unwrap();
        let freed = store
            .collect_garbage(&HashSet::from([keep.hash.clone()]))
            .unwrap();
        assert_eq!(freed, 4);
        assert!(store.contains(&keep.hash));
        assert!(!store.contains(&drop.hash));
    }

    #[test]
    fn placement_follows_file_type() {
        assert_eq!(Placement::for_path("mods/sodium.jar"), Placement::Link);
        assert_eq!(
            Placement::for_path("mods/sodium.jar.disabled"),
            Placement::Link
        );
        assert_eq!(Placement::for_path("config/sodium.json"), Placement::Copy);
    }
}

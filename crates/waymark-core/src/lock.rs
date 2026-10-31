//! The lockfile: a complete, portable description of an instance. Every
//! feature that crosses instances or machines works by moving this file
//! around, so game files themselves never need to be redistributed.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::{Placement, Store};

pub const LOCK_FORMAT: u32 = 1;
pub const LOCK_FILE: &str = "waymark.lock";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderKind {
    pub fn slug(self) -> &'static str {
        match self {
            LoaderKind::Vanilla => "minecraft",
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loader {
    pub kind: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum Source {
    Modrinth { project: String, version: String },
    CurseForge { project: u64, file: u64 },
    Manual,
}

/// Why a mod is here. This lets pack updates leave your own additions alone
/// and lets unused dependencies be removed along with what needed them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum Origin {
    Manual,
    Pack { name: String },
    Dependency { of: Vec<String> },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Both,
    Client,
    Server,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedMod {
    pub name: String,
    pub file: String,
    pub hash: String,
    pub size: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub source: Source,
    pub origin: Origin,
    #[serde(default)]
    pub side: Side,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Hashes of the mods this one needs, used to keep dependencies switched
    /// on together during a bisect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// The file's hash as published on Modrinth, which is how one request
    /// can check a whole instance for updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_hash: Option<String>,
}

fn enabled() -> bool {
    true
}

impl LockedMod {
    /// The file's name on disk, which carries the enabled state.
    pub fn disk_name(&self) -> String {
        if self.enabled {
            self.file.clone()
        } else {
            format!("{}.disabled", self.file)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lockfile {
    pub format: u32,
    pub minecraft: String,
    pub loader: Loader,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<LockedMod>,
}

impl Lockfile {
    pub fn new(minecraft: impl Into<String>, loader: Loader) -> Self {
        Lockfile { format: LOCK_FORMAT, minecraft: minecraft.into(), loader, mods: Vec::new() }
    }

    pub fn load(instance_dir: &Path) -> Result<Self> {
        Ok(toml::from_str(&fs::read_to_string(instance_dir.join(LOCK_FILE))?)?)
    }

    pub fn save(&self, instance_dir: &Path) -> Result<()> {
        let path = instance_dir.join(LOCK_FILE);
        let part = instance_dir.join(format!("{LOCK_FILE}.part"));
        fs::write(&part, toml::to_string_pretty(self)?)?;
        fs::rename(part, path)?;
        Ok(())
    }

    pub fn find(&self, hash: &str) -> Option<&LockedMod> {
        self.mods.iter().find(|m| m.hash == hash)
    }

    /// Brings the lockfile in line with the `mods` folder, so jars dropped in
    /// by hand are tracked and ones deleted by hand are forgotten.
    pub fn reconcile(&mut self, game_dir: &Path, store: &Store) -> Result<bool> {
        let mods_dir = game_dir.join("mods");
        let mut on_disk = Vec::new();
        if mods_dir.is_dir() {
            for entry in fs::read_dir(&mods_dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type()?.is_file() && is_mod_file(&name) {
                    on_disk.push(name);
                }
            }
        }

        let mut changed = false;
        let before = self.mods.len();
        self.mods.retain_mut(|m| {
            if on_disk.contains(&m.disk_name()) {
                return true;
            }
            let other_state = if m.enabled { format!("{}.disabled", m.file) } else { m.file.clone() };
            if on_disk.contains(&other_state) {
                m.enabled = !m.enabled;
                changed = true;
                return true;
            }
            false
        });
        changed |= before != self.mods.len();

        for name in on_disk {
            if self.mods.iter().any(|m| m.disk_name() == name) {
                continue;
            }
            let path = mods_dir.join(&name);
            let stored = store.add(&path, Placement::Link)?;
            let enabled = !name.ends_with(".disabled");
            let file = name.trim_end_matches(".disabled").to_string();
            let info = crate::modinfo::read(&path).unwrap_or_default();
            self.mods.push(LockedMod {
                name: info.name.unwrap_or_else(|| display_name(&file)),
                file,
                hash: stored.hash,
                size: stored.size,
                enabled,
                source: Source::Manual,
                origin: Origin::Manual,
                side: Side::Both,
                version: info.version,
                icon: None,
                group: None,
                tags: Vec::new(),
                requires: Vec::new(),
                remote_hash: None,
            });
            changed = true;
        }
        Ok(changed)
    }
}

fn is_mod_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".jar") || lower.ends_with(".jar.disabled")
}

pub(crate) fn display_name(file: &str) -> String {
    file.trim_end_matches(".jar").replace(['-', '_'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Lockfile {
        let mut lock = Lockfile::new("1.21.4", Loader { kind: LoaderKind::Fabric, version: Some("0.16.9".into()) });
        lock.mods.push(LockedMod {
            name: "Sodium".into(),
            file: "sodium.jar".into(),
            hash: "abc123".into(),
            size: 42,
            enabled: true,
            source: Source::Modrinth { project: "AANobbMI".into(), version: "xyz".into() },
            origin: Origin::Dependency { of: vec!["Iris".into()] },
            side: Side::Client,
            version: Some("0.6".into()),
            icon: None,
            group: Some("Performance".into()),
            tags: vec!["render".into()],
            requires: vec![],
            remote_hash: None,
        });
        lock
    }

    #[test]
    fn survives_a_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let lock = sample();
        lock.save(dir.path()).unwrap();
        assert_eq!(Lockfile::load(dir.path()).unwrap(), lock);
    }

    #[test]
    fn reconcile_tracks_hand_placed_mods_and_forgets_deleted_ones() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("store"));
        let game = dir.path().join("game");
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::write(game.join("mods/hand-placed.jar"), b"jar").unwrap();
        let mut lock = sample();

        assert!(lock.reconcile(&game, &store).unwrap());

        assert_eq!(lock.mods.len(), 1);
        assert_eq!(lock.mods[0].file, "hand-placed.jar");
        assert_eq!(lock.mods[0].origin, Origin::Manual);
    }

    #[test]
    fn reconcile_notices_a_mod_disabled_outside_the_launcher() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("store"));
        let game = dir.path().join("game");
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::write(game.join("mods/sodium.jar.disabled"), b"jar").unwrap();
        let mut lock = sample();

        lock.reconcile(&game, &store).unwrap();

        assert_eq!(lock.mods.len(), 1);
        assert!(!lock.mods[0].enabled);
        assert_eq!(lock.mods[0].name, "Sodium");
    }
}

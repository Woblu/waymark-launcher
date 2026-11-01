//! The lockfile: a complete, portable description of an instance. Every
//! feature that crosses instances or machines works by moving this file
//! around, so game files themselves never need to be redistributed.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::error::Result;
use crate::modinfo::ModInfo;
use crate::store::{Placement, Store, Stored};

pub const LOCK_FORMAT: u32 = 1;
pub const LOCK_FILE: &str = "waymark.lock";

/// Every list in the lockfile.
pub const SHELVES: [ContentKind; 4] = [
    ContentKind::Mod,
    ContentKind::ResourcePack,
    ContentKind::Shader,
    ContentKind::DataPack,
];

/// How an unpacked folder is marked, since it has no hash of its own.
pub const UNPACKED: &str = "folder-";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
    LegacyFabric,
    Babric,
    Ornithe,
}

impl LoaderKind {
    pub fn slug(self) -> &'static str {
        match self {
            LoaderKind::Vanilla => "minecraft",
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
            LoaderKind::LegacyFabric => "legacy-fabric",
            LoaderKind::Babric => "babric",
            LoaderKind::Ornithe => "ornithe",
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
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Source {
    Modrinth { project: String, version: String },
    CurseForge { project: u64, file: u64 },
    Manual,
}

/// Why a mod is here. This lets pack updates leave your own additions alone
/// and lets unused dependencies be removed along with what needed them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_packs: Vec<LockedMod>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shaders: Vec<LockedMod>,
    /// Data packs belong to a world, so each one's `group` is its world's folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_packs: Vec<LockedMod>,
}

/// Where an entry's file lives inside the game folder.
pub fn content_folder(kind: ContentKind, entry: &LockedMod) -> PathBuf {
    match (kind, &entry.group) {
        (ContentKind::ResourcePack, _) => PathBuf::from("resourcepacks"),
        (ContentKind::Shader, _) => PathBuf::from("shaderpacks"),
        (ContentKind::DataPack, Some(world)) => Path::new("saves").join(world).join("datapacks"),
        _ => PathBuf::from("mods"),
    }
}

impl Lockfile {
    pub fn new(minecraft: impl Into<String>, loader: Loader) -> Self {
        Lockfile {
            format: LOCK_FORMAT,
            minecraft: minecraft.into(),
            loader,
            mods: Vec::new(),
            resource_packs: Vec::new(),
            shaders: Vec::new(),
            data_packs: Vec::new(),
        }
    }

    /// The list a kind of content is kept in.
    pub fn shelf(&self, kind: ContentKind) -> &[LockedMod] {
        match kind {
            ContentKind::ResourcePack => &self.resource_packs,
            ContentKind::Shader => &self.shaders,
            ContentKind::DataPack => &self.data_packs,
            ContentKind::Mod | ContentKind::Modpack => &self.mods,
        }
    }

    pub fn shelf_mut(&mut self, kind: ContentKind) -> &mut Vec<LockedMod> {
        match kind {
            ContentKind::ResourcePack => &mut self.resource_packs,
            ContentKind::Shader => &mut self.shaders,
            ContentKind::DataPack => &mut self.data_packs,
            ContentKind::Mod | ContentKind::Modpack => &mut self.mods,
        }
    }

    /// Everything in the instance, with what kind each entry is.
    pub fn everything(&self) -> impl Iterator<Item = (ContentKind, &LockedMod)> {
        SHELVES
            .into_iter()
            .flat_map(|kind| self.shelf(kind).iter().map(move |entry| (kind, entry)))
    }

    pub fn load(instance_dir: &Path) -> Result<Self> {
        Ok(toml::from_str(&fs::read_to_string(
            instance_dir.join(LOCK_FILE),
        )?)?)
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

    /// Brings the lockfile in line with the folders on disk, so files dropped
    /// in by hand are tracked and ones deleted by hand are forgotten.
    pub fn reconcile(&mut self, game_dir: &Path, store: &Store) -> Result<bool> {
        let mods = reconcile_shelf(&mut self.mods, &game_dir.join("mods"), None, store)?;
        let resource_packs = reconcile_shelf(
            &mut self.resource_packs,
            &game_dir.join("resourcepacks"),
            None,
            store,
        )?;
        let shaders = reconcile_shelf(
            &mut self.shaders,
            &game_dir.join("shaderpacks"),
            None,
            store,
        )?;

        let mut data_packs = false;
        let mut worlds = Vec::new();
        if let Ok(saves) = fs::read_dir(game_dir.join("saves")) {
            worlds.extend(
                saves.filter_map(|entry| {
                    Some(entry.ok()?.file_name().to_string_lossy().into_owned())
                }),
            );
        }
        let before = self.data_packs.len();
        self.data_packs.retain(|pack| {
            pack.group
                .as_ref()
                .is_some_and(|world| worlds.contains(world))
        });
        data_packs |= before != self.data_packs.len();
        for world in worlds {
            let folder = game_dir.join("saves").join(&world).join("datapacks");
            let mut here: Vec<LockedMod> = self
                .data_packs
                .extract_if(.., |pack| pack.group.as_ref() == Some(&world))
                .collect();
            data_packs |= reconcile_shelf(&mut here, &folder, Some(&world), store)?;
            self.data_packs.append(&mut here);
        }
        Ok(mods || resource_packs || shaders || data_packs)
    }
}

/// Matches one list against its folder. Packs can be unpacked folders, which
/// aren't kept in the store because players edit them in place.
fn reconcile_shelf(
    list: &mut Vec<LockedMod>,
    folder: &Path,
    world: Option<&str>,
    store: &Store,
) -> Result<bool> {
    let jars = folder.ends_with("mods");
    let mut on_disk = Vec::new();
    if folder.is_dir() {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = fs::metadata(entry.path())?;
            if (kind.is_file() && is_content_file(&name, jars)) || (kind.is_dir() && !jars) {
                on_disk.push(name);
            }
        }
    }

    let mut changed = false;
    let before = list.len();
    list.retain_mut(|m| {
        if on_disk.contains(&m.disk_name()) {
            return true;
        }
        let other_state = if m.enabled {
            format!("{}.disabled", m.file)
        } else {
            m.file.clone()
        };
        if on_disk.contains(&other_state) {
            m.enabled = !m.enabled;
            changed = true;
            return true;
        }
        false
    });
    changed |= before != list.len();

    for name in on_disk {
        if list.iter().any(|m| m.disk_name() == name) {
            continue;
        }
        let path = folder.join(&name);
        let enabled = !name.ends_with(".disabled");
        let file = name.trim_end_matches(".disabled").to_string();
        let stored = if path.is_dir() {
            Stored {
                hash: format!("{UNPACKED}{file}"),
                size: 0,
            }
        } else {
            store.add(&path, Placement::Link)?
        };
        let info = if jars {
            crate::modinfo::read(&path).unwrap_or_default()
        } else {
            ModInfo::default()
        };
        list.push(LockedMod {
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
            group: world.map(String::from),
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        });
        changed = true;
    }
    Ok(changed)
}

fn is_content_file(name: &str, jars: bool) -> bool {
    let lower = name.to_ascii_lowercase();
    let ending = if jars { ".jar" } else { ".zip" };
    lower.ends_with(ending) || lower.ends_with(&format!("{ending}.disabled"))
}

pub(crate) fn display_name(file: &str) -> String {
    file.trim_end_matches(".jar")
        .trim_end_matches(".zip")
        .replace(['-', '_'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Lockfile {
        let mut lock = Lockfile::new(
            "1.21.4",
            Loader {
                kind: LoaderKind::Fabric,
                version: Some("0.16.9".into()),
            },
        );
        lock.mods.push(LockedMod {
            name: "Sodium".into(),
            file: "sodium.jar".into(),
            hash: "abc123".into(),
            size: 42,
            enabled: true,
            source: Source::Modrinth {
                project: "AANobbMI".into(),
                version: "xyz".into(),
            },
            origin: Origin::Dependency {
                of: vec!["Iris".into()],
            },
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
    fn reconcile_tracks_packs_of_every_kind_and_their_worlds() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("store"));
        let game = dir.path().join("game");
        fs::create_dir_all(game.join("resourcepacks/Unpacked")).unwrap();
        fs::create_dir_all(game.join("shaderpacks")).unwrap();
        fs::create_dir_all(game.join("saves/Home/datapacks")).unwrap();
        fs::write(game.join("resourcepacks/Faithful-32x.zip"), b"zip").unwrap();
        fs::write(game.join("resourcepacks/notes.txt"), b"not a pack").unwrap();
        fs::write(game.join("shaderpacks/Complementary.zip.disabled"), b"zip").unwrap();
        fs::write(
            game.join("shaderpacks/Complementary.txt"),
            b"shader settings",
        )
        .unwrap();
        fs::write(game.join("saves/Home/datapacks/terralith.zip"), b"zip").unwrap();
        let mut lock = Lockfile::new("1.21.4", Loader::default());

        assert!(lock.reconcile(&game, &store).unwrap());

        let names: Vec<&str> = lock
            .resource_packs
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"Faithful 32x") && names.contains(&"Unpacked"));
        assert!(
            lock.resource_packs
                .iter()
                .any(|p| p.hash == "folder-Unpacked")
        );
        assert_eq!(lock.shaders.len(), 1);
        assert!(!lock.shaders[0].enabled);
        assert_eq!(lock.data_packs[0].group.as_deref(), Some("Home"));
        assert_eq!(
            content_folder(ContentKind::DataPack, &lock.data_packs[0]),
            Path::new("saves/Home/datapacks")
        );

        assert!(
            !lock.reconcile(&game, &store).unwrap(),
            "nothing changed the second time"
        );
        fs::remove_dir_all(game.join("saves/Home")).unwrap();
        assert!(lock.reconcile(&game, &store).unwrap());
        assert!(
            lock.data_packs.is_empty(),
            "a deleted world takes its data packs with it"
        );
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

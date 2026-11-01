//! Deleting moves things here instead of erasing them, so a misclick is never
//! final. Everything stays for 30 days, then goes at the next start or clean up.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::Core;
use crate::error::{Error, Result};
use crate::lock::Lockfile;
use crate::waymarks::Trail;

const ITEM_FILE: &str = "item.json";
const FILES_DIR: &str = "files";
const KEEP_FOR: Duration = Duration::days(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrashKind {
    Instance,
    World,
    Screenshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trashed {
    pub id: String,
    pub kind: TrashKind,
    pub name: String,
    /// The instance something came from. For a world, it goes back there.
    pub instance: String,
    /// What it was called on disk, so it can go back to the same place.
    pub folder: String,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

impl Core {
    pub(crate) fn send_to_trash(
        &self,
        path: &Path,
        kind: TrashKind,
        name: &str,
        instance: Option<&str>,
    ) -> Result<Trashed> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let folder = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let item = Trashed {
            id: id.clone(),
            kind,
            name: name.to_string(),
            instance: instance.map_or_else(|| folder.clone(), String::from),
            folder,
            at: OffsetDateTime::now_utc(),
        };
        let home = self.paths.trash().join(&id);
        fs::create_dir_all(&home)?;
        fs::write(home.join(ITEM_FILE), serde_json::to_vec(&item)?)?;
        if let Err(error) = fs::rename(path, home.join(FILES_DIR)) {
            let _ = fs::remove_dir_all(&home);
            return Err(error.into());
        }
        Ok(item)
    }

    /// What can still be restored, newest first.
    pub fn trashed(&self) -> Result<Vec<Trashed>> {
        let Ok(entries) = fs::read_dir(self.paths.trash()) else {
            return Ok(Vec::new());
        };
        let mut items: Vec<Trashed> = entries
            .filter_map(|entry| {
                serde_json::from_slice(&fs::read(entry.ok()?.path().join(ITEM_FILE)).ok()?).ok()
            })
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.at));
        Ok(items)
    }

    /// Puts something back. A name that's been reused since gets a new one, so
    /// restoring never overwrites what's there now.
    pub fn restore_trashed(&self, id: &str) -> Result<Trashed> {
        let item = self
            .trashed()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| Error::NotFound("that deleted item".into()))?;
        let home = self.paths.trash().join(id);
        match item.kind {
            TrashKind::Instance => {
                let target = self.free_path(&self.paths.instances(), &item.folder);
                fs::create_dir_all(self.paths.instances())?;
                fs::rename(home.join(FILES_DIR), &target)?;
                let new_id = target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if new_id != item.folder {
                    let mut instance = self.instance(&new_id)?;
                    instance.id = new_id;
                    self.save_instance(&instance)?;
                }
            }
            TrashKind::World | TrashKind::Screenshot => {
                self.instance(&item.instance).map_err(|_| {
                    Error::Other(format!("the instance {} no longer exists", item.instance))
                })?;
                let folder = if item.kind == TrashKind::World {
                    "saves"
                } else {
                    "screenshots"
                };
                let parent = self.paths.game(&item.instance).join(folder);
                fs::create_dir_all(&parent)?;
                fs::rename(home.join(FILES_DIR), self.free_path(&parent, &item.folder))?;
            }
        }
        fs::remove_dir_all(home)?;
        Ok(item)
    }

    fn free_path(&self, parent: &Path, name: &str) -> std::path::PathBuf {
        let wanted = parent.join(name);
        if !wanted.exists() {
            return wanted;
        }
        (2..)
            .map(|n| parent.join(format!("{name}-{n}")))
            .find(|path| !path.exists())
            .expect("names are unbounded")
    }

    /// Erases one item for good.
    pub fn erase_trashed(&self, id: &str) -> Result<()> {
        if !self.trashed()?.iter().any(|item| item.id == id) {
            return Err(Error::NotFound("that deleted item".into()));
        }
        fs::remove_dir_all(self.paths.trash().join(id))?;
        Ok(())
    }

    /// Erases what's been here longer than the 30 days it was promised.
    pub fn purge_trash(&self) -> Result<()> {
        let oldest = OffsetDateTime::now_utc() - KEEP_FOR;
        for item in self.trashed()?.into_iter().filter(|item| item.at < oldest) {
            fs::remove_dir_all(self.paths.trash().join(item.id))?;
        }
        Ok(())
    }

    /// A deleted instance can come back, so its stored files have to stay.
    pub(crate) fn trash_hashes(&self) -> Result<HashSet<String>> {
        let mut live = HashSet::new();
        for item in self
            .trashed()?
            .into_iter()
            .filter(|item| item.kind == TrashKind::Instance)
        {
            let files = self.paths.trash().join(&item.id).join(FILES_DIR);
            live.extend(Trail::new(&files, self.store.clone()).live_hashes()?);
            if let Ok(lock) = Lockfile::load(&files) {
                live.extend(lock.everything().map(|(_, entry)| entry.hash.clone()));
            }
        }
        Ok(live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;
    use crate::store::Placement;

    fn setup() -> (tempfile::TempDir, Core, String) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Test", "1.21.4", Loader::default(), "1.21.4".into(), None)
            .unwrap();
        (dir, core, instance.id)
    }

    #[test]
    fn a_deleted_instance_comes_back_whole() {
        let (_dir, core, id) = setup();
        fs::write(core.paths.game(&id).join("mods/keep.jar"), b"jar").unwrap();

        core.delete_instance(&id).unwrap();
        assert!(core.instances().unwrap().is_empty());

        let item = core.trashed().unwrap().remove(0);
        assert_eq!(item.name, "Test");
        assert_eq!(item.kind, TrashKind::Instance);
        core.restore_trashed(&item.id).unwrap();

        assert_eq!(core.instances().unwrap().len(), 1);
        assert!(core.paths.game(&id).join("mods/keep.jar").is_file());
        assert!(core.trashed().unwrap().is_empty());
    }

    #[test]
    fn restoring_never_overwrites_a_reused_name() {
        let (_dir, core, id) = setup();
        core.delete_instance(&id).unwrap();
        let again = core
            .register_instance("Test", "1.21.4", Loader::default(), "1.21.4".into(), None)
            .unwrap();
        assert_eq!(again.id, id);

        let item = core.trashed().unwrap().remove(0);
        core.restore_trashed(&item.id).unwrap();

        let mut ids: Vec<String> = core
            .instances()
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        ids.sort();
        assert_eq!(ids, [id.clone(), format!("{id}-2")]);
    }

    #[test]
    fn a_world_goes_back_to_its_instance() {
        let (_dir, core, id) = setup();
        let world = core.paths.game(&id).join("saves/Home");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), b"x").unwrap();

        let item = core.delete_world(&id, "Home").unwrap();
        assert!(!world.exists());
        assert_eq!(item.kind, TrashKind::World);
        core.restore_trashed(&item.id).unwrap();
        assert!(world.join("level.dat").is_file());
    }

    #[test]
    fn old_items_go_and_recent_ones_stay() {
        let (_dir, core, id) = setup();
        core.delete_instance(&id).unwrap();
        let item = core.trashed().unwrap().remove(0);
        core.purge_trash().unwrap();
        assert_eq!(core.trashed().unwrap().len(), 1);

        let path = core.paths.trash().join(&item.id).join(ITEM_FILE);
        let old = Trashed {
            at: OffsetDateTime::now_utc() - Duration::days(31),
            ..item
        };
        fs::write(path, serde_json::to_vec(&old).unwrap()).unwrap();
        core.purge_trash().unwrap();
        assert!(core.trashed().unwrap().is_empty());
    }

    #[test]
    fn cleaning_up_keeps_what_a_deleted_instance_needs() {
        let (dir, core, id) = setup();
        let jar = dir.path().join("mod.jar");
        fs::write(&jar, b"jar").unwrap();
        let stored = core.store.add(&jar, Placement::Copy).unwrap();
        let mut lock = core.lockfile(&id).unwrap();
        lock.mods.push(crate::lock::LockedMod {
            name: "m".into(),
            file: "m.jar".into(),
            hash: stored.hash.clone(),
            size: 3,
            enabled: true,
            source: crate::lock::Source::Manual,
            origin: crate::lock::Origin::Manual,
            side: crate::lock::Side::Both,
            version: None,
            icon: None,
            group: None,
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        });
        lock.save(&core.paths.instance(&id)).unwrap();

        core.delete_instance(&id).unwrap();
        core.reclaim_space().unwrap();

        assert!(core.store.contains(&stored.hash));
    }
}

//! What the UI needs to draw, gathered in one call so opening the launcher
//! doesn't take a separate request for every instance.

use std::collections::{BTreeMap, HashSet};
use std::fs;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::Core;
use crate::bisect::{Bisect, Step, Verdict};
use crate::error::{Error, Result};
use crate::instance::{Instance, LaunchOptions};
use crate::lock::Lockfile;
use crate::waymarks::WaymarkKind;

const BISECT_FILE: &str = ".waymarks/bisect.json";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    #[serde(flatten)]
    pub instance: Instance,
    pub mods: usize,
    pub waymarks: usize,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_waymark: Option<OffsetDateTime>,
    pub running: bool,
    pub bisecting: bool,
    /// The store project this instance's modpack came from, as `source:id`.
    pub pack_project: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePatch {
    pub name: Option<String>,
    pub memory_mb: Option<Option<u32>>,
    pub java_path: Option<Option<String>>,
    pub jvm_args: Option<Vec<String>>,
    pub options: Option<LaunchOptions>,
}

/// One point in a bisect, in terms the player can act on.
#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BisectView {
    Testing {
        round: u32,
        estimate: u32,
        enabled: usize,
        total: usize,
    },
    Found {
        culprits: Vec<String>,
    },
    NotAMod,
}

#[derive(Serialize, Deserialize)]
struct SavedBisect {
    search: Bisect,
    /// Which mods were on before the search began.
    original: Vec<String>,
}

impl Core {
    pub fn library(&self) -> Result<Vec<LibraryEntry>> {
        let mut entries = Vec::new();
        for instance in self.instances()? {
            let lock = self.lockfile(&instance.id)?;
            let trail = self.trail(&instance.id).list()?;
            entries.push(LibraryEntry {
                mods: lock.mods.len(),
                waymarks: trail.len(),
                last_waymark: trail.last().map(|w| w.at),
                running: self.game_running(&instance.id),
                bisecting: self
                    .paths
                    .instance(&instance.id)
                    .join(BISECT_FILE)
                    .is_file(),
                pack_project: self.pack_project(&instance.id),
                instance,
            });
        }
        Ok(entries)
    }

    pub fn update_instance(&self, id: &str, patch: InstancePatch) -> Result<Instance> {
        let mut instance = self.instance(id)?;
        if let Some(name) = patch.name {
            instance.name = name;
        }
        if let Some(memory) = patch.memory_mb {
            instance.memory_mb = memory;
        }
        if let Some(java) = patch.java_path {
            instance.java_path = java;
        }
        if let Some(args) = patch.jvm_args {
            instance.jvm_args = args;
        }
        if let Some(options) = patch.options {
            instance.options = options;
        }
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Switches exactly the mods in `enabled` on and the rest off.
    fn apply_mod_set(&self, id: &str, enabled: &HashSet<&str>) -> Result<()> {
        let mut lock = self.lockfile(id)?;
        let mods_dir = self.paths.game(id).join("mods");
        for entry in &mut lock.mods {
            let wanted = enabled.contains(entry.hash.as_str());
            if entry.enabled != wanted {
                let from = mods_dir.join(entry.disk_name());
                entry.enabled = wanted;
                fs::rename(from, mods_dir.join(entry.disk_name()))?;
            }
        }
        lock.save(&self.paths.instance(id))
    }

    fn apply_step(
        &self,
        id: &str,
        step: &Step,
        total: usize,
        lock: &Lockfile,
    ) -> Result<BisectView> {
        match step {
            Step::Test {
                enabled,
                round,
                estimate,
                ..
            } => {
                self.apply_mod_set(id, &enabled.iter().map(String::as_str).collect())?;
                Ok(BisectView::Testing {
                    round: *round,
                    estimate: *estimate,
                    enabled: enabled.len(),
                    total,
                })
            }
            Step::Found { culprits } => Ok(BisectView::Found {
                culprits: culprits
                    .iter()
                    .filter_map(|hash| lock.find(hash).map(|m| m.name.clone()))
                    .collect(),
            }),
            Step::NotAMod => Ok(BisectView::NotAMod),
        }
    }

    pub fn start_bisect(&self, id: &str) -> Result<BisectView> {
        self.trail(id)
            .mark("Before searching for a crash", WaymarkKind::Automatic)?;
        let lock = self.lockfile(id)?;
        let active: Vec<_> = lock.mods.iter().filter(|m| m.enabled).collect();
        if active.is_empty() {
            return Err(Error::Other("this instance has no mods switched on".into()));
        }
        let requires: BTreeMap<String, Vec<String>> = active
            .iter()
            .map(|m| (m.hash.clone(), m.requires.clone()))
            .collect();
        let original: Vec<String> = active.iter().map(|m| m.hash.clone()).collect();
        let search = Bisect::new(original.clone(), requires);
        let view = self.apply_step(id, &search.step(), original.len(), &lock)?;
        self.save_bisect(id, &SavedBisect { search, original })?;
        Ok(view)
    }

    pub fn report_bisect(&self, id: &str, verdict: Verdict) -> Result<BisectView> {
        let mut saved = self.load_bisect(id)?;
        let lock = self.lockfile(id)?;
        let step = saved.search.report(verdict);
        let view = self.apply_step(id, &step, saved.original.len(), &lock)?;
        if matches!(step, Step::Test { .. }) {
            self.save_bisect(id, &saved)?;
        } else {
            let keep: HashSet<&str> = saved.original.iter().map(String::as_str).collect();
            let culprits: HashSet<&str> = match &step {
                Step::Found { culprits } => culprits.iter().map(String::as_str).collect(),
                _ => HashSet::new(),
            };
            // The culprit stays off, so the instance works again straight away.
            self.apply_mod_set(id, &keep.difference(&culprits).copied().collect())?;
            let _ = fs::remove_file(self.paths.instance(id).join(BISECT_FILE));
        }
        Ok(view)
    }

    /// Gives up on a search and puts every mod back as it was.
    pub fn cancel_bisect(&self, id: &str) -> Result<()> {
        let saved = self.load_bisect(id)?;
        self.apply_mod_set(id, &saved.original.iter().map(String::as_str).collect())?;
        fs::remove_file(self.paths.instance(id).join(BISECT_FILE))?;
        Ok(())
    }

    pub fn bisect_status(&self, id: &str) -> Result<Option<BisectView>> {
        if !self.paths.instance(id).join(BISECT_FILE).is_file() {
            return Ok(None);
        }
        let saved = self.load_bisect(id)?;
        let lock = self.lockfile(id)?;
        match saved.search.step() {
            Step::Test {
                enabled,
                round,
                estimate,
                ..
            } => Ok(Some(BisectView::Testing {
                round,
                estimate,
                enabled: enabled.len(),
                total: saved.original.len(),
            })),
            step => self
                .apply_step(id, &step, saved.original.len(), &lock)
                .map(Some),
        }
    }

    fn save_bisect(&self, id: &str, saved: &SavedBisect) -> Result<()> {
        fs::write(
            self.paths.instance(id).join(BISECT_FILE),
            serde_json::to_vec(saved)?,
        )?;
        Ok(())
    }

    fn load_bisect(&self, id: &str) -> Result<SavedBisect> {
        let path = self.paths.instance(id).join(BISECT_FILE);
        if !path.is_file() {
            return Err(Error::NotFound("a running crash search".into()));
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{Loader, LoaderKind, LockedMod, Origin, Side, Source};

    fn locked(name: &str) -> LockedMod {
        LockedMod {
            name: name.into(),
            file: format!("{name}.jar"),
            hash: name.into(),
            size: 1,
            enabled: true,
            source: Source::Manual,
            origin: Origin::Manual,
            side: Side::Both,
            version: None,
            icon: None,
            group: None,
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        }
    }

    fn setup(names: &[&str]) -> (tempfile::TempDir, Core, String) {
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
        for name in names {
            fs::write(mods_dir.join(format!("{name}.jar")), name.as_bytes()).unwrap();
            lock.mods.push(locked(name));
        }
        lock.save(&core.paths.instance(&instance.id)).unwrap();
        (dir, core, instance.id)
    }

    fn enabled(core: &Core, id: &str) -> Vec<String> {
        core.lockfile(id)
            .unwrap()
            .mods
            .into_iter()
            .filter(|m| m.enabled)
            .map(|m| m.name)
            .collect()
    }

    #[test]
    fn a_search_runs_end_to_end_and_leaves_the_culprit_off() {
        let (_dir, core, id) = setup(&["a", "b", "bad", "d"]);
        let mut view = core.start_bisect(&id).unwrap();
        while let BisectView::Testing { .. } = view {
            let verdict = if enabled(&core, &id).contains(&"bad".to_string()) {
                Verdict::Crashes
            } else {
                Verdict::Works
            };
            view = core.report_bisect(&id, verdict).unwrap();
        }
        let BisectView::Found { culprits } = view else {
            panic!("expected a culprit")
        };
        assert_eq!(culprits, ["bad"]);
        assert_eq!(enabled(&core, &id), ["a", "b", "d"]);
        assert!(core.bisect_status(&id).unwrap().is_none());
    }

    #[test]
    fn cancelling_puts_every_mod_back() {
        let (_dir, core, id) = setup(&["a", "b", "c", "d"]);
        core.start_bisect(&id).unwrap();
        assert!(enabled(&core, &id).is_empty());
        core.cancel_bisect(&id).unwrap();
        assert_eq!(enabled(&core, &id).len(), 4);
    }

    #[test]
    fn a_running_search_survives_a_restart() {
        let (dir, core, id) = setup(&["a", "b"]);
        core.start_bisect(&id).unwrap();
        let reopened = Core::new(dir.path(), crate::event::silent()).unwrap();
        assert!(matches!(
            reopened.bisect_status(&id).unwrap(),
            Some(BisectView::Testing { .. })
        ));
    }

    #[test]
    fn the_library_summarises_each_instance() {
        let (_dir, core, id) = setup(&["a", "b"]);
        core.trail(&id).mark("first", WaymarkKind::Manual).unwrap();
        let entries = core.library().unwrap();
        assert_eq!(entries[0].mods, 2);
        assert_eq!(entries[0].waymarks, 1);
        assert!(entries[0].last_waymark.is_some());
    }
}

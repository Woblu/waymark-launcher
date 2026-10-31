//! Acting on many mods at once: switching them, grouping them and tagging them.

use std::fs;

use crate::Core;
use crate::error::Result;
use crate::lock::LockedMod;

impl Core {
    /// Switches several mods on or off in one step.
    pub fn set_mods_enabled(&self, id: &str, hashes: &[String], enabled: bool) -> Result<Vec<LockedMod>> {
        let mut lock = self.lockfile(id)?;
        let mods_dir = self.paths.game(id).join("mods");
        for entry in lock.mods.iter_mut().filter(|m| hashes.contains(&m.hash) && m.enabled != enabled) {
            let from = mods_dir.join(entry.disk_name());
            entry.enabled = enabled;
            fs::rename(from, mods_dir.join(entry.disk_name()))?;
        }
        lock.save(&self.paths.instance(id))?;
        Ok(lock.mods)
    }

    /// Puts mods in a group, or takes them out of one with `None`.
    pub fn set_mods_group(&self, id: &str, hashes: &[String], group: Option<String>) -> Result<Vec<LockedMod>> {
        let mut lock = self.lockfile(id)?;
        let group = group.map(|g| g.trim().to_string()).filter(|g| !g.is_empty());
        for entry in lock.mods.iter_mut().filter(|m| hashes.contains(&m.hash)) {
            entry.group = group.clone();
        }
        lock.save(&self.paths.instance(id))?;
        Ok(lock.mods)
    }

    /// Adds a tag to several mods, or removes it from all of them if they
    /// already all have it.
    pub fn toggle_mods_tag(&self, id: &str, hashes: &[String], tag: &str) -> Result<Vec<LockedMod>> {
        let mut lock = self.lockfile(id)?;
        let tag = tag.trim();
        if tag.is_empty() {
            return Ok(lock.mods);
        }
        let chosen = |m: &LockedMod| hashes.contains(&m.hash);
        let all_have_it = lock.mods.iter().filter(|m| chosen(m)).all(|m| m.tags.iter().any(|t| t == tag));
        for entry in lock.mods.iter_mut().filter(|m| chosen(m)) {
            entry.tags.retain(|t| t != tag);
            if !all_have_it {
                entry.tags.push(tag.to_string());
            }
        }
        lock.save(&self.paths.instance(id))?;
        Ok(lock.mods)
    }
}

#[cfg(test)]
mod tests {
    use crate::Core;
    use crate::lock::{Loader, LockedMod, Origin, Side, Source};

    fn setup(names: &[&str]) -> (tempfile::TempDir, Core, String) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core.register_instance("Test", "1.21.4", Loader::default(), "x".into(), None).unwrap();
        let mods_dir = core.paths.game(&instance.id).join("mods");
        let mut lock = core.lockfile(&instance.id).unwrap();
        for name in names {
            std::fs::write(mods_dir.join(format!("{name}.jar")), name.as_bytes()).unwrap();
            lock.mods.push(LockedMod {
                name: name.to_string(),
                file: format!("{name}.jar"),
                hash: name.to_string(),
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
            });
        }
        lock.save(&core.paths.instance(&instance.id)).unwrap();
        (dir, core, instance.id)
    }

    fn hashes(names: &[&str]) -> Vec<String> {
        names.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn switches_several_mods_at_once() {
        let (_dir, core, id) = setup(&["a", "b", "c"]);
        let mods = core.set_mods_enabled(&id, &hashes(&["a", "b"]), false).unwrap();
        assert_eq!(mods.iter().filter(|m| !m.enabled).count(), 2);
        let folder = core.paths.game(&id).join("mods");
        assert!(folder.join("a.jar.disabled").is_file());
        assert!(folder.join("c.jar").is_file());
    }

    #[test]
    fn groups_mods_and_ungroups_them() {
        let (_dir, core, id) = setup(&["a", "b"]);
        let mods = core.set_mods_group(&id, &hashes(&["a"]), Some(" Performance ".into())).unwrap();
        assert_eq!(mods[0].group.as_deref(), Some("Performance"));
        assert_eq!(mods[1].group, None);
        let mods = core.set_mods_group(&id, &hashes(&["a"]), None).unwrap();
        assert_eq!(mods[0].group, None);
    }

    #[test]
    fn groups_are_saved_to_the_lockfile() {
        let (_dir, core, id) = setup(&["a"]);
        core.set_mods_group(&id, &hashes(&["a"]), Some("Visuals".into())).unwrap();
        assert_eq!(core.lockfile(&id).unwrap().mods[0].group.as_deref(), Some("Visuals"));
    }

    #[test]
    fn a_tag_toggles_across_a_selection() {
        let (_dir, core, id) = setup(&["a", "b"]);
        core.toggle_mods_tag(&id, &hashes(&["a"]), "client").unwrap();

        let mods = core.toggle_mods_tag(&id, &hashes(&["a", "b"]), "client").unwrap();
        assert!(mods.iter().all(|m| m.tags == ["client"]), "a mixed selection gets the tag");

        let mods = core.toggle_mods_tag(&id, &hashes(&["a", "b"]), "client").unwrap();
        assert!(mods.iter().all(|m| m.tags.is_empty()), "a selection that all has it loses it");
    }

    #[test]
    fn removing_several_mods_leaves_one_waymark() {
        let (_dir, core, id) = setup(&["a", "b", "c"]);
        core.trail(&id).mark("start", crate::waymarks::WaymarkKind::Manual).unwrap();
        let mark = core.remove_mods(&id, &hashes(&["a", "b"])).unwrap().unwrap();
        assert_eq!(mark.label, "Removed 2 mods");
        assert_eq!(core.lockfile(&id).unwrap().mods.len(), 1);
    }
}

//! Folders several instances can use at once, and copying settings between
//! instances. A shared folder is one real folder in the data directory, linked
//! into each instance, so a resource pack only has to be added once.

use std::fs;
use std::io;
use std::path::Path;

use walkdir::WalkDir;

use crate::Core;
use crate::error::{Error, Result};
use crate::instance::Instance;
use crate::waymarks::{TRACKED_FILES, WaymarkKind, WaymarkSummary};

pub const SHAREABLE: [&str; 3] = ["resourcepacks", "shaderpacks", "screenshots"];

#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) -> io::Result<()> {
    // Junctions need no special rights, unlike symlinks on Windows.
    junction::create(target, link)
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn unlink_dir(link: &Path) -> io::Result<()> {
    fs::remove_dir(link)
}

#[cfg(unix)]
fn unlink_dir(link: &Path) -> io::Result<()> {
    fs::remove_file(link)
}

/// Moves files from one folder into another. A file that exists on both sides
/// keeps the destination's copy, and the other is set aside under a new name.
fn move_contents(from: &Path, to: &Path, tag: &str) -> Result<()> {
    for entry in WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(from)
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut target = to.join(relative);
        if target.exists() {
            let name = target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            target.set_file_name(format!("{tag} {name}"));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::rename(entry.path(), &target).is_err() {
            fs::copy(entry.path(), &target)?;
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn copy_contents(from: &Path, to: &Path) -> Result<()> {
    for entry in WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(from)
            .map_err(|e| Error::Other(e.to_string()))?;
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), target)?;
    }
    Ok(())
}

impl Core {
    /// Starts or stops sharing a folder. Starting moves what's there into the
    /// shared folder. Stopping leaves the instance with its own copy, so
    /// nothing disappears from either side.
    pub fn set_folder_shared(&self, id: &str, folder: &str, shared: bool) -> Result<Instance> {
        if !SHAREABLE.contains(&folder) {
            return Err(Error::Other(format!("{folder} can't be shared")));
        }
        if self.game_running(id) {
            return Err(Error::Other(
                "close the game before changing shared folders".into(),
            ));
        }
        let mut instance = self.instance(id)?;
        let local = self.paths.game(id).join(folder);
        let common = self.paths.shared(folder);
        let linked = crate::waymarks::is_linked(&local);

        if shared && !linked {
            fs::create_dir_all(&common)?;
            if local.is_dir() {
                move_contents(&local, &common, &instance.name)?;
                fs::remove_dir_all(&local)?;
            }
            link_dir(&common, &local)?;
            instance.shared.retain(|f| f != folder);
            instance.shared.push(folder.to_string());
        } else if !shared && linked {
            unlink_dir(&local)?;
            fs::create_dir_all(&local)?;
            copy_contents(&common, &local)?;
            instance.shared.retain(|f| f != folder);
        }
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Copies game options and keybinds from one instance to another.
    pub fn copy_settings(&self, from: &str, to: &str) -> Result<Option<WaymarkSummary>> {
        if self.game_running(to) {
            return Err(Error::Other(
                "close the game before copying settings".into(),
            ));
        }
        let source = self.instance(from)?;
        self.instance(to)?;
        self.trail(to)
            .mark("Before copying settings", WaymarkKind::Automatic)?;
        let (from_dir, to_dir) = (self.paths.game(from), self.paths.game(to));
        let mut copied = 0;
        for file in TRACKED_FILES {
            if from_dir.join(file).is_file() {
                fs::copy(from_dir.join(file), to_dir.join(file))?;
                copied += 1;
            }
        }
        if copied == 0 {
            return Err(Error::Other(format!(
                "{} has no game settings yet",
                source.name
            )));
        }
        self.trail(to).mark(
            &format!("Copied settings from {}", source.name),
            WaymarkKind::Manual,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;

    fn setup() -> (tempfile::TempDir, Core, Instance, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let a = core
            .register_instance("A", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let b = core
            .register_instance("B", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        (dir, core, a, b)
    }

    #[test]
    fn a_shared_folder_shows_the_same_files_in_every_instance() {
        let (_dir, core, a, b) = setup();
        let packs = core.paths.game(&a.id).join("resourcepacks");
        fs::create_dir_all(&packs).unwrap();
        fs::write(packs.join("mine.zip"), b"pack").unwrap();

        core.set_folder_shared(&a.id, "resourcepacks", true)
            .unwrap();
        core.set_folder_shared(&b.id, "resourcepacks", true)
            .unwrap();

        assert_eq!(
            fs::read(core.paths.game(&b.id).join("resourcepacks/mine.zip")).unwrap(),
            b"pack"
        );
        fs::write(core.paths.game(&b.id).join("resourcepacks/new.zip"), b"new").unwrap();
        assert!(
            core.paths
                .game(&a.id)
                .join("resourcepacks/new.zip")
                .is_file()
        );
    }

    #[test]
    fn a_clash_keeps_both_files() {
        let (_dir, core, a, b) = setup();
        for (instance, body) in [(&a, "first"), (&b, "second")] {
            let packs = core.paths.game(&instance.id).join("resourcepacks");
            fs::create_dir_all(&packs).unwrap();
            fs::write(packs.join("same.zip"), body).unwrap();
        }
        core.set_folder_shared(&a.id, "resourcepacks", true)
            .unwrap();
        core.set_folder_shared(&b.id, "resourcepacks", true)
            .unwrap();

        let common = core.paths.shared("resourcepacks");
        assert_eq!(fs::read(common.join("same.zip")).unwrap(), b"first");
        assert_eq!(fs::read(common.join("B same.zip")).unwrap(), b"second");
    }

    #[test]
    fn stopping_sharing_leaves_a_private_copy() {
        let (_dir, core, a, _b) = setup();
        let packs = core.paths.game(&a.id).join("shaderpacks");
        fs::create_dir_all(&packs).unwrap();
        fs::write(packs.join("shader.zip"), b"s").unwrap();
        core.set_folder_shared(&a.id, "shaderpacks", true).unwrap();

        let instance = core.set_folder_shared(&a.id, "shaderpacks", false).unwrap();

        assert!(instance.shared.is_empty());
        assert!(!crate::waymarks::is_linked(&packs));
        assert_eq!(fs::read(packs.join("shader.zip")).unwrap(), b"s");
        assert!(
            core.paths
                .shared("shaderpacks")
                .join("shader.zip")
                .is_file()
        );
    }

    #[test]
    fn waymarks_ignore_shared_folders() {
        let (_dir, core, a, _b) = setup();
        core.set_folder_shared(&a.id, "resourcepacks", true)
            .unwrap();
        fs::write(core.paths.shared("resourcepacks").join("big.zip"), b"pack").unwrap();
        let first = core
            .trail(&a.id)
            .mark("first", WaymarkKind::Manual)
            .unwrap()
            .unwrap();
        assert_eq!(first.files, 0);
    }

    #[test]
    fn only_known_folders_can_be_shared() {
        let (_dir, core, a, _b) = setup();
        assert!(core.set_folder_shared(&a.id, "saves", true).is_err());
    }

    #[test]
    fn copies_options_and_keybinds() {
        let (_dir, core, a, b) = setup();
        fs::write(core.paths.game(&a.id).join("options.txt"), b"fov:90").unwrap();
        let mark = core.copy_settings(&a.id, &b.id).unwrap().unwrap();
        assert_eq!(mark.label, "Copied settings from A");
        assert_eq!(
            fs::read(core.paths.game(&b.id).join("options.txt")).unwrap(),
            b"fov:90"
        );
    }

    #[test]
    fn copying_from_an_instance_with_no_settings_says_so() {
        let (_dir, core, a, b) = setup();
        assert!(core.copy_settings(&a.id, &b.id).is_err());
    }
}

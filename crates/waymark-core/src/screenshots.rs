//! Screenshots from every instance, in one list.

use std::collections::HashSet;
use std::fs;

use serde::Serialize;

use crate::Core;
use crate::error::{Error, Result};
use crate::install::safe_relative;
use crate::trash::{TrashKind, Trashed};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    pub instance: String,
    pub instance_name: String,
    pub file: String,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: u64,
}

impl Core {
    /// Newest first. A shared folder is only listed once.
    pub fn screenshots(&self) -> Result<Vec<Screenshot>> {
        let mut shots = Vec::new();
        let mut seen = HashSet::new();
        for instance in self.instances()? {
            let folder = self.paths.game(&instance.id).join("screenshots");
            let real = fs::canonicalize(&folder).unwrap_or_else(|_| folder.clone());
            if !seen.insert(real) {
                continue;
            }
            let Ok(entries) = fs::read_dir(folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                let Ok(meta) = entry.metadata() else { continue };
                if !meta.is_file() || !file.to_ascii_lowercase().ends_with(".png") {
                    continue;
                }
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |at| at.as_secs());
                shots.push(Screenshot {
                    instance: instance.id.clone(),
                    instance_name: instance.name.clone(),
                    file,
                    size: meta.len(),
                    modified,
                });
            }
        }
        shots.sort_by_key(|shot| std::cmp::Reverse(shot.modified));
        Ok(shots)
    }

    pub fn delete_screenshot(&self, id: &str, file: &str) -> Result<Trashed> {
        if safe_relative(file)?.components().count() != 1 {
            return Err(Error::NotFound(file.to_string()));
        }
        let path = self.paths.game(id).join("screenshots").join(file);
        if !path.is_file() {
            return Err(Error::NotFound(file.to_string()));
        }
        self.send_to_trash(&path, TrashKind::Screenshot, file, Some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;

    #[test]
    fn lists_every_instance_and_a_deleted_shot_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let first = core
            .register_instance("One", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let second = core
            .register_instance("Two", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        for (instance, name) in [(&first, "a.png"), (&second, "b.png")] {
            let folder = core.paths.game(&instance.id).join("screenshots");
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join(name), b"png").unwrap();
            fs::write(folder.join("notes.txt"), b"text").unwrap();
        }
        assert_eq!(core.screenshots().unwrap().len(), 2);

        let item = core.delete_screenshot(&first.id, "a.png").unwrap();
        assert_eq!(core.screenshots().unwrap().len(), 1);
        core.restore_trashed(&item.id).unwrap();
        assert_eq!(core.screenshots().unwrap().len(), 2);

        assert!(
            core.delete_screenshot(&first.id, "../../instance.toml")
                .is_err()
        );
    }
}

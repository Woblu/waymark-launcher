//! Writing state files so that a crash, a power cut or a second writer can
//! never leave half of one behind. Every file the launcher keeps its own
//! state in goes through here.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Replaces `path` with `bytes` all at once: readers see the old file or the
/// new one, never a mix, even if the machine dies halfway through.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // A unique name each time, so two writers can't trample each other's
    // half-written copy. Waymarks already ignore anything ending in `.part`.
    let part = temp_beside(path);
    let written = File::create(&part).and_then(|mut file| {
        file.write_all(bytes)?;
        // Without this, a rename can reach the disk before the data does,
        // and a power cut leaves an empty file under the real name.
        file.sync_all()
    });
    if let Err(error) = written.and_then(|()| rename(&part, path)) {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

pub fn temp_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    path.with_file_name(format!("{name}.{}.part", uuid::Uuid::new_v4().simple()))
}

/// Antivirus and indexers briefly hold files open on Windows, which makes a
/// rename over them fail with "access denied". A short wait is enough.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied && attempt < 5 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(40 * attempt));
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_replaces_the_whole_file_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("state.toml");
        write(&path, b"first, and rather longer").unwrap();
        write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn two_writers_get_different_temporary_files() {
        let path = Path::new("state.json");
        assert_ne!(temp_beside(path), temp_beside(path));
    }
}

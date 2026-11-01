//! Desktop shortcuts that start an instance straight away.

use std::fs;
use std::path::{Path, PathBuf};

use directories::UserDirs;

use crate::Core;
use crate::error::{Error, Result};

/// The argument a shortcut passes to the launcher.
pub const LAUNCH_FLAG: &str = "--launch";

fn file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_control())
        .collect();
    let safe = safe.trim().trim_end_matches('.').to_string();
    if safe.is_empty() {
        "Minecraft".to_string()
    } else {
        safe
    }
}

#[cfg(windows)]
fn write_shortcut(desktop: &Path, name: &str, exe: &Path, id: &str) -> Result<PathBuf> {
    let path = desktop.join(format!("{name}.lnk"));
    let problem = |error: mslnk::MSLinkError| Error::Other(error.to_string());
    let mut link = mslnk::ShellLink::new(exe).map_err(problem)?;
    link.set_arguments(Some(format!("{LAUNCH_FLAG} {id}")));
    link.create_lnk(&path).map_err(problem)?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn write_shortcut(desktop: &Path, name: &str, exe: &Path, id: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = desktop.join(format!("{name}.command"));
    fs::write(
        &path,
        format!("#!/bin/sh\nexec \"{}\" {LAUNCH_FLAG} {id}\n", exe.display()),
    )?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn write_shortcut(desktop: &Path, name: &str, exe: &Path, id: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = desktop.join(format!("{name}.desktop"));
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec=\"{}\" {LAUNCH_FLAG} {id}\nTerminal=false\n",
        exe.display()
    );
    fs::write(&path, entry)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

impl Core {
    /// Puts a shortcut on the desktop that launches this instance.
    pub fn create_shortcut(&self, id: &str, exe: &Path) -> Result<PathBuf> {
        let instance = self.instance(id)?;
        let dirs = UserDirs::new().ok_or_else(|| Error::Other("no home folder".into()))?;
        let desktop = dirs
            .desktop_dir()
            .ok_or_else(|| Error::Other("this computer has no desktop folder".into()))?;
        fs::create_dir_all(desktop)?;
        write_shortcut(desktop, &file_name(&instance.name), exe, &instance.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_for_the_desktop() {
        assert_eq!(file_name("All The Mods: 10?"), "All The Mods 10");
        assert_eq!(file_name("..."), "Minecraft");
        assert_eq!(file_name("a/b\\c"), "abc");
    }

    #[test]
    fn writes_a_shortcut_file() {
        let dir = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let path = write_shortcut(dir.path(), "Pack", &exe, "pack").unwrap();
        assert!(path.is_file());
    }
}

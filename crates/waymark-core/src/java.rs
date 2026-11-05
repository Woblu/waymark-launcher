//! Java runtimes, from the same source Mojang's own launcher uses, so players
//! never have to think about which Java a version wants.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::install::safe_relative;
use crate::net::{Expected, Fetch};

const RUNTIME_INDEX_URL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
const INSTALLED_MARKER: &str = ".waymark-installed";

type RuntimeIndex = BTreeMap<String, BTreeMap<String, Vec<RuntimeListing>>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RuntimeListing {
    manifest: ManifestRef,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ManifestRef {
    sha1: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: BTreeMap<String, RuntimeEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RuntimeEntry {
    Directory,
    File {
        #[serde(default)]
        executable: bool,
        downloads: RuntimeDownloads,
    },
    Link {
        target: String,
    },
}

#[derive(Debug, Deserialize)]
struct RuntimeDownloads {
    raw: RawDownload,
}

#[derive(Debug, Deserialize)]
struct RawDownload {
    sha1: String,
    size: u64,
    url: String,
}

/// Mojang's name for this platform in the runtime index.
fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        ("windows", _) => "windows-x64",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

fn java_binary(home: &Path) -> PathBuf {
    match std::env::consts::OS {
        "windows" => home.join("bin").join("javaw.exe"),
        "macos" => home.join("jre.bundle/Contents/Home/bin/java"),
        _ => home.join("bin").join("java"),
    }
}

impl Core {
    /// Returns the Java binary for a runtime component, installing it if needed.
    pub async fn ensure_java(&self, component: &str, task: &Task) -> Result<PathBuf> {
        let home = self.paths.java().join(component);
        let binary = java_binary(&home);
        if home.join(INSTALLED_MARKER).is_file() && binary.is_file() {
            return Ok(binary);
        }

        task.progress(0, 1, &format!("Finding Java ({component})"));
        let cache = self.paths.cache().join("java-runtimes.json");
        let index: RuntimeIndex = self
            .net
            .cached_json(RUNTIME_INDEX_URL, &cache, Duration::from_secs(6 * 3600))
            .await?;
        let listing = index
            .get(platform())
            .and_then(|components| components.get(component))
            .and_then(|listings| listings.first())
            .ok_or_else(|| {
                Error::Other(format!(
                    "Mojang has no {component} runtime for {}",
                    platform()
                ))
            })?;

        let bytes = self
            .net
            .client()
            .get(&listing.manifest.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let manifest = read_manifest(&bytes, &listing.manifest.sha1)?;
        let plan = plan(&home, &manifest)?;
        for folder in &plan.folders {
            tokio::fs::create_dir_all(folder).await?;
        }
        self.net
            .fetch_all(plan.fetches, |done, total| {
                task.progress(done, total, "Downloading Java")
            })
            .await?;
        finish_runtime(&plan.executables, &plan.links)?;
        tokio::fs::write(home.join(INSTALLED_MARKER), b"").await?;
        Ok(binary)
    }
}

/// A runtime's manifest, checked against the hash Mojang's index gives for it,
/// since every file's own hash comes from here.
fn read_manifest(bytes: &[u8], sha1: &str) -> Result<RuntimeManifest> {
    if !hex::encode(Sha1::digest(bytes)).eq_ignore_ascii_case(sha1) {
        return Err(Error::HashMismatch {
            url: "the Java runtime manifest".into(),
        });
    }
    Ok(serde_json::from_slice(bytes)?)
}

/// What installing a runtime takes: folders, verified downloads, and the
/// executables and links to set up afterwards. Every path has to stay inside
/// the runtime's folder.
#[derive(Debug, Default)]
struct Plan {
    folders: Vec<PathBuf>,
    fetches: Vec<Fetch>,
    executables: Vec<PathBuf>,
    links: Vec<(PathBuf, String)>,
}

fn plan(home: &Path, manifest: &RuntimeManifest) -> Result<Plan> {
    let mut plan = Plan::default();
    for (relative, entry) in &manifest.files {
        let path = home.join(safe_relative(relative)?);
        match entry {
            RuntimeEntry::Directory => plan.folders.push(path),
            RuntimeEntry::File {
                executable,
                downloads,
            } => {
                plan.fetches.push(Fetch {
                    url: downloads.raw.url.clone(),
                    dest: path.clone(),
                    hash: Expected::Sha1(downloads.raw.sha1.clone()),
                    size: Some(downloads.raw.size),
                });
                if *executable {
                    plan.executables.push(path);
                }
            }
            RuntimeEntry::Link { target } => plan.links.push((path, target.clone())),
        }
    }
    Ok(plan)
}

#[cfg(unix)]
fn finish_runtime(executables: &[PathBuf], links: &[(PathBuf, String)]) -> Result<()> {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for path in executables {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    for (path, target) in links {
        if !path.exists() {
            symlink(target, path)?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn finish_runtime(_: &[PathBuf], _: &[(PathBuf, String)]) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real manifest, trimmed to one of each kind of entry.
    const MANIFEST: &str = r#"{"files":{
        "bin":{"type":"directory"},
        "bin/java":{"type":"file","executable":true,"downloads":{"raw":{"sha1":"aa","size":12,"url":"https://piston-data.mojang.com/v1/objects/aa/java"}}},
        "lib/libjli.so":{"type":"file","executable":false,"downloads":{"raw":{"sha1":"bb","size":34,"url":"https://piston-data.mojang.com/v1/objects/bb/libjli.so"}}},
        "legal/java.base/LICENSE":{"type":"link","target":"../java.desktop/LICENSE"}
    }}"#;

    fn sha1_of(text: &str) -> String {
        hex::encode(Sha1::digest(text.as_bytes()))
    }

    #[test]
    fn a_manifest_is_only_read_when_its_hash_matches() {
        assert!(read_manifest(MANIFEST.as_bytes(), &sha1_of(MANIFEST)).is_ok());
        assert!(read_manifest(MANIFEST.as_bytes(), &sha1_of(MANIFEST).to_uppercase()).is_ok());
        let tampered = MANIFEST.replace("bb/libjli", "evil/libjli");
        assert!(matches!(
            read_manifest(tampered.as_bytes(), &sha1_of(MANIFEST)),
            Err(Error::HashMismatch { .. })
        ));
    }

    #[test]
    fn a_runtime_is_planned_inside_its_own_folder() {
        let manifest: RuntimeManifest = serde_json::from_str(MANIFEST).unwrap();
        let home = Path::new("java").join("java-runtime-delta");
        let plan = plan(&home, &manifest).unwrap();
        assert_eq!(plan.folders, [home.join("bin")]);
        assert_eq!(plan.fetches.len(), 2);
        assert!(plan.fetches.iter().all(|f| f.dest.starts_with(&home)));
        assert!(plan.fetches.iter().all(|f| matches!(f.hash, Expected::Sha1(_))));
        assert_eq!(plan.executables, [home.join("bin/java")]);
        assert_eq!(plan.links.len(), 1);
    }

    #[test]
    fn a_manifest_path_that_leaves_the_folder_is_refused() {
        let evil = MANIFEST.replace("\"bin/java\"", "\"../../startup/java\"");
        let manifest: RuntimeManifest = serde_json::from_str(&evil).unwrap();
        assert!(plan(Path::new("java"), &manifest).is_err());
    }

    #[test]
    fn every_platform_has_a_name_mojang_uses() {
        let known = [
            "windows-x64",
            "windows-x86",
            "windows-arm64",
            "mac-os",
            "mac-os-arm64",
            "linux",
            "linux-i386",
        ];
        assert!(known.contains(&platform()));
    }

    #[tokio::test]
    async fn an_installed_runtime_is_used_without_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let home = core.paths.java().join("java-runtime-delta");
        let binary = java_binary(&home);
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, b"").unwrap();
        std::fs::write(home.join(INSTALLED_MARKER), b"").unwrap();
        let task = Task::start(&core.events, "test");
        assert_eq!(core.ensure_java("java-runtime-delta", &task).await.unwrap(), binary);
    }

    #[cfg(unix)]
    #[test]
    fn executables_and_links_are_set_up_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("java");
        std::fs::write(&java, b"").unwrap();
        std::fs::write(dir.path().join("target"), b"").unwrap();
        let link = dir.path().join("link");
        finish_runtime(&[java.clone()], &[(link.clone(), "target".into())]).unwrap();
        assert_eq!(std::fs::metadata(&java).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }
}

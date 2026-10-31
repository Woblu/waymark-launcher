//! Java runtimes, from the same source Mojang's own launcher uses, so players
//! never have to think about which Java a version wants.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::net::{Expected, Fetch};

const RUNTIME_INDEX_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
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
        let index: RuntimeIndex = self.net.cached_json(RUNTIME_INDEX_URL, &cache, Duration::from_secs(6 * 3600)).await?;
        let listing = index
            .get(platform())
            .and_then(|components| components.get(component))
            .and_then(|listings| listings.first())
            .ok_or_else(|| Error::Other(format!("Mojang has no {component} runtime for {}", platform())))?;

        let manifest: RuntimeManifest = self.net.json(&listing.manifest.url).await?;
        let mut fetches = Vec::new();
        let mut executables = Vec::new();
        let mut links = Vec::new();
        for (relative, entry) in &manifest.files {
            let path = home.join(relative);
            match entry {
                RuntimeEntry::Directory => tokio::fs::create_dir_all(&path).await?,
                RuntimeEntry::File { executable, downloads } => {
                    fetches.push(Fetch {
                        url: downloads.raw.url.clone(),
                        dest: path.clone(),
                        hash: Expected::Sha1(downloads.raw.sha1.clone()),
                        size: Some(downloads.raw.size),
                    });
                    if *executable {
                        executables.push(path);
                    }
                }
                RuntimeEntry::Link { target } => links.push((path, target.clone())),
            }
        }

        self.net
            .fetch_all(fetches, |done, total| task.progress(done, total, "Downloading Java"))
            .await?;
        finish_runtime(&executables, &links)?;
        tokio::fs::write(home.join(INSTALLED_MARKER), b"").await?;
        Ok(binary)
    }
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

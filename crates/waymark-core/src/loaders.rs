//! Fabric, Quilt, Forge and NeoForge. The first two publish ready-made version
//! profiles. The other two only ship installers, which we run headlessly
//! against the data directory because it has the same layout as `.minecraft`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::lock::LoaderKind;
use crate::mojang::VersionMeta;
use crate::net::{Expected, Fetch};

const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions/loader";
const QUILT_META: &str = "https://meta.quiltmc.org/v3/versions/loader";
const NEOFORGE_VERSIONS: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const FORGE_VERSIONS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Serialize, Deserialize)]
struct MetaEntry {
    loader: MetaLoader,
}

#[derive(Serialize, Deserialize)]
struct MetaLoader {
    version: String,
    #[serde(default = "assume_stable")]
    stable: bool,
}

fn assume_stable() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
struct NeoForgeVersions {
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct InstallerVersion {
    id: String,
}

/// NeoForge numbers follow Minecraft: 1.21.4 is `21.4.x`, and 1.21 is `21.0.x`.
/// Calendar-style versions like 26.1 keep their own numbers.
fn neoforge_prefix(minecraft: &str) -> Option<String> {
    let parts: Vec<&str> = minecraft.split('.').collect();
    match parts.as_slice() {
        ["1", minor] => Some(format!("{minor}.0.")),
        ["1", minor, patch] => Some(format!("{minor}.{patch}.")),
        [year, drop] => Some(format!("{year}.{drop}.")),
        [year, drop, hotfix] => Some(format!("{year}.{drop}.{hotfix}.")),
        _ => None,
    }
}

impl Core {
    pub async fn loader_versions(&self, kind: LoaderKind, minecraft: &str) -> Result<Vec<LoaderVersion>> {
        let cache = self.paths.cache().join(format!("loaders-{}-{minecraft}.json", kind.slug()));
        let hour = Duration::from_secs(3600);
        match kind {
            LoaderKind::Vanilla => Ok(Vec::new()),
            LoaderKind::Fabric | LoaderKind::Quilt => {
                let base = if kind == LoaderKind::Fabric { FABRIC_META } else { QUILT_META };
                let url = format!("{base}/{minecraft}");
                let entries: Vec<MetaEntry> = self.net.cached_json(&url, &cache, hour).await?;
                // Quilt doesn't flag stability, but its prereleases have a suffix.
                Ok(entries
                    .into_iter()
                    .map(|e| LoaderVersion { stable: e.loader.stable && !e.loader.version.contains('-'), version: e.loader.version })
                    .collect())
            }
            LoaderKind::NeoForge => {
                let prefix = neoforge_prefix(minecraft).ok_or_else(|| Error::NotFound(format!("NeoForge for {minecraft}")))?;
                let listing: NeoForgeVersions = self.net.cached_json(NEOFORGE_VERSIONS, &cache, hour).await?;
                Ok(listing
                    .versions
                    .into_iter()
                    .rev()
                    .filter(|v| v.starts_with(&prefix))
                    .map(|version| LoaderVersion { stable: !version.contains("beta"), version })
                    .collect())
            }
            LoaderKind::Forge => {
                let listing: BTreeMap<String, Vec<String>> = self.net.cached_json(FORGE_VERSIONS, &cache, hour).await?;
                Ok(listing
                    .get(minecraft)
                    .into_iter()
                    .flatten()
                    .rev()
                    .map(|full| LoaderVersion {
                        version: full.strip_prefix(&format!("{minecraft}-")).unwrap_or(full).to_string(),
                        stable: true,
                    })
                    .collect())
            }
        }
    }

    /// Installs a loader and returns the version id to launch.
    pub async fn install_loader(&self, kind: LoaderKind, minecraft: &str, version: &str, task: &Task) -> Result<String> {
        match kind {
            LoaderKind::Vanilla => Ok(minecraft.to_string()),
            LoaderKind::Fabric => self.install_profile(FABRIC_META, minecraft, version).await,
            LoaderKind::Quilt => self.install_profile(QUILT_META, minecraft, version).await,
            LoaderKind::NeoForge | LoaderKind::Forge => {
                let url = installer_url(kind, minecraft, version).expect("these loaders ship installers");
                self.run_installer(&url, minecraft, task).await
            }
        }
    }

    async fn install_profile(&self, base: &str, minecraft: &str, version: &str) -> Result<String> {
        let url = format!("{base}/{minecraft}/{version}/profile/json");
        let meta: VersionMeta = self.net.json(&url).await?;
        let path = self.paths.version_json(&meta.id);
        crate::net::write_atomic(&path, &serde_json::to_vec(&meta)?).await?;
        Ok(meta.id)
    }

    async fn run_installer(&self, url: &str, minecraft: &str, task: &Task) -> Result<String> {
        let name = url.rsplit('/').next().unwrap_or("installer.jar");
        let installer = self.paths.cache().join("installers").join(name);
        task.progress(0, 1, "Downloading loader");
        self.net
            .fetch(&Fetch { url: url.to_string(), dest: installer.clone(), hash: Expected::Nothing, size: None })
            .await?;
        let id = installed_version_id(&installer)?;
        if self.paths.version_json(&id).is_file() {
            return Ok(id);
        }

        // The installer patches the vanilla client, so that has to exist first.
        let vanilla = self.prepare_version(minecraft, task).await?;
        let profiles = self.paths.root().join("launcher_profiles.json");
        if !profiles.is_file() {
            tokio::fs::write(&profiles, br#"{"profiles":{}}"#).await?;
        }

        task.progress(0, 1, "Running loader installer");
        let mut command = Command::new(&vanilla.java);
        command
            .arg("-jar")
            .arg(&installer)
            .arg("--installClient")
            .arg(self.paths.root())
            .current_dir(self.paths.cache())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let output = command.output().await?;
        if !output.status.success() || !self.paths.version_json(&id).is_file() {
            let detail = String::from_utf8_lossy(&output.stderr);
            let last = detail.lines().last().unwrap_or("no error output");
            return Err(Error::Other(format!("the loader installer failed: {last}")));
        }
        Ok(id)
    }
}

/// Where Forge and NeoForge publish their installers.
pub(crate) fn installer_url(kind: LoaderKind, minecraft: &str, version: &str) -> Option<String> {
    match kind {
        LoaderKind::NeoForge => Some(format!("{NEOFORGE_MAVEN}/{version}/neoforge-{version}-installer.jar")),
        LoaderKind::Forge => {
            let full = format!("{minecraft}-{version}");
            Some(format!("{FORGE_MAVEN}/{full}/forge-{full}-installer.jar"))
        }
        _ => None,
    }
}

/// Reads the id the installer will create without having to run it.
fn installed_version_id(installer: &Path) -> Result<String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(installer)?)?;
    let mut text = String::new();
    archive.by_name("version.json")?.read_to_string(&mut text)?;
    Ok(serde_json::from_str::<InstallerVersion>(&text)?.id)
}

#[cfg(windows)]
pub(crate) fn hide_console(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn hide_console(_: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_minecraft_versions_to_neoforge_numbers() {
        assert_eq!(neoforge_prefix("1.21.4").unwrap(), "21.4.");
        assert_eq!(neoforge_prefix("1.21").unwrap(), "21.0.");
        assert_eq!(neoforge_prefix("26.1").unwrap(), "26.1.");
        assert_eq!(neoforge_prefix("26.1.2").unwrap(), "26.1.2.");
    }
}

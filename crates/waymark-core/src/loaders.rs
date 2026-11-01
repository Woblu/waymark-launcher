//! Mod loaders. Fabric, Quilt and the projects that carry Fabric back to older
//! versions all publish ready-made version profiles. Forge and NeoForge only
//! ship installers, which we run headlessly against the data directory because
//! it has the same layout as `.minecraft`.

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
use crate::install::safe_relative;
use crate::instance::Instance;
use crate::lock::LoaderKind;
use crate::mojang::{VersionMeta, maven_path};
use crate::net::{Expected, Fetch};

const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions/loader";
const QUILT_META: &str = "https://meta.quiltmc.org/v3/versions/loader";
const LEGACY_FABRIC_META: &str = "https://meta.legacyfabric.net/v2/versions/loader";
const BABRIC_META: &str = "https://meta.babric.glass-launcher.net/v2/versions/loader";
const ORNITHE_META: &str = "https://meta.ornithemc.net/v3/versions/fabric-loader";
const NEOFORGE_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const FORGE_VERSIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
/// Forge's own fix for its old builds crashing on Java 8.
const JAVA_FIX: &str = "https://maven.minecraftforge.net/net/minecraftforge/lex/legacyjavafixer/1.0/legacyjavafixer-1.0.jar";
const JAVA_FIX_SHA1: &str = "a11b502bef19f49bfc199722b94da5f3d7b470a8";

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

/// Where a loader publishes its profiles, if it does.
fn profile_host(kind: LoaderKind) -> Option<&'static str> {
    match kind {
        LoaderKind::Fabric => Some(FABRIC_META),
        LoaderKind::Quilt => Some(QUILT_META),
        LoaderKind::LegacyFabric => Some(LEGACY_FABRIC_META),
        LoaderKind::Babric => Some(BABRIC_META),
        LoaderKind::Ornithe => Some(ORNITHE_META),
        _ => None,
    }
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
    pub async fn loader_versions(
        &self,
        kind: LoaderKind,
        minecraft: &str,
    ) -> Result<Vec<LoaderVersion>> {
        let cache = self
            .paths
            .cache()
            .join(format!("loaders-{}-{minecraft}.json", kind.slug()));
        let hour = Duration::from_secs(3600);
        match kind {
            LoaderKind::Vanilla => Ok(Vec::new()),
            LoaderKind::Fabric
            | LoaderKind::Quilt
            | LoaderKind::LegacyFabric
            | LoaderKind::Babric
            | LoaderKind::Ornithe => {
                let base = profile_host(kind).expect("these loaders publish profiles");
                let url = format!("{base}/{minecraft}");
                let entries: Vec<MetaEntry> = self.net.cached_json(&url, &cache, hour).await?;
                // Quilt doesn't flag stability, but its prereleases have a suffix.
                Ok(entries
                    .into_iter()
                    .map(|e| LoaderVersion {
                        stable: e.loader.stable && !e.loader.version.contains('-'),
                        version: e.loader.version,
                    })
                    .collect())
            }
            LoaderKind::NeoForge => {
                let prefix = neoforge_prefix(minecraft)
                    .ok_or_else(|| Error::NotFound(format!("NeoForge for {minecraft}")))?;
                let listing: NeoForgeVersions = self
                    .net
                    .cached_json(NEOFORGE_VERSIONS, &cache, hour)
                    .await?;
                Ok(listing
                    .versions
                    .into_iter()
                    .rev()
                    .filter(|v| v.starts_with(&prefix))
                    .map(|version| LoaderVersion {
                        stable: !version.contains("beta"),
                        version,
                    })
                    .collect())
            }
            LoaderKind::Forge => {
                let listing: BTreeMap<String, Vec<String>> =
                    self.net.cached_json(FORGE_VERSIONS, &cache, hour).await?;
                Ok(listing
                    .get(minecraft)
                    .into_iter()
                    .flatten()
                    .rev()
                    .map(|full| LoaderVersion {
                        version: full
                            .strip_prefix(&format!("{minecraft}-"))
                            .unwrap_or(full)
                            .to_string(),
                        stable: true,
                    })
                    .collect())
            }
        }
    }

    /// Installs a loader and returns the version id to launch.
    pub async fn install_loader(
        &self,
        kind: LoaderKind,
        minecraft: &str,
        version: &str,
        task: &Task,
    ) -> Result<String> {
        match kind {
            LoaderKind::Vanilla => Ok(minecraft.to_string()),
            LoaderKind::Fabric
            | LoaderKind::Quilt
            | LoaderKind::LegacyFabric
            | LoaderKind::Babric
            | LoaderKind::Ornithe => {
                self.install_profile(
                    profile_host(kind).expect("these loaders publish profiles"),
                    minecraft,
                    version,
                )
                .await
            }
            LoaderKind::NeoForge | LoaderKind::Forge => {
                let url = self
                    .loader_installer(kind, minecraft, version)
                    .await
                    .expect("these loaders ship installers");
                self.run_installer(&url, minecraft, task).await
            }
        }
    }

    /// The installer's address. Some old Forge builds end in their Minecraft
    /// version, which packs leave off, so those are looked up.
    pub(crate) async fn loader_installer(
        &self,
        kind: LoaderKind,
        minecraft: &str,
        version: &str,
    ) -> Option<String> {
        if kind != LoaderKind::Forge {
            return installer_url(kind, minecraft, version);
        }
        let known = self
            .loader_versions(kind, minecraft)
            .await
            .unwrap_or_default();
        let build = full_forge_build(known.iter().map(|known| known.version.as_str()), version);
        installer_url(kind, minecraft, build.unwrap_or(version))
    }

    /// Puts Forge's Java 8 fix with an old instance's mods. Forge from before
    /// Java 8 crashes on it, and Mojang's oldest Java for these versions is 8.
    pub(crate) async fn add_java_fix(&self, instance: &Instance) -> Result<()> {
        let version = instance.loader.version.as_deref().unwrap_or_default();
        if instance.loader.kind != LoaderKind::Forge
            || !needs_java_fix(&instance.minecraft, version)
        {
            return Ok(());
        }
        let file = JAVA_FIX.rsplit('/').next().unwrap_or("legacyjavafixer.jar");
        self.net
            .fetch(&Fetch {
                url: JAVA_FIX.to_string(),
                dest: self.paths.game(&instance.id).join("mods").join(file),
                hash: Expected::Sha1(JAVA_FIX_SHA1.to_string()),
                size: None,
            })
            .await
    }

    async fn install_profile(&self, base: &str, minecraft: &str, version: &str) -> Result<String> {
        let url = format!("{base}/{minecraft}/{version}/profile/json");
        let mut meta: VersionMeta = self.net.json(&url).await?;
        // Ornithe names its base version in a way only its own tools recognise.
        meta.inherits_from = meta
            .inherits_from
            .map(|parent| parent.trim_end_matches("-vanilla").to_string());
        let path = self.paths.version_json(&meta.id);
        crate::net::write_atomic(&path, &serde_json::to_vec(&meta)?).await?;
        Ok(meta.id)
    }

    /// Forge before 1.12 has no headless installer, but its installer holds
    /// everything needed: a version profile and the mod loader's own jar.
    async fn install_legacy_forge(
        &self,
        installer: &Path,
        mut legacy: LegacyProfile,
        minecraft: &str,
    ) -> Result<String> {
        // The oldest of these relied on the installer copying the game in.
        if let Some(profile) = legacy.version_info.as_object_mut() {
            profile.entry("inheritsFrom").or_insert(minecraft.into());
        }
        let id = legacy.install.target;
        let library = maven_path(&legacy.install.path).ok_or_else(|| {
            Error::Other(format!(
                "Forge named a library oddly: {}",
                legacy.install.path
            ))
        })?;
        safe_relative(&legacy.install.file_path)?;
        let mut archive = zip::ZipArchive::new(std::fs::File::open(installer)?)?;
        let mut jar = Vec::new();
        archive
            .by_name(&legacy.install.file_path)?
            .read_to_end(&mut jar)?;
        crate::net::write_atomic(&self.paths.libraries().join(safe_relative(&library)?), &jar)
            .await?;
        crate::net::write_atomic(
            &self.paths.version_json(&id),
            &serde_json::to_vec(&legacy.version_info)?,
        )
        .await?;
        Ok(id)
    }

    async fn run_installer(&self, url: &str, minecraft: &str, task: &Task) -> Result<String> {
        let name = url.rsplit('/').next().unwrap_or("installer.jar");
        let installer = self.paths.cache().join("installers").join(name);
        task.progress(0, 1, "Downloading loader");
        self.net
            .fetch(&Fetch {
                url: url.to_string(),
                dest: installer.clone(),
                hash: Expected::Nothing,
                size: None,
            })
            .await?;
        if let Some(legacy) = legacy_profile(&installer)? {
            return self
                .install_legacy_forge(&installer, legacy, minecraft)
                .await;
        }
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

/// The listed build a pack's shorter Forge version stands for, if it's different.
fn full_forge_build<'a>(listed: impl Iterator<Item = &'a str>, version: &str) -> Option<&'a str> {
    let suffixed = format!("{version}-");
    listed
        .into_iter()
        .find(|build| build.starts_with(&suffixed))
}

/// Whether this Forge is from before its Java 8 fix.
fn needs_java_fix(minecraft: &str, forge: &str) -> bool {
    match minecraft {
        "1.7.10" => ["10.13.0.", "10.13.1.", "10.13.2."]
            .iter()
            .any(|old| forge.starts_with(old)),
        _ => minecraft.starts_with("1.6.") || minecraft.starts_with("1.7."),
    }
}

/// Where Forge and NeoForge publish their installers.
fn installer_url(kind: LoaderKind, minecraft: &str, version: &str) -> Option<String> {
    match kind {
        LoaderKind::NeoForge => Some(format!(
            "{NEOFORGE_MAVEN}/{version}/neoforge-{version}-installer.jar"
        )),
        LoaderKind::Forge => {
            let full = format!("{minecraft}-{version}");
            Some(format!("{FORGE_MAVEN}/{full}/forge-{full}-installer.jar"))
        }
        _ => None,
    }
}

#[derive(Deserialize)]
struct LegacyProfile {
    install: LegacyInstall,
    #[serde(rename = "versionInfo")]
    version_info: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyInstall {
    target: String,
    path: String,
    file_path: String,
}

/// The old installer format, if that's what this is.
fn legacy_profile(installer: &Path) -> Result<Option<LegacyProfile>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(installer)?)?;
    let Ok(mut entry) = archive.by_name("install_profile.json") else {
        return Ok(None);
    };
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Ok(serde_json::from_str(&text).ok())
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
    fn tells_old_forge_installers_from_new_ones() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, profile: &str| {
            let path = dir.path().join(name);
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.start_file(
                "install_profile.json",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(profile.as_bytes()).unwrap();
            zip.finish().unwrap();
            path
        };
        let old = write(
            "old.jar",
            r#"{"install":{"target":"1.7.10-Forge","path":"net.minecraftforge:forge:1","filePath":"u.jar"},"versionInfo":{"id":"1.7.10-Forge"}}"#,
        );
        assert_eq!(
            legacy_profile(&old).unwrap().unwrap().install.target,
            "1.7.10-Forge"
        );
        let new = write("new.jar", r#"{"spec":1,"profile":"forge","version":"x"}"#);
        assert!(legacy_profile(&new).unwrap().is_none());
    }

    #[test]
    fn finds_the_forge_build_a_pack_means() {
        let listed = ["10.13.4.1614-1.7.10", "10.13.4.1448-1.7.10", "10.13.0.1160"];
        let find = |version| full_forge_build(listed.iter().copied(), version);
        assert_eq!(find("10.13.4.1448"), Some("10.13.4.1448-1.7.10"));
        assert_eq!(find("10.13.4.1448-1.7.10"), None);
        assert_eq!(find("10.13.0.1160"), None);
    }

    #[test]
    fn only_forge_from_before_java_8_gets_the_fix() {
        assert!(needs_java_fix("1.6.4", "9.11.1.1345") && needs_java_fix("1.7.2", "10.12.2.1161"));
        assert!(needs_java_fix("1.7.10", "10.13.2.1230"));
        assert!(
            !needs_java_fix("1.7.10", "10.13.4.1614") && !needs_java_fix("1.8.9", "11.15.1.2318")
        );
    }

    #[test]
    fn maps_minecraft_versions_to_neoforge_numbers() {
        assert_eq!(neoforge_prefix("1.21.4").unwrap(), "21.4.");
        assert_eq!(neoforge_prefix("1.21").unwrap(), "21.0.");
        assert_eq!(neoforge_prefix("26.1").unwrap(), "26.1.");
        assert_eq!(neoforge_prefix("26.1.2").unwrap(), "26.1.2.");
    }
}

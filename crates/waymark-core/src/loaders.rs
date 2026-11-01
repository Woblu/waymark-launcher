//! Mod loaders. Fabric, Quilt and the projects that carry Fabric back to older
//! versions all publish ready-made version profiles. Forge, NeoForge and
//! Cleanroom ship installers, which we run headlessly against the data directory
//! because it has the same layout as `.minecraft`. LiteLoader, Rift and Forge
//! from before Minecraft 1.6 have neither, so their profiles are written here.

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
use crate::mojang::{
    Argument, Arguments, Library, LibraryDownloads, LibraryFile, VersionMeta, maven_path,
};
use crate::net::{Expected, Fetch};
use crate::packs::sha1_of;

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
const FORGE_ROOT: &str = "https://maven.minecraftforge.net/";
const FORGE_FILES: &str = "https://files.minecraftforge.net/net/minecraftforge/forge";
/// Forge's own fix for its old builds crashing on Java 8.
const JAVA_FIX: &str = "https://maven.minecraftforge.net/net/minecraftforge/lex/legacyjavafixer/1.0/legacyjavafixer-1.0.jar";
const JAVA_FIX_SHA1: &str = "a11b502bef19f49bfc199722b94da5f3d7b470a8";
/// FML used to fetch these itself from an address that no longer exists.
const FML_LIBRARIES: &str = "https://files.prismlauncher.org/fmllibs";
/// Without the first, old Forge looks for its folder in the player's real `.minecraft`.
const OLD_FORGE_FLAGS: [&str; 3] = [
    "-Dminecraft.applet.TargetDirectory=${game_directory}",
    "-Dfml.ignoreInvalidMinecraftCertificates=true",
    "-Dfml.ignorePatchDiscrepancies=true",
];

const FML_1_3: [(&str, &str); 3] = [
    ("argo-2.25.jar", "bb672829fde76cb163004752b86b0484bd0a7f4b"),
    (
        "guava-12.0.1.jar",
        "b8e78b9af7bf45900e14c6f958486b6ca682195f",
    ),
    (
        "asm-all-4.0.jar",
        "98308890597acb64047f7e896638e0d98753ae82",
    ),
];
const FML_1_4: [(&str, &str); 4] = [
    FML_1_3[0],
    FML_1_3[1],
    FML_1_3[2],
    (
        "bcprov-jdk15on-147.jar",
        "b6f5d9926b0afbde9f4dbe3db88c5247be7794bb",
    ),
];
const FML_1_5: [(&str, &str); 5] = [
    (
        "argo-small-3.2.jar",
        "58912ea2858d168c50781f956fa5b59f0f7c6b51",
    ),
    (
        "guava-14.0-rc3.jar",
        "931ae21fa8014c3ce686aaa621eae565fefb1a6a",
    ),
    (
        "asm-all-4.1.jar",
        "054986e962b88d8660ae4566475658469595ef58",
    ),
    (
        "bcprov-jdk15on-148.jar",
        "960dea7c9181ba0b17e8bab0c06a43f0a5f04e65",
    ),
    (
        "scala-library.jar",
        "458d046151ad179c85429ed7420ffb1eaf6ddf85",
    ),
];
/// Each 1.5 version also has its own table of names.
const FML_DEOBFUSCATION: [(&str, &str, &str); 3] = [
    (
        "1.5",
        "deobfuscation_data_1.5.zip",
        "5f7c142d53776f16304c0bbe10542014abad6af8",
    ),
    (
        "1.5.1",
        "deobfuscation_data_1.5.1.zip",
        "22e221a0d89516c1f721d6cab056a7e37471d0a6",
    ),
    (
        "1.5.2",
        "deobfuscation_data_1.5.2.zip",
        "446e55cd986582c70fcf12cb27bc00114c5adfd9",
    ),
];

const LITELOADER_VERSIONS: &str = "https://dl.liteloader.com/versions/versions.json";
/// The `repo` addresses in the listing serve different files from the ones it
/// names, so everything comes from here, where the checksums match.
const LITELOADER_REPO: &str = "https://dl.liteloader.com/versions/";
const LITELOADER_SNAPSHOTS: &str =
    "https://repo.mumfrey.com/content/repositories/snapshots/com/mumfrey/liteloader";
/// Holds the one library LiteLoader needs that Mojang doesn't have.
const LITELOADER_LIBRARIES: &str = "https://repo.liteloader.com/";
const LAUNCHWRAPPER: &str = "net.minecraft.launchwrapper.Launch";
/// The first that can chain tweakers, from before it needed log4j.
const CHAINING_LAUNCHWRAPPER: &str = "net.minecraft:launchwrapper:1.8";
const VANILLA_TWEAKER: &str = "net.minecraft.launchwrapper.VanillaTweaker";

const RIFT_RELEASES: &str = "https://github.com/DimensionalDevelopment/Rift/releases/download";
const RIFT_TWEAKER: &str = "org.dimdev.riftloader.launch.RiftLoaderClientTweaker";
const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2/";
/// Rift's own fork of Mixin was only ever on its Maven, which is gone. This is
/// the Internet Archive's copy from while that site was up, and the hash is the
/// one the archive recorded then, so nothing else can pass for it.
const RIFT_MIXIN: &str = "https://web.archive.org/web/20180813161025id_/https://www.dimdev.org/maven/org/dimdev/mixin/0.7.11-SNAPSHOT/mixin-0.7.11-SNAPSHOT.jar";
const RIFT_MIXIN_SHA1: &str = "81814a44fbd03bff4dd3efd08b2791a47ba7191b";
const RIFT_MIXIN_PATH: &str = "org/dimdev/mixin/0.7.11-SNAPSHOT/mixin-0.7.11-SNAPSHOT.jar";

struct RiftBuild {
    minecraft: &'static str,
    version: &'static str,
    sha1: &'static str,
}

/// Rift ended in 2019 and these are its last builds. None was made for 1.13.1.
const RIFT_BUILDS: [RiftBuild; 2] = [
    RiftBuild {
        minecraft: "1.13.2",
        version: "1.0.4-106",
        sha1: "2d523f42012265949561409803725bea904f2b34",
    },
    RiftBuild {
        minecraft: "1.13",
        version: "1.0.4-105",
        sha1: "0f84c50217505d3774a9ad4b0d25fdcbaa4282c2",
    },
];

const CLEANROOM_RELEASES: &str =
    "https://api.github.com/repos/CleanroomMC/Cleanroom/releases?per_page=30";
const CLEANROOM_DOWNLOADS: &str = "https://github.com/CleanroomMC/Cleanroom/releases/download";
const CLEANROOM_MINECRAFT: &str = "1.12.2";

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

#[derive(Serialize, Deserialize)]
struct LiteLoaderListing {
    versions: BTreeMap<String, LiteLoaderVersion>,
}

#[derive(Serialize, Deserialize)]
struct LiteLoaderVersion {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    artefacts: Option<LiteLoaderBuilds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshots: Option<LiteLoaderBuilds>,
}

#[derive(Serialize, Deserialize)]
struct LiteLoaderBuilds {
    #[serde(rename = "com.mumfrey:liteloader")]
    builds: BTreeMap<String, LiteLoaderBuild>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiteLoaderBuild {
    version: String,
    md5: String,
    tweak_class: String,
    #[serde(default)]
    libraries: Vec<LiteLoaderLibrary>,
}

#[derive(Clone, Serialize, Deserialize)]
struct LiteLoaderLibrary {
    name: String,
}

#[derive(Serialize, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Forge's record of which files a build shipped, as `classifier: {extension: md5}`.
#[derive(Deserialize)]
struct ForgeFiles {
    classifiers: BTreeMap<String, BTreeMap<String, String>>,
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
            LoaderKind::LiteLoader => {
                let listing = self.liteloader_listing().await?;
                Ok(liteloader_builds(&listing, minecraft)
                    .into_iter()
                    .map(|build| LoaderVersion {
                        stable: !build.version.ends_with("-SNAPSHOT"),
                        version: build.version.clone(),
                    })
                    .collect())
            }
            LoaderKind::Rift => Ok(RIFT_BUILDS
                .iter()
                .filter(|build| build.minecraft == minecraft)
                .map(|build| LoaderVersion {
                    version: build.version.to_string(),
                    stable: true,
                })
                .collect()),
            LoaderKind::Cleanroom => {
                if minecraft != CLEANROOM_MINECRAFT {
                    return Ok(Vec::new());
                }
                let releases: Vec<GithubRelease> = self
                    .net
                    .cached_json(CLEANROOM_RELEASES, &cache, hour)
                    .await?;
                Ok(releases
                    .into_iter()
                    .filter(|release| !release.draft)
                    .map(|release| LoaderVersion {
                        version: release.tag_name,
                        stable: !release.prerelease,
                    })
                    .collect())
            }
        }
    }

    async fn liteloader_listing(&self) -> Result<LiteLoaderListing> {
        let cache = self.paths.cache().join("liteloader-versions.json");
        self.net
            .cached_json(LITELOADER_VERSIONS, &cache, Duration::from_secs(3600))
            .await
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
            LoaderKind::LiteLoader => self.install_liteloader(minecraft, version).await,
            LoaderKind::Rift => self.install_rift(minecraft, version).await,
            LoaderKind::Forge if forge_is_a_jar_mod(minecraft) => {
                self.install_jar_forge(minecraft, version).await
            }
            LoaderKind::Cleanroom => self.install_cleanroom(version, task).await,
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

    /// Puts the libraries old FML wants in the game's `lib` folder, where it
    /// finds them and skips the download that would otherwise stop the game.
    /// They're shared on disk, and `lib` isn't in waymarks because it's no one's work.
    pub(crate) async fn add_fml_libraries(&self, instance: &Instance, task: &Task) -> Result<()> {
        let wanted = fml_libraries(&instance.minecraft);
        if instance.loader.kind != LoaderKind::Forge || wanted.is_empty() {
            return Ok(());
        }
        let shared = self.paths.libraries().join("fmllibs");
        let fetches = wanted
            .iter()
            .map(|(name, sha1)| Fetch {
                url: format!("{FML_LIBRARIES}/{name}"),
                dest: shared.join(name),
                hash: Expected::Sha1((*sha1).to_string()),
                size: None,
            })
            .collect();
        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading Forge's libraries")
            })
            .await?;
        let folder = self.paths.game(&instance.id).join("lib");
        tokio::fs::create_dir_all(&folder).await?;
        for (name, sha1) in wanted {
            let (from, to) = (shared.join(name), folder.join(name));
            // A pack can ship its own `lib`. FML checks these and stops on a wrong one.
            if sha1_of(&to).as_deref() == Some(sha1) {
                continue;
            }
            let _ = tokio::fs::remove_file(&to).await;
            if tokio::fs::hard_link(&from, &to).await.is_err() {
                tokio::fs::copy(&from, &to).await?;
            }
        }
        Ok(())
    }

    /// LiteLoader's listing has no ready-made profile, only the parts of one.
    async fn install_liteloader(&self, minecraft: &str, version: &str) -> Result<String> {
        let listing = self.liteloader_listing().await?;
        let build = liteloader_builds(&listing, minecraft)
            .into_iter()
            .find(|build| build.version == version)
            .ok_or_else(|| Error::NotFound(format!("LiteLoader {version}")))?;

        let id = format!("{minecraft}-LiteLoader{version}");
        let name = format!("com.mumfrey:liteloader:{version}");
        let path = maven_path(&name)
            .ok_or_else(|| Error::Other(format!("LiteLoader named a library oddly: {name}")))?;
        let jar = self.paths.libraries().join(&path);
        let url = if version.ends_with("-SNAPSHOT") {
            self.liteloader_snapshot(version).await?
        } else {
            format!("{LITELOADER_REPO}{path}")
        };
        self.net
            .fetch(&Fetch {
                url: url.clone(),
                dest: jar.clone(),
                hash: Expected::Md5(build.md5.clone()),
                size: None,
            })
            .await
            .map_err(|error| match error {
                Error::HashMismatch { .. } => Error::Other(format!(
                    "LiteLoader's file for Minecraft {minecraft} no longer matches its own \
                     listing, so Waymark won't use it"
                )),
                other => other,
            })?;

        let (vanilla, _) = self.resolve_version(minecraft).await?;
        let mut libraries = vec![pinned_library(name, url, sha1_of(&jar))];
        libraries.extend(build.libraries.iter().map(|library| {
            Library {
                url: library
                    .name
                    .starts_with("org.ow2.asm:asm-all:5.2")
                    .then(|| LITELOADER_LIBRARIES.to_string()),
                name: library.name.clone(),
                ..Library::default()
            }
        }));
        let mut arguments = with_tweaker(
            vanilla.minecraft_arguments.as_deref().unwrap_or_default(),
            &build.tweak_class,
        );
        // Before 1.6 it's Mojang's own tweaker that moves the game out of the
        // player's real `.minecraft`, and only a newer launchwrapper can run both.
        if before_1_6(minecraft) {
            arguments = with_tweaker(&arguments, VANILLA_TWEAKER);
            for library in &mut libraries {
                if library.name.starts_with("net.minecraft:launchwrapper:") {
                    library.name = CHAINING_LAUNCHWRAPPER.to_string();
                }
            }
        }
        let profile = VersionMeta {
            id: id.clone(),
            inherits_from: Some(minecraft.to_string()),
            main_class: Some(LAUNCHWRAPPER.to_string()),
            minecraft_arguments: Some(arguments),
            libraries,
            ..VersionMeta::default()
        };
        self.write_profile(&profile).await?;
        Ok(id)
    }

    /// The address of a snapshot's jar. LiteLoader's own address for these passes
    /// through an insecure redirect, so this asks the repository behind it instead.
    async fn liteloader_snapshot(&self, version: &str) -> Result<String> {
        let folder = format!("{LITELOADER_SNAPSHOTS}/{version}");
        let metadata = self
            .net
            .client()
            .get(format!("{folder}/maven-metadata.xml"))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let build = snapshot_build(&metadata)
            .ok_or_else(|| Error::NotFound(format!("LiteLoader {version}")))?;
        let minecraft = version.trim_end_matches("-SNAPSHOT");
        Ok(format!(
            "{folder}/liteloader-{minecraft}-{build}-release.jar"
        ))
    }

    /// Rift's installer only writes to a real `.minecraft`, so this is its
    /// profile with each library pointed at somewhere that still has it.
    async fn install_rift(&self, minecraft: &str, version: &str) -> Result<String> {
        let build = RIFT_BUILDS
            .iter()
            .find(|build| build.minecraft == minecraft && build.version == version)
            .ok_or_else(|| Error::NotFound(format!("Rift {version} for {minecraft}")))?;
        self.net
            .fetch(&Fetch {
                url: RIFT_MIXIN.to_string(),
                dest: self.paths.libraries().join(RIFT_MIXIN_PATH),
                hash: Expected::Sha1(RIFT_MIXIN_SHA1.to_string()),
                size: None,
            })
            .await
            .map_err(|_| {
                Error::Other(
                    "Rift needs a library that only the Internet Archive still has, and it \
                     couldn't be fetched just now. Try again in a while"
                        .into(),
                )
            })?;
        let id = format!("{minecraft}-rift-{version}");
        let maven = |name: &str, url: &str| Library {
            name: name.to_string(),
            url: Some(url.to_string()),
            ..Library::default()
        };
        let profile = VersionMeta {
            id: id.clone(),
            inherits_from: Some(minecraft.to_string()),
            main_class: Some(LAUNCHWRAPPER.to_string()),
            arguments: Some(Arguments {
                game: vec![
                    Argument::Plain("--tweakClass".to_string()),
                    Argument::Plain(RIFT_TWEAKER.to_string()),
                ],
                jvm: Vec::new(),
            }),
            libraries: vec![
                pinned_library(
                    format!("org.dimdev:rift:{version}"),
                    format!("{RIFT_RELEASES}/v{version}/Rift-{version}.jar"),
                    Some(build.sha1.to_string()),
                ),
                pinned_library(
                    "org.dimdev:mixin:0.7.11-SNAPSHOT".to_string(),
                    RIFT_MIXIN.to_string(),
                    Some(RIFT_MIXIN_SHA1.to_string()),
                ),
                maven("org.ow2.asm:asm:6.2", MAVEN_CENTRAL),
                maven("org.ow2.asm:asm-commons:6.2", MAVEN_CENTRAL),
                maven("org.ow2.asm:asm-tree:6.2", MAVEN_CENTRAL),
                Library {
                    name: "net.minecraft:launchwrapper:1.12".to_string(),
                    ..Library::default()
                },
            ],
            ..VersionMeta::default()
        };
        self.write_profile(&profile).await?;
        Ok(id)
    }

    /// Forge before Minecraft 1.6 is a zip of replacement classes rather than an
    /// installer. The zip is laid over the game when it's prepared for launch.
    async fn install_jar_forge(&self, minecraft: &str, version: &str) -> Result<String> {
        let (name, zip) = self.fetch_jar_forge(minecraft, version).await?;
        if !runs_by_itself(&zip)? {
            return Err(Error::Other(format!(
                "Forge {version} needs ModLoader, which has no official download, so Waymark \
                 can't set it up. Forge for Minecraft 1.2.5 and later works"
            )));
        }
        let id = format!("{minecraft}-forge-{version}");
        let profile = VersionMeta {
            id: id.clone(),
            inherits_from: Some(minecraft.to_string()),
            jar_mods: vec![Library {
                name,
                url: Some(FORGE_ROOT.to_string()),
                sha1: sha1_of(&zip),
                ..Library::default()
            }],
            arguments: Some(Arguments {
                game: Vec::new(),
                jvm: OLD_FORGE_FLAGS
                    .iter()
                    .map(|flag| Argument::Plain((*flag).to_string()))
                    .collect(),
            }),
            ..VersionMeta::default()
        };
        self.write_profile(&profile).await?;
        Ok(id)
    }

    /// Fetches the zip for a build of old Forge and says where it landed.
    pub(crate) async fn forge_jar_mod(
        &self,
        minecraft: &str,
        version: &str,
    ) -> Result<std::path::PathBuf> {
        Ok(self.fetch_jar_forge(minecraft, version).await?.1)
    }

    async fn fetch_jar_forge(
        &self,
        minecraft: &str,
        version: &str,
    ) -> Result<(String, std::path::PathBuf)> {
        let full = format!("{minecraft}-{version}");
        let files = self
            .net
            .json::<ForgeFiles>(&format!("{FORGE_FILES}/{full}/meta.json"))
            .await
            .ok();
        let classifier = forge_jar_classifier(files.as_ref(), minecraft, version);
        let md5 = files
            .as_ref()
            .and_then(|files| files.classifiers.get(classifier)?.get("zip"));

        let name = format!("net.minecraftforge:forge:{full}:{classifier}@zip");
        let path = maven_path(&name)
            .ok_or_else(|| Error::Other(format!("Forge named a library oddly: {name}")))?;
        let zip = self.paths.libraries().join(&path);
        self.net
            .fetch(&Fetch {
                url: format!("{FORGE_ROOT}{path}"),
                dest: zip.clone(),
                hash: md5.map_or(Expected::Nothing, |md5| Expected::Md5(md5.clone())),
                size: None,
            })
            .await?;
        Ok((name, zip))
    }

    async fn write_profile(&self, profile: &VersionMeta) -> Result<()> {
        let path = self.paths.version_json(&profile.id);
        crate::net::write_atomic(&path, &serde_json::to_vec(profile)?).await
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

    /// Cleanroom's installer has its own command line and looks for Java 25 on
    /// the computer, but it holds nothing that has to run: a complete profile, and
    /// its one jar nobody else hosts. Reading those out leaves Java to `java.rs`.
    async fn install_cleanroom(&self, version: &str, task: &Task) -> Result<String> {
        let url = installer_url(LoaderKind::Cleanroom, CLEANROOM_MINECRAFT, version)
            .expect("Cleanroom ships installers");
        let installer = self.download_installer(&url, task).await?;
        if needs_running(&installer)? {
            return Err(Error::Other(format!(
                "Cleanroom {version} needs installer steps Waymark doesn't support yet"
            )));
        }
        let id = installed_version_id(&installer)?;
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&installer)?)?;
        let mut profile = Vec::new();
        archive.by_name("version.json")?.read_to_end(&mut profile)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(library) = entry.name()?.strip_prefix("maven/").map(str::to_string) else {
                continue;
            };
            if entry.is_dir() {
                continue;
            }
            let mut jar = Vec::new();
            entry.read_to_end(&mut jar)?;
            let path = self.paths.libraries().join(safe_relative(&library)?);
            crate::net::write_atomic(&path, &jar).await?;
        }
        crate::net::write_atomic(&self.paths.version_json(&id), &profile).await?;
        Ok(id)
    }

    async fn download_installer(&self, url: &str, task: &Task) -> Result<std::path::PathBuf> {
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
        Ok(installer)
    }

    async fn run_installer(&self, url: &str, minecraft: &str, task: &Task) -> Result<String> {
        let installer = self.download_installer(url, task).await?;
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

fn before_1_6(minecraft: &str) -> bool {
    let mut parts = minecraft.split('.').map(|part| part.parse::<u32>().ok());
    matches!(
        (parts.next(), parts.next()),
        (Some(Some(1)), Some(Some(1..=5)))
    )
}

/// Whether a version's Forge is a zip laid over the game: everything from 1.1 to 1.5.2.
pub(crate) fn forge_is_a_jar_mod(minecraft: &str) -> bool {
    before_1_6(minecraft)
}

/// The earliest Forge builds are only mods for Risugami's ModLoader. Without it
/// the game still starts, but as plain Minecraft, because Forge never wakes up.
/// Builds with FML carry their own copy.
fn runs_by_itself(zip: &Path) -> Result<bool> {
    let archive = zip::ZipArchive::new(std::fs::File::open(zip)?)?;
    Ok(archive.index_for_name("ModLoader.class").is_some())
}

/// The file a jar mod build shipped as. Forge's record says, and its early
/// builds, from before it had a server half, went out as `client`.
fn forge_jar_classifier(
    files: Option<&ForgeFiles>,
    minecraft: &str,
    version: &str,
) -> &'static str {
    if let Some(files) = files {
        for classifier in ["universal", "client"] {
            if files.classifiers.contains_key(classifier) {
                return classifier;
            }
        }
    }
    let build = version
        .rsplit('.')
        .next()
        .and_then(|b| b.parse::<u32>().ok());
    let early = minecraft == "1.1" || minecraft.starts_with("1.2.");
    if early || (minecraft == "1.3.2" && build.is_some_and(|build| build < 183)) {
        "client"
    } else {
        "universal"
    }
}

/// What FML needs in `lib`. Forge before 1.3 has no such step.
fn fml_libraries(minecraft: &str) -> Vec<(&'static str, &'static str)> {
    if let Some((_, name, sha1)) = FML_DEOBFUSCATION
        .iter()
        .find(|(version, _, _)| *version == minecraft)
    {
        let mut wanted = FML_1_5.to_vec();
        wanted.push((name, sha1));
        return wanted;
    }
    match minecraft {
        "1.3.2" => FML_1_3.to_vec(),
        "1.4" => FML_1_4.to_vec(),
        version if version.starts_with("1.4.") => FML_1_4.to_vec(),
        _ => Vec::new(),
    }
}

/// A version's LiteLoader builds, full releases first. The listing keeps every
/// old build too, but only the newest of each kind can still be downloaded.
fn liteloader_builds(listing: &LiteLoaderListing, minecraft: &str) -> Vec<LiteLoaderBuild> {
    let Some(version) = listing.versions.get(minecraft) else {
        return Vec::new();
    };
    [&version.artefacts, &version.snapshots]
        .into_iter()
        .flatten()
        .filter_map(|builds| builds.builds.get("latest").cloned())
        .collect()
}

/// The `timestamp-build` a Maven repository gave a snapshot's newest upload.
fn snapshot_build(metadata: &str) -> Option<String> {
    let between = |open: &str, close: &str| -> Option<String> {
        let (_, rest) = metadata.split_once(open)?;
        Some(rest.split_once(close)?.0.trim().to_string())
    };
    Some(format!(
        "{}-{}",
        between("<timestamp>", "</timestamp>")?,
        between("<buildNumber>", "</buildNumber>")?
    ))
}

/// A library at an exact address, for files that aren't laid out like a Maven repository.
fn pinned_library(name: String, url: String, sha1: Option<String>) -> Library {
    Library {
        name,
        downloads: Some(LibraryDownloads {
            artifact: Some(LibraryFile {
                path: None,
                sha1,
                size: None,
                url: Some(url),
            }),
            classifiers: None,
        }),
        ..Library::default()
    }
}

/// Adds a tweaker to an old version's argument string.
fn with_tweaker(arguments: &str, tweaker: &str) -> String {
    format!("{arguments} --tweakClass {tweaker}")
        .trim()
        .to_string()
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

/// Where Forge, NeoForge and Cleanroom publish their installers.
fn installer_url(kind: LoaderKind, minecraft: &str, version: &str) -> Option<String> {
    match kind {
        LoaderKind::NeoForge => Some(format!(
            "{NEOFORGE_MAVEN}/{version}/neoforge-{version}-installer.jar"
        )),
        LoaderKind::Cleanroom => Some(format!(
            "{CLEANROOM_DOWNLOADS}/{version}/cleanroom-{version}-installer.jar"
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

/// Whether an installer has work to do beyond handing over its files.
fn needs_running(installer: &Path) -> Result<bool> {
    #[derive(Deserialize)]
    struct Steps {
        #[serde(default)]
        processors: Vec<serde_json::Value>,
    }
    let mut archive = zip::ZipArchive::new(std::fs::File::open(installer)?)?;
    let entry = archive.by_name("install_profile.json")?;
    let steps: Steps = serde_json::from_reader(entry)?;
    Ok(!steps.processors.is_empty())
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
    fn forge_is_only_a_jar_mod_before_minecraft_1_6() {
        for old in ["1.1", "1.2.5", "1.3.2", "1.4", "1.4.7", "1.5.2"] {
            assert!(forge_is_a_jar_mod(old), "{old}");
        }
        for newer in ["1.6.4", "1.7.10", "1.12.2", "1.21.1", "26.1", "b1.7.3"] {
            assert!(!forge_is_a_jar_mod(newer), "{newer}");
        }
    }

    #[test]
    fn forge_that_needs_modloader_is_told_apart() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, entries: &[&str]| {
            let path = dir.path().join(name);
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            for entry in entries {
                zip.start_file(*entry, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"x").unwrap();
            }
            zip.finish().unwrap();
            path
        };
        let dormant = write("early.zip", &["mod_MinecraftForge.class"]);
        let whole = write("fml.zip", &["ModLoader.class", "cpw/mods/fml/a.class"]);
        assert!(!runs_by_itself(&dormant).unwrap());
        assert!(runs_by_itself(&whole).unwrap());
    }

    #[test]
    fn old_forge_files_are_named_by_what_they_shipped_as() {
        let files = |classifier: &str| ForgeFiles {
            classifiers: BTreeMap::from([(
                classifier.to_string(),
                BTreeMap::from([("zip".to_string(), "abc".to_string())]),
            )]),
        };
        let client = files("client");
        assert_eq!(
            forge_jar_classifier(Some(&client), "1.5.2", "7.8.1.738"),
            "client"
        );
        assert_eq!(
            forge_jar_classifier(Some(&files("universal")), "1.1", "1.3.4.29"),
            "universal"
        );
        // Without Forge's record, the shape of the version decides.
        assert_eq!(forge_jar_classifier(None, "1.1", "1.3.4.29"), "client");
        assert_eq!(forge_jar_classifier(None, "1.2.5", "3.4.9.171"), "client");
        assert_eq!(forge_jar_classifier(None, "1.3.2", "4.0.0.182"), "client");
        assert_eq!(
            forge_jar_classifier(None, "1.3.2", "4.3.5.318"),
            "universal"
        );
        assert_eq!(
            forge_jar_classifier(None, "1.5.2", "7.8.1.738"),
            "universal"
        );
    }

    #[test]
    fn fml_wants_different_libraries_for_each_version() {
        let names = |minecraft| {
            fml_libraries(minecraft)
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        };
        assert!(names("1.2.5").is_empty() && names("1.6.4").is_empty());
        assert_eq!(names("1.3.2").len(), 3);
        assert!(names("1.4.7").contains(&"bcprov-jdk15on-147.jar"));
        assert!(names("1.4").contains(&"guava-12.0.1.jar"));
        assert_eq!(names("1.5.2").len(), 6);
        assert!(names("1.5.2").contains(&"deobfuscation_data_1.5.2.zip"));
        assert!(names("1.5.1").contains(&"deobfuscation_data_1.5.1.zip"));
        assert!(names("1.5").contains(&"deobfuscation_data_1.5.zip"));
        assert!(
            fml_libraries("1.5.2")
                .iter()
                .all(|(_, sha1)| sha1.len() == 40)
        );
    }

    #[test]
    fn liteloader_lists_releases_before_snapshots() {
        let listing: LiteLoaderListing = serde_json::from_str(
            r#"{"versions":{"1.10.2":{
                "artefacts":{"com.mumfrey:liteloader":{"latest":{"version":"1.10.2","md5":"a","tweakClass":"T","libraries":[{"name":"net.minecraft:launchwrapper:1.12"}]}}},
                "snapshots":{"libraries":[{"name":"ignored:ignored:1"}],"com.mumfrey:liteloader":{"latest":{"version":"1.10.2-SNAPSHOT","md5":"b","tweakClass":"T"},"abc":{"version":"old","md5":"c","tweakClass":"T"}}}
            }}}"#,
        )
        .unwrap();
        let builds = liteloader_builds(&listing, "1.10.2");
        let versions: Vec<&str> = builds.iter().map(|b| b.version.as_str()).collect();
        assert_eq!(versions, ["1.10.2", "1.10.2-SNAPSHOT"]);
        assert_eq!(builds[0].libraries.len(), 1);
        assert!(liteloader_builds(&listing, "1.13").is_empty());
    }

    #[test]
    fn a_snapshot_is_found_by_its_newest_upload() {
        let metadata = "<metadata><versioning><snapshot>\n<timestamp>20171128.144431</timestamp>\n<buildNumber>4</buildNumber>\n</snapshot></versioning></metadata>";
        assert_eq!(snapshot_build(metadata).unwrap(), "20171128.144431-4");
        assert!(snapshot_build("<metadata></metadata>").is_none());
    }

    #[test]
    fn tweakers_go_after_the_game_arguments() {
        assert_eq!(
            with_tweaker("--username ${auth_player_name}", "a.Tweaker"),
            "--username ${auth_player_name} --tweakClass a.Tweaker"
        );
        assert_eq!(with_tweaker("", "a.Tweaker"), "--tweakClass a.Tweaker");
    }

    #[test]
    fn rift_only_offers_the_versions_it_was_built_for() {
        let built = |minecraft: &str| RIFT_BUILDS.iter().any(|b| b.minecraft == minecraft);
        assert!(built("1.13") && built("1.13.2"));
        assert!(!built("1.13.1") && !built("1.12.2"));
    }

    #[test]
    fn cleanroom_installers_are_found_by_tag() {
        assert_eq!(
            installer_url(LoaderKind::Cleanroom, "1.12.2", "0.7.14-alpha").unwrap(),
            "https://github.com/CleanroomMC/Cleanroom/releases/download/0.7.14-alpha/cleanroom-0.7.14-alpha-installer.jar"
        );
    }

    #[test]
    fn an_installer_with_nothing_to_run_can_just_be_read() {
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
        assert!(!needs_running(&write("a.jar", r#"{"spec":1,"processors":[]}"#)).unwrap());
        assert!(!needs_running(&write("b.jar", r#"{"spec":1}"#)).unwrap());
        assert!(needs_running(&write("c.jar", r#"{"processors":[{"jar":"x"}]}"#)).unwrap());
    }

    #[test]
    fn maps_minecraft_versions_to_neoforge_numbers() {
        assert_eq!(neoforge_prefix("1.21.4").unwrap(), "21.4.");
        assert_eq!(neoforge_prefix("1.21").unwrap(), "21.0.");
        assert_eq!(neoforge_prefix("26.1").unwrap(), "26.1.");
        assert_eq!(neoforge_prefix("26.1.2").unwrap(), "26.1.2.");
    }
}

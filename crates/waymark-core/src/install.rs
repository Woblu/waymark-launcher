//! Makes a version ready to launch: its metadata, client, libraries, assets,
//! logging config, natives and Java. Files are shared across every instance.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::instance::Instance;
use crate::mojang::{Features, RESOURCES_URL, VERSION_MANIFEST_URL, VersionManifest, VersionMeta};
use crate::net::{Expected, Fetch};

const NATIVES_MARKER: &str = ".waymark-extracted";
const DEFAULT_JAVA: &str = "jre-legacy";

#[derive(Clone, Debug)]
pub struct Prepared {
    pub meta: VersionMeta,
    pub client_jar: PathBuf,
    pub java: PathBuf,
    pub natives: PathBuf,
    pub log_config: Option<PathBuf>,
    pub game_assets: PathBuf,
    /// Set for versions that read their assets from inside the game folder.
    pub resources: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AssetIndex {
    #[serde(default)]
    objects: BTreeMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    is_virtual: bool,
    #[serde(default)]
    map_to_resources: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

impl Core {
    pub async fn version_manifest(&self) -> Result<VersionManifest> {
        let cache = self.paths.cache().join("version-manifest.json");
        self.net
            .cached_json(VERSION_MANIFEST_URL, &cache, Duration::from_secs(600))
            .await
    }

    async fn load_meta(&self, id: &str) -> Result<VersionMeta> {
        let path = self.paths.version_json(id);
        if !path.is_file() {
            let manifest = self.version_manifest().await?;
            let entry = manifest
                .versions
                .iter()
                .find(|v| v.id == id)
                .ok_or_else(|| Error::NotFound(format!("Minecraft {id}")))?;
            let hash = entry.sha1.clone().map_or(Expected::Nothing, Expected::Sha1);
            self.net
                .fetch(&Fetch {
                    url: entry.url.clone(),
                    dest: path.clone(),
                    hash,
                    size: None,
                })
                .await?;
        }
        Ok(serde_json::from_slice(&tokio::fs::read(&path).await?)?)
    }

    /// Follows `inheritsFrom` and returns the merged version, plus the id of
    /// the vanilla version at the bottom, whose jar is the game client.
    pub async fn resolve_version(&self, id: &str) -> Result<(VersionMeta, String)> {
        let mut chain = vec![self.load_meta(id).await?];
        while let Some(parent) = chain.last().and_then(|m| m.inherits_from.clone()) {
            chain.push(self.load_meta(&parent).await?);
        }
        let base = chain.last().map(|m| m.id.clone()).unwrap_or_default();
        let mut merged = chain.pop().ok_or_else(|| Error::NotFound(id.to_string()))?;
        while let Some(child) = chain.pop() {
            merged = child.merged_over(merged);
        }
        Ok((merged, base))
    }

    pub async fn prepare_version(&self, id: &str, task: &Task) -> Result<Prepared> {
        task.progress(0, 1, "Reading version");
        let (meta, base) = self.resolve_version(id).await?;
        let features = Features::default();
        let mut fetches = Vec::new();

        let client_jar = self.paths.version_jar(&base);
        if let Some(client) = meta.downloads.as_ref().and_then(|d| d.get("client")) {
            fetches.push(Fetch {
                url: client.url.clone(),
                dest: client_jar.clone(),
                hash: client
                    .sha1
                    .clone()
                    .map_or(Expected::Nothing, Expected::Sha1),
                size: client.size,
            });
        }

        let mut jar_mods = Vec::new();
        for artifact in meta.jar_mods.iter().filter_map(|m| m.artifact()) {
            push_library(&mut fetches, &self.paths.libraries(), &artifact);
            jar_mods.push(self.paths.libraries().join(&artifact.path));
        }

        let mut native_jars = Vec::new();
        for library in meta.libraries.iter().filter(|l| l.is_allowed(features)) {
            if let Some(artifact) = library.artifact() {
                push_library(&mut fetches, &self.paths.libraries(), &artifact);
            }
            if let Some(native) = library.native() {
                push_library(&mut fetches, &self.paths.libraries(), &native);
                let exclude = library
                    .extract
                    .as_ref()
                    .map(|e| e.exclude.clone())
                    .unwrap_or_default();
                native_jars.push((self.paths.libraries().join(&native.path), exclude));
            }
        }

        let log_config = match meta.logging.as_ref().and_then(|l| l.client.as_ref()) {
            Some(logging) => {
                let dest = self
                    .paths
                    .assets()
                    .join("log_configs")
                    .join(&logging.file.id);
                fetches.push(Fetch {
                    url: logging.file.url.clone(),
                    dest: dest.clone(),
                    hash: Expected::Sha1(logging.file.sha1.clone()),
                    size: Some(logging.file.size),
                });
                Some(dest)
            }
            None => None,
        };

        let mut game_assets = self.paths.assets();
        let mut legacy_assets = None;
        let mut resources = None;
        if let Some(reference) = &meta.asset_index {
            let dest = self
                .paths
                .assets()
                .join("indexes")
                .join(format!("{}.json", reference.id));
            self.net
                .fetch(&Fetch {
                    url: reference.url.clone(),
                    dest: dest.clone(),
                    hash: Expected::Sha1(reference.sha1.clone()),
                    size: Some(reference.size),
                })
                .await?;
            let index: AssetIndex = serde_json::from_slice(&tokio::fs::read(&dest).await?)?;
            for object in index.objects.values() {
                let prefix = &object.hash[..2];
                fetches.push(Fetch {
                    url: format!("{RESOURCES_URL}/{prefix}/{}", object.hash),
                    dest: self
                        .paths
                        .assets()
                        .join("objects")
                        .join(prefix)
                        .join(&object.hash),
                    hash: Expected::Sha1(object.hash.clone()),
                    size: Some(object.size),
                });
            }
            if index.is_virtual || index.map_to_resources {
                game_assets = self.paths.assets().join("virtual").join(&reference.id);
                resources = index.map_to_resources.then(|| game_assets.clone());
                legacy_assets = Some(index);
            }
        }

        self.net
            .fetch_all(fetches, |done, total| {
                task.progress(done, total, "Downloading Minecraft")
            })
            .await?;

        if let Some(index) = legacy_assets {
            self.lay_out_legacy_assets(&index, &game_assets).await?;
        }

        let natives = self.paths.natives(id);
        if !native_jars.is_empty() && !natives.join(NATIVES_MARKER).is_file() {
            task.progress(0, 1, "Unpacking natives");
            let target = natives.clone();
            tokio::task::spawn_blocking(move || extract_natives(&native_jars, &target))
                .await
                .map_err(|e| Error::Other(e.to_string()))??;
        }
        tokio::fs::create_dir_all(&natives).await?;

        let client_jar = if jar_mods.is_empty() {
            self.named_client_jar(id, &base, client_jar).await?
        } else {
            task.progress(0, 1, "Patching the game");
            self.patched_client_jar(id, client_jar, jar_mods).await?
        };

        let component = meta
            .java_version
            .as_ref()
            .map_or(DEFAULT_JAVA, |j| j.component.as_str());
        let java = self.ensure_java(component, task).await?;

        Ok(Prepared {
            meta,
            client_jar,
            java,
            natives,
            log_config,
            game_assets,
            resources,
        })
    }

    /// Minecraft before 1.6 reads its sounds from the game folder, and the
    /// address it used to fetch them from is gone. These are links to the shared
    /// assets, and exports only read the folders they name, so they stay here.
    pub(crate) async fn link_resources(
        &self,
        instance: &Instance,
        prepared: &Prepared,
    ) -> Result<()> {
        let Some(assets) = prepared.resources.clone() else {
            return Ok(());
        };
        let target = self.paths.game(&instance.id).join("resources");
        tokio::task::spawn_blocking(move || link_tree(&assets, &target))
            .await
            .map_err(|e| Error::Other(e.to_string()))?
    }

    /// Forge and NeoForge find the vanilla jar by the modded version's name so
    /// they can leave it out of their module layer. Under any other name it
    /// loads twice and Java refuses to start. A hard link costs no disk space.
    async fn named_client_jar(&self, id: &str, base: &str, vanilla: PathBuf) -> Result<PathBuf> {
        if id == base {
            return Ok(vanilla);
        }
        let named = self.paths.version_jar(id);
        if !named.is_file() {
            tokio::fs::create_dir_all(self.paths.version_dir(id)).await?;
            if tokio::fs::hard_link(&vanilla, &named).await.is_err() {
                tokio::fs::copy(&vanilla, &named).await?;
            }
        }
        Ok(named)
    }

    /// Builds the game jar for a loader that replaces the game's own classes.
    /// This is Mojang's file with changes, so it's made here from the player's
    /// copy and stays in `versions`, which no export or waymark ever reads.
    async fn patched_client_jar(
        &self,
        id: &str,
        vanilla: PathBuf,
        mods: Vec<PathBuf>,
    ) -> Result<PathBuf> {
        let patched = self.paths.version_jar(id);
        if !patched.is_file() {
            tokio::fs::create_dir_all(self.paths.version_dir(id)).await?;
            let target = patched.clone();
            tokio::task::spawn_blocking(move || patch_jar(&vanilla, &mods, &target))
                .await
                .map_err(|e| Error::Other(e.to_string()))??;
        }
        Ok(patched)
    }

    /// Versions before 1.7 read assets by name instead of by hash.
    async fn lay_out_legacy_assets(&self, index: &AssetIndex, root: &Path) -> Result<()> {
        for (name, object) in &index.objects {
            let target = root.join(safe_relative(name)?);
            if target.is_file() {
                continue;
            }
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let source = self
                .paths
                .assets()
                .join("objects")
                .join(&object.hash[..2])
                .join(&object.hash);
            tokio::fs::copy(source, target).await?;
        }
        Ok(())
    }
}

fn push_library(fetches: &mut Vec<Fetch>, root: &Path, artifact: &crate::mojang::LibraryArtifact) {
    if let Some(url) = &artifact.url {
        fetches.push(Fetch {
            url: url.clone(),
            dest: root.join(&artifact.path),
            hash: artifact
                .sha1
                .clone()
                .map_or(Expected::Nothing, Expected::Sha1),
            size: artifact.size,
        });
    }
}

fn link_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        let Ok(relative) = entry.path().strip_prefix(from) else {
            continue;
        };
        let dest = to.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(dest)?;
        } else if !dest.is_file() && fs::hard_link(entry.path(), &dest).is_err() {
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// Signature files only fit the exact bytes they were made for, and Java refuses
/// a package that mixes signed and unsigned classes.
fn is_signature(name: &str) -> bool {
    let Some(file) = name.strip_prefix("META-INF/") else {
        return false;
    };
    let lower = file.to_ascii_lowercase();
    lower == "manifest.mf" || [".sf", ".dsa", ".rsa"].iter().any(|e| lower.ends_with(e))
}

/// Writes the mods over the game. Entries are copied as they are, so nothing is
/// unpacked, and the first copy of a name wins, which is why mods go first.
fn patch_jar(vanilla: &Path, mods: &[PathBuf], target: &Path) -> Result<()> {
    let part = target.with_extension("jar.part");
    let mut jar = zip::ZipWriter::new(fs::File::create(&part)?);
    let mut seen = HashSet::new();
    for source in mods.iter().map(PathBuf::as_path).chain([vanilla]) {
        let base = source == vanilla;
        let mut archive = zip::ZipArchive::new(fs::File::open(source)?)?;
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i)?;
            let name = entry.name()?.to_string();
            let unwanted = if base {
                name.starts_with("META-INF/")
            } else {
                is_signature(&name)
            };
            if !entry.is_dir() && !unwanted && seen.insert(name) {
                jar.raw_copy_file(entry)?;
            }
        }
    }
    jar.finish()?;
    fs::rename(part, target)?;
    Ok(())
}

fn extract_natives(jars: &[(PathBuf, Vec<String>)], target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for (jar, exclude) in jars {
        let mut archive = zip::ZipArchive::new(fs::File::open(jar)?)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name()?.to_string();
            if entry.is_dir() || name.ends_with(".git") || name.ends_with(".sha1") {
                continue;
            }
            if name.starts_with("META-INF/")
                || exclude.iter().any(|prefix| name.starts_with(prefix))
            {
                continue;
            }
            let dest = target.join(safe_relative(&name)?);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            io::copy(&mut entry, &mut fs::File::create(dest)?)?;
        }
    }
    fs::write(target.join(NATIVES_MARKER), b"")?;
    Ok(())
}

/// Rejects paths that would escape their folder. Pack files and archives come
/// from strangers, so a name like `../../startup/evil.exe` has to be refused.
pub fn safe_relative(path: &str) -> Result<PathBuf> {
    let candidate = Path::new(path);
    let safe = !path.is_empty()
        && !path.contains('\\')
        && candidate
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
    if safe {
        Ok(candidate.to_path_buf())
    } else {
        Err(Error::Other(format!("refused an unsafe path: {path}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, text) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn read_zip(path: &Path) -> BTreeMap<String, String> {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut entry = archive.by_index(i).unwrap();
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                (entry.name().unwrap().to_string(), text)
            })
            .collect()
    }

    #[test]
    fn a_patched_jar_is_the_game_with_the_mod_over_it() {
        let dir = tempfile::tempdir().unwrap();
        let (game, forge, out) = (
            dir.path().join("game.jar"),
            dir.path().join("forge.zip"),
            dir.path().join("patched.jar"),
        );
        write_zip(
            &game,
            &[
                ("Minecraft.class", "vanilla"),
                ("Other.class", "untouched"),
                ("META-INF/MOJANG_C.SF", "signature"),
                ("META-INF/MANIFEST.MF", "digests"),
            ],
        );
        write_zip(
            &forge,
            &[
                ("Minecraft.class", "patched"),
                ("forge/Hook.class", "new"),
                ("META-INF/FORGE.RSA", "signature"),
                ("META-INF/services/thing", "kept"),
            ],
        );

        patch_jar(&game, &[forge], &out).unwrap();

        let jar = read_zip(&out);
        assert_eq!(jar["Minecraft.class"], "patched");
        assert_eq!(jar["Other.class"], "untouched");
        assert_eq!(jar["forge/Hook.class"], "new");
        assert_eq!(jar["META-INF/services/thing"], "kept");
        assert_eq!(jar.len(), 4);
        assert!(!dir.path().join("patched.jar.part").exists());
    }

    #[test]
    fn linking_resources_leaves_the_players_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("assets"), dir.path().join("resources"));
        fs::create_dir_all(from.join("sound/step")).unwrap();
        fs::write(from.join("sound/step/grass.ogg"), "grass").unwrap();
        fs::write(from.join("icon.png"), "icon").unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(to.join("icon.png"), "theirs").unwrap();

        link_tree(&from, &to).unwrap();

        assert_eq!(
            fs::read_to_string(to.join("sound/step/grass.ogg")).unwrap(),
            "grass"
        );
        assert_eq!(fs::read_to_string(to.join("icon.png")).unwrap(), "theirs");
    }

    #[test]
    fn accepts_ordinary_relative_paths() {
        assert!(safe_relative("mods/sodium.jar").is_ok());
        assert!(safe_relative("config/deep/file.toml").is_ok());
    }

    #[test]
    fn refuses_paths_that_escape() {
        for path in [
            "../evil.exe",
            "mods/../../evil.exe",
            "/etc/passwd",
            "C:\\Windows\\evil.exe",
            "..\\evil.exe",
            "",
        ] {
            assert!(safe_relative(path).is_err(), "{path} should be refused");
        }
    }
}

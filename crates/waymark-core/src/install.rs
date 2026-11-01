//! Makes a version ready to launch: its metadata, client, libraries, assets,
//! logging config, natives and Java. Files are shared across every instance.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
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

        let client_jar = self.named_client_jar(id, &base, client_jar).await?;

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
        })
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

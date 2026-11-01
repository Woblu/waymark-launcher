//! Exports an instance as a modpack, or as a plain list of what's in it. Files
//! a store can provide are linked, so nothing is redistributed. Anything else
//! has to travel inside the pack, and the report names those files so the
//! player can check they're allowed to share them before sending it on.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use walkdir::WalkDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::Core;
use crate::content::ContentKind;
use crate::error::{Error, Result};
use crate::instance::Instance;
use crate::lock::{Loader, LoaderKind, LockedMod, Lockfile, SHELVES, Side, Source, content_folder};
use crate::modrinth;
use crate::waymarks::TRACKED_FILES;

const OVERRIDE_FOLDERS: [&str; 3] = ["config", "defaultconfigs", "kubejs"];
const LOOKUP_BATCH: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackFormat {
    Modrinth,
    CurseForge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListFormat {
    Text,
    Markdown,
    Csv,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackExport {
    pub linked: usize,
    /// Files that had to go inside the pack.
    pub bundled: Vec<String>,
}

/// A file a store provides, ready to be listed in a Modrinth index.
struct Linked {
    path: String,
    sha1: String,
    sha512: String,
    size: u64,
    url: String,
    side: Side,
}

fn side_support(side: Side) -> Value {
    let (client, server) = match side {
        Side::Both => ("required", "required"),
        Side::Client => ("required", "unsupported"),
        Side::Server => ("unsupported", "required"),
    };
    json!({ "client": client, "server": server })
}

fn modrinth_loader(kind: LoaderKind) -> Option<&'static str> {
    match kind {
        LoaderKind::Fabric => Some("fabric-loader"),
        LoaderKind::Quilt => Some("quilt-loader"),
        LoaderKind::Forge => Some("forge"),
        LoaderKind::NeoForge => Some("neoforge"),
        _ => None,
    }
}

fn modrinth_index(name: &str, version: &str, lock: &Lockfile, linked: &[Linked]) -> Value {
    let mut dependencies = BTreeMap::from([("minecraft".to_string(), lock.minecraft.clone())]);
    if let (Some(key), Some(loader)) = (
        modrinth_loader(lock.loader.kind),
        lock.loader.version.clone(),
    ) {
        dependencies.insert(key.to_string(), loader);
    }
    let files: Vec<Value> = linked
        .iter()
        .map(|file| {
            json!({
                "path": file.path,
                "hashes": { "sha1": file.sha1, "sha512": file.sha512 },
                "env": side_support(file.side),
                "downloads": [file.url],
                "fileSize": file.size,
            })
        })
        .collect();
    json!({ "formatVersion": 1, "game": "minecraft", "versionId": version, "name": name, "files": files, "dependencies": dependencies })
}

fn curseforge_loader(loader: &Loader) -> Option<String> {
    Some(format!(
        "{}-{}",
        loader.kind.slug(),
        loader.version.as_ref()?
    ))
}

fn curseforge_manifest(
    name: &str,
    version: &str,
    author: &str,
    lock: &Lockfile,
    files: &[(u64, u64)],
) -> Value {
    let loaders: Vec<Value> = curseforge_loader(&lock.loader)
        .map(|id| json!({ "id": id, "primary": true }))
        .into_iter()
        .collect();
    let files: Vec<Value> = files
        .iter()
        .map(|(project, file)| json!({ "projectID": project, "fileID": file, "required": true }))
        .collect();
    json!({
        "minecraft": { "version": lock.minecraft, "modLoaders": loaders },
        "manifestType": "minecraftModpack",
        "manifestVersion": 1,
        "name": name,
        "version": version,
        "author": author,
        "files": files,
        "overrides": "overrides",
    })
}

/// Writes the pack's index and every override into a zip, in one go so a
/// failure never leaves half a pack behind.
fn write_pack(
    target: &Path,
    index_name: &str,
    index: &Value,
    overrides: &[(String, PathBuf)],
) -> Result<()> {
    let part = target.with_extension("part");
    let result = (|| -> Result<()> {
        let mut zip = ZipWriter::new(fs::File::create(&part)?);
        let options = SimpleFileOptions::default();
        let problem = |error: zip::result::ZipError| Error::Other(error.to_string());
        zip.start_file(index_name, options).map_err(problem)?;
        zip.write_all(serde_json::to_string_pretty(index)?.as_bytes())?;
        for (relative, source) in overrides {
            zip.start_file(format!("overrides/{relative}"), options)
                .map_err(problem)?;
            zip.write_all(&fs::read(source)?)?;
        }
        zip.finish().map_err(problem)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result?;
    fs::rename(part, target)?;
    Ok(())
}

/// The settings and config files every pack carries.
fn config_overrides(game: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for folder in OVERRIDE_FOLDERS {
        let root = game.join(folder);
        for entry in WalkDir::new(&root)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let relative = entry
                .path()
                .strip_prefix(game)
                .map_err(|e| Error::Other(e.to_string()))?;
            files.push((
                relative.to_string_lossy().replace('\\', "/"),
                entry.into_path(),
            ));
        }
    }
    for file in TRACKED_FILES {
        let path = game.join(file);
        if path.is_file() {
            files.push((file.to_string(), path));
        }
    }
    Ok(files)
}

/// Everything switched on that belongs in a pack, as a path inside the game
/// folder. Data packs live in worlds, which packs don't carry.
fn pack_entries(lock: &Lockfile) -> Vec<(ContentKind, String, &LockedMod)> {
    SHELVES
        .into_iter()
        .filter(|kind| *kind != ContentKind::DataPack)
        .flat_map(|kind| {
            lock.shelf(kind)
                .iter()
                .filter(|entry| entry.enabled)
                .map(move |entry| {
                    let folder = content_folder(kind, entry)
                        .to_string_lossy()
                        .replace('\\', "/");
                    (kind, format!("{folder}/{}", entry.file), entry)
                })
        })
        .filter(|(_, _, entry)| !entry.hash.starts_with(crate::lock::UNPACKED))
        .collect()
}

fn csv_cell(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

fn store_link(entry: &LockedMod) -> Option<String> {
    match &entry.source {
        Source::Modrinth { project, .. } => Some(format!("https://modrinth.com/project/{project}")),
        _ => None,
    }
}

fn mod_list_text(lock: &Lockfile, format: ListFormat) -> String {
    let entries: Vec<&LockedMod> = SHELVES
        .into_iter()
        .filter(|kind| *kind != ContentKind::DataPack)
        .flat_map(|kind| lock.shelf(kind).iter())
        .filter(|entry| entry.enabled)
        .collect();
    let version = |entry: &LockedMod| entry.version.clone().unwrap_or_default();
    let mut out = String::new();
    match format {
        ListFormat::Text => {
            for entry in entries {
                out.push_str(&format!("{} {}\n", entry.name, version(entry)).replace(" \n", "\n"));
            }
        }
        ListFormat::Markdown => {
            for entry in entries {
                let title = store_link(entry).map_or_else(
                    || entry.name.clone(),
                    |url| format!("[{}]({url})", entry.name),
                );
                out.push_str(&format!("- {title} {}\n", version(entry)).replace(" \n", "\n"));
            }
        }
        ListFormat::Csv => {
            out.push_str("Name,Version,File,Link\n");
            for entry in entries {
                let cells = [
                    entry.name.clone(),
                    version(entry),
                    entry.file.clone(),
                    store_link(entry).unwrap_or_default(),
                ];
                out.push_str(&cells.map(|cell| csv_cell(&cell)).join(","));
                out.push('\n');
            }
        }
    }
    out
}

impl Core {
    pub fn mod_list(&self, id: &str, format: ListFormat) -> Result<String> {
        Ok(mod_list_text(&self.lockfile(id)?, format))
    }

    /// Writes an instance to `target` as a pack others can install.
    pub async fn export_pack(
        &self,
        id: &str,
        target: &Path,
        format: PackFormat,
        name: &str,
        version: &str,
    ) -> Result<PackExport> {
        let instance = self.instance(id)?;
        let lock = self.lockfile(id)?;
        let name = if name.trim().is_empty() {
            instance.name.as_str()
        } else {
            name.trim()
        };
        match format {
            PackFormat::Modrinth => {
                self.export_modrinth(&instance, &lock, target, name, version)
                    .await
            }
            PackFormat::CurseForge => {
                self.export_curseforge(&instance, &lock, target, name, version)
            }
        }
    }

    async fn export_modrinth(
        &self,
        instance: &Instance,
        lock: &Lockfile,
        target: &Path,
        name: &str,
        version: &str,
    ) -> Result<PackExport> {
        let entries = pack_entries(lock);
        let hashes: Vec<String> = entries
            .iter()
            .filter_map(|(_, _, entry)| entry.remote_hash.clone())
            .collect();
        let mut known = std::collections::HashMap::new();
        for chunk in hashes.chunks(LOOKUP_BATCH) {
            known.extend(modrinth::versions_for_files(&self.net, chunk).await?);
        }

        let game = self.paths.game(&instance.id);
        let mut overrides = config_overrides(&game)?;
        let mut linked = Vec::new();
        let mut bundled = Vec::new();
        for (kind, path, entry) in &entries {
            let file = entry.remote_hash.as_ref().and_then(|hash| {
                known
                    .get(hash)?
                    .files
                    .iter()
                    .find(|file| file.hashes.sha512 == *hash)
            });
            match file.and_then(|file| Some((file, file.hashes.sha1.clone()?))) {
                Some((file, sha1)) => linked.push(Linked {
                    path: path.clone(),
                    sha1,
                    sha512: file.hashes.sha512.clone(),
                    size: file.size,
                    url: file.url.clone(),
                    side: entry.side,
                }),
                None => {
                    bundled.push(entry.name.clone());
                    overrides.push((
                        path.clone(),
                        game.join(content_folder(*kind, entry))
                            .join(entry.disk_name()),
                    ));
                }
            }
        }
        let index = modrinth_index(name, version, lock, &linked);
        write_pack(target, "modrinth.index.json", &index, &overrides)?;
        Ok(PackExport {
            linked: linked.len(),
            bundled,
        })
    }

    fn export_curseforge(
        &self,
        instance: &Instance,
        lock: &Lockfile,
        target: &Path,
        name: &str,
        version: &str,
    ) -> Result<PackExport> {
        let game = self.paths.game(&instance.id);
        let mut overrides = config_overrides(&game)?;
        let mut files = Vec::new();
        let mut bundled = Vec::new();
        for (kind, path, entry) in pack_entries(lock) {
            if let Source::CurseForge { project, file } = entry.source {
                files.push((project, file));
            } else {
                bundled.push(entry.name.clone());
                overrides.push((
                    path,
                    game.join(content_folder(kind, entry))
                        .join(entry.disk_name()),
                ));
            }
        }
        let author = self
            .accounts()?
            .into_iter()
            .find(|account| account.active)
            .map_or_else(String::new, |account| account.name);
        let manifest = curseforge_manifest(name, version, &author, lock, &files);
        write_pack(target, "manifest.json", &manifest, &overrides)?;
        Ok(PackExport {
            linked: files.len(),
            bundled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Origin;

    fn entry(name: &str, source: Source) -> LockedMod {
        LockedMod {
            name: name.into(),
            file: format!("{name}.jar"),
            hash: format!("{name}-hash"),
            size: 1,
            enabled: true,
            source,
            origin: Origin::Manual,
            side: Side::Both,
            version: Some("1.0".into()),
            icon: None,
            group: None,
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        }
    }

    fn lock() -> Lockfile {
        let mut lock = Lockfile::new(
            "1.21.4",
            Loader {
                kind: LoaderKind::Fabric,
                version: Some("0.16.9".into()),
            },
        );
        lock.mods.push(entry(
            "sodium",
            Source::Modrinth {
                project: "AANobbMI".into(),
                version: "x".into(),
            },
        ));
        lock.mods.push(entry(
            "jei",
            Source::CurseForge {
                project: 238222,
                file: 77,
            },
        ));
        let mut off = entry("off", Source::Manual);
        off.enabled = false;
        lock.mods.push(off);
        lock
    }

    #[test]
    fn a_modrinth_index_matches_the_format() {
        let linked = [Linked {
            path: "mods/sodium.jar".into(),
            sha1: "a".into(),
            sha512: "b".into(),
            size: 9,
            url: "https://cdn.modrinth.com/data/x/sodium.jar".into(),
            side: Side::Client,
        }];
        let index = modrinth_index("Pack", "1.0.0", &lock(), &linked);
        assert_eq!(index["formatVersion"], 1);
        assert_eq!(index["dependencies"]["minecraft"], "1.21.4");
        assert_eq!(index["dependencies"]["fabric-loader"], "0.16.9");
        assert_eq!(index["files"][0]["hashes"]["sha512"], "b");
        assert_eq!(index["files"][0]["env"]["server"], "unsupported");
        assert_eq!(
            index["files"][0]["downloads"][0],
            "https://cdn.modrinth.com/data/x/sodium.jar"
        );
    }

    #[test]
    fn a_curseforge_manifest_matches_the_format() {
        let manifest = curseforge_manifest("Pack", "1.0.0", "Me", &lock(), &[(238222, 77)]);
        assert_eq!(manifest["manifestType"], "minecraftModpack");
        assert_eq!(
            manifest["minecraft"]["modLoaders"][0]["id"],
            "fabric-0.16.9"
        );
        assert_eq!(manifest["files"][0]["projectID"], 238222);
        assert_eq!(manifest["overrides"], "overrides");
    }

    #[test]
    fn switched_off_mods_stay_out() {
        let lock = lock();
        let names: Vec<&str> = pack_entries(&lock)
            .iter()
            .map(|(_, _, entry)| entry.name.as_str())
            .collect();
        assert_eq!(names, ["sodium", "jei"]);
    }

    #[test]
    fn a_curseforge_export_links_what_it_can_and_names_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance(
                "Test",
                "1.21.4",
                Loader {
                    kind: LoaderKind::Fabric,
                    version: Some("0.16.9".into()),
                },
                "x".into(),
                None,
            )
            .unwrap();
        let game = core.paths.game(&instance.id);
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(game.join("config/a.toml"), b"a = 1").unwrap();
        fs::write(game.join("options.txt"), b"fov:90").unwrap();
        fs::write(game.join("mods/sodium.jar"), b"jar").unwrap();
        let mut lock = lock();
        lock.mods.retain(|m| m.name != "off");
        lock.save(&core.paths.instance(&instance.id)).unwrap();

        let target = dir.path().join("pack.zip");
        let report = core
            .export_curseforge(&instance, &lock, &target, "Pack", "1.0.0")
            .unwrap();

        assert_eq!(report.linked, 1);
        assert_eq!(report.bundled, ["sodium"]);
        let mut archive = zip::ZipArchive::new(fs::File::open(&target).unwrap()).unwrap();
        let names: Vec<String> = archive
            .file_names()
            .filter_map(|name| Some(name.ok()?.into_owned()))
            .collect();
        for expected in [
            "manifest.json",
            "overrides/config/a.toml",
            "overrides/options.txt",
            "overrides/mods/sodium.jar",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "{expected} missing from {names:?}"
            );
        }
        assert!(archive.by_name("manifest.json").is_ok());
    }

    #[test]
    fn lists_come_in_each_format() {
        let lock = lock();
        assert_eq!(
            mod_list_text(&lock, ListFormat::Text),
            "sodium 1.0\njei 1.0\n"
        );
        assert!(
            mod_list_text(&lock, ListFormat::Markdown)
                .contains("- [sodium](https://modrinth.com/project/AANobbMI) 1.0")
        );
        let csv = mod_list_text(&lock, ListFormat::Csv);
        assert!(csv.starts_with("Name,Version,File,Link\n"));
        assert!(csv.contains("\"jei\",\"1.0\",\"jei.jar\",\"\""));
    }

    #[test]
    fn a_comma_or_quote_cannot_break_a_csv() {
        assert_eq!(csv_cell("a,\"b\""), "\"a,\"\"b\"\"\"");
    }
}

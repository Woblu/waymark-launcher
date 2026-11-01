//! Moving in from other launchers. Importing only ever reads the original,
//! so nobody loses anything by trying Waymark.

use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::instance::Instance;
use crate::lock::{Loader, LoaderKind};
use crate::store::Placement;
use crate::waymarks::WaymarkKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExternalLauncher {
    Prism,
    MultiMc,
    CurseForge,
    Modrinth,
    #[serde(rename = "atlauncher")]
    AtLauncher,
    #[serde(rename = "gdlauncher")]
    GdLauncher,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalInstance {
    pub launcher: ExternalLauncher,
    pub name: String,
    pub game_dir: PathBuf,
    pub minecraft: String,
    pub loader: Loader,
}

#[derive(Deserialize)]
struct PrismPack {
    components: Vec<PrismComponent>,
}

#[derive(Deserialize)]
struct PrismComponent {
    uid: String,
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseInstance {
    name: String,
    game_version: String,
    #[serde(default)]
    base_mod_loader: Option<CurseBaseLoader>,
}

#[derive(Deserialize)]
struct CurseBaseLoader {
    name: String,
}

fn cfg_value(cfg: &str, key: &str) -> Option<String> {
    cfg.lines().find_map(|line| {
        line.strip_prefix(key)?
            .strip_prefix('=')
            .map(|v| v.trim().to_string())
    })
}

fn prism_instance(dir: &Path, launcher: ExternalLauncher) -> Option<ExternalInstance> {
    let cfg = fs::read_to_string(dir.join("instance.cfg")).ok()?;
    let pack: PrismPack =
        serde_json::from_slice(&fs::read(dir.join("mmc-pack.json")).ok()?).ok()?;
    let game_dir = [".minecraft", "minecraft"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_dir())?;
    let mut minecraft = None;
    let mut loader = Loader::default();
    for component in pack.components {
        let kind = match component.uid.as_str() {
            "net.minecraft" => {
                minecraft = component.version;
                continue;
            }
            "net.fabricmc.fabric-loader" => LoaderKind::Fabric,
            "org.quiltmc.quilt-loader" => LoaderKind::Quilt,
            "net.neoforged" => LoaderKind::NeoForge,
            "net.minecraftforge" => LoaderKind::Forge,
            _ => continue,
        };
        loader = Loader {
            kind,
            version: component.version,
        };
    }
    Some(ExternalInstance {
        launcher,
        name: cfg_value(&cfg, "name").unwrap_or_else(|| {
            dir.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        }),
        game_dir,
        minecraft: minecraft?,
        loader,
    })
}

fn loader_named(name: &str, version: Option<String>) -> Loader {
    let kind = match name.to_ascii_lowercase().as_str() {
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::NeoForge,
        _ => return Loader::default(),
    };
    Loader { kind, version }
}

#[derive(Deserialize)]
struct AtInstance {
    /// The Minecraft version, despite the name.
    id: String,
    launcher: AtDetails,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AtDetails {
    name: String,
    #[serde(default)]
    loader_version: Option<AtLoader>,
}

#[derive(Deserialize)]
struct AtLoader {
    #[serde(rename = "type")]
    kind: String,
    version: String,
}

fn scan_atlauncher(root: &Path) -> Vec<ExternalInstance> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let dir = entry.ok()?.path();
            let found: AtInstance =
                serde_json::from_slice(&fs::read(dir.join("instance.json")).ok()?).ok()?;
            Some(ExternalInstance {
                launcher: ExternalLauncher::AtLauncher,
                name: found.launcher.name,
                game_dir: dir,
                minecraft: found.id,
                loader: found
                    .launcher
                    .loader_version
                    .map_or_else(Loader::default, |loader| {
                        loader_named(&loader.kind, Some(loader.version))
                    }),
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct GdConfig {
    loader: GdLoader,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GdLoader {
    loader_type: String,
    #[serde(default)]
    loader_version: Option<String>,
    mc_version: String,
    #[serde(default)]
    source_name: Option<String>,
}

fn scan_gdlauncher(root: &Path) -> Vec<ExternalInstance> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let dir = entry.ok()?.path();
            let found: GdConfig =
                serde_json::from_slice(&fs::read(dir.join("config.json")).ok()?).ok()?;
            let folder = dir.file_name()?.to_string_lossy().into_owned();
            Some(ExternalInstance {
                launcher: ExternalLauncher::GdLauncher,
                name: found.loader.source_name.unwrap_or(folder),
                minecraft: found.loader.mc_version,
                loader: loader_named(&found.loader.loader_type, found.loader.loader_version),
                game_dir: dir,
            })
        })
        .collect()
}

fn scan_prism(root: &Path, launcher: ExternalLauncher) -> Vec<ExternalInstance> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| prism_instance(&e.ok()?.path(), launcher))
        .collect()
}

/// CurseForge names its loaders like `fabric-0.16.9-1.21.4` or `forge-47.3.0`.
fn curse_base_loader(name: &str, minecraft: &str) -> Loader {
    let (kind, rest) = name.split_once('-').unwrap_or((name, ""));
    let kind = match kind {
        "fabric" => LoaderKind::Fabric,
        "quilt" => LoaderKind::Quilt,
        "neoforge" => LoaderKind::NeoForge,
        "forge" => LoaderKind::Forge,
        _ => return Loader::default(),
    };
    let version = rest.trim_end_matches(&format!("-{minecraft}"));
    Loader {
        kind,
        version: (!version.is_empty()).then(|| version.to_string()),
    }
}

fn scan_curseforge(root: &Path) -> Vec<ExternalInstance> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let dir = entry.ok()?.path();
            let meta: CurseInstance =
                serde_json::from_slice(&fs::read(dir.join("minecraftinstance.json")).ok()?).ok()?;
            let loader = meta.base_mod_loader.map_or_else(Loader::default, |l| {
                curse_base_loader(&l.name, &meta.game_version)
            });
            Some(ExternalInstance {
                launcher: ExternalLauncher::CurseForge,
                name: meta.name,
                game_dir: dir,
                minecraft: meta.game_version,
                loader,
            })
        })
        .collect()
}

/// The Modrinth App keeps its profiles in a SQLite database.
fn scan_modrinth(root: &Path) -> Vec<ExternalInstance> {
    read_modrinth(root).unwrap_or_else(|error| {
        tracing::debug!(%error, "no readable Modrinth App database");
        Vec::new()
    })
}

fn read_modrinth(root: &Path) -> rusqlite::Result<Vec<ExternalInstance>> {
    let database =
        Connection::open_with_flags(root.join("app.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut query = database
        .prepare("SELECT path, name, game_version, mod_loader, mod_loader_version FROM profiles")?;
    let rows = query.query_map([], |row| {
        let path: String = row.get(0)?;
        let name: String = row.get(1)?;
        let minecraft: String = row.get(2)?;
        let loader: String = row.get(3)?;
        let version: Option<String> = row.get(4)?;
        Ok((path, name, minecraft, loader, version))
    })?;
    let mut instances = Vec::new();
    for row in rows {
        let (path, name, minecraft, loader, version) = row?;
        let kind = match loader.as_str() {
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            "neoforge" => LoaderKind::NeoForge,
            "forge" => LoaderKind::Forge,
            _ => LoaderKind::Vanilla,
        };
        instances.push(ExternalInstance {
            launcher: ExternalLauncher::Modrinth,
            name,
            game_dir: root.join("profiles").join(path),
            minecraft,
            loader: Loader { kind, version },
        });
    }
    Ok(instances)
}

/// Links immutable archives into the store and copies everything else. This
/// walks the whole game folder, so worlds come along too.
/// Copies a game folder, keeping everything that never changes in the store
/// so it only takes up disk space once. `skip` names top-level entries to leave out.
pub(crate) fn copy_game_dir(
    core: &Core,
    from: &Path,
    to: &Path,
    skip: &[&str],
    task: &Task,
) -> Result<()> {
    let leave_out = |entry: &walkdir::DirEntry| {
        entry.depth() == 1
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| skip.contains(&name))
    };
    let files: Vec<PathBuf> = WalkDir::new(from)
        .into_iter()
        .filter_entry(|entry| !leave_out(entry))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    let total = files.len() as u64;
    for (done, source) in files.iter().enumerate() {
        task.progress(done as u64, total, "Copying files");
        let relative = source
            .strip_prefix(from)
            .map_err(|e| Error::Other(e.to_string()))?;
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = relative.to_string_lossy().replace('\\', "/");
        let stored_folder = ["mods/", "resourcepacks/", "shaderpacks/"]
            .iter()
            .any(|folder| text.starts_with(folder));
        if stored_folder && Placement::for_path(&text) == Placement::Link {
            let stored = core.store.add(source, Placement::Link)?;
            core.store.place(&stored.hash, &target, Placement::Link)?;
        } else {
            fs::copy(source, &target)?;
        }
    }
    Ok(())
}

impl Core {
    /// Every instance we can find in other launchers on this machine.
    pub fn find_external_instances(&self) -> Vec<ExternalInstance> {
        let Some(base) = BaseDirs::new() else {
            return Vec::new();
        };
        let data = base.data_dir();
        let mut found = scan_prism(
            &data.join("PrismLauncher/instances"),
            ExternalLauncher::Prism,
        );
        found.extend(scan_prism(
            &data.join("MultiMC/instances"),
            ExternalLauncher::MultiMc,
        ));
        found.extend(scan_curseforge(
            &base.home_dir().join("curseforge/minecraft/Instances"),
        ));
        found.extend(scan_modrinth(&data.join("ModrinthApp")));
        found.extend(scan_atlauncher(&data.join("ATLauncher/instances")));
        found.extend(scan_gdlauncher(&data.join("gdlauncher_next/instances")));
        found
    }

    pub async fn import_external(&self, external: ExternalInstance) -> Result<Instance> {
        let task = Task::start(&self.events, format!("Importing {}", external.name));
        match self.run_import(&external, &task).await {
            Ok(instance) => {
                task.finish();
                Ok(instance)
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    async fn run_import(&self, external: &ExternalInstance, task: &Task) -> Result<Instance> {
        let version_id = self
            .install_loader(
                external.loader.kind,
                &external.minecraft,
                external.loader.version.as_deref().unwrap_or(""),
                task,
            )
            .await?;
        let instance = self.register_instance(
            &external.name,
            &external.minecraft,
            external.loader.clone(),
            version_id,
            None,
        )?;
        let core = self.clone();
        let game = self.paths.game(&instance.id);
        let id = instance.id.clone();
        let source = external.game_dir.clone();
        tokio::task::block_in_place(|| {
            copy_game_dir(&core, &source, &game, &[], task)?;
            core.trail(&id).mark("Imported", WaymarkKind::Import)
        })?;
        Ok(instance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_atlauncher_and_gdlauncher_instances() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("at/SkyPack");
        fs::create_dir_all(&at).unwrap();
        fs::write(at.join("instance.json"), r#"{"id":"1.20.1","launcher":{"name":"Sky Pack","pack":"Sky","version":"2","loaderVersion":{"type":"Forge","version":"47.2.0"}}}"#).unwrap();
        let found = scan_atlauncher(&dir.path().join("at"));
        assert_eq!(found[0].name, "Sky Pack");
        assert_eq!(found[0].minecraft, "1.20.1");
        assert_eq!(found[0].loader.kind, LoaderKind::Forge);
        assert_eq!(found[0].game_dir, at);

        let gd = dir.path().join("gd/Fancy");
        fs::create_dir_all(&gd).unwrap();
        fs::write(
            gd.join("config.json"),
            r#"{"loader":{"loaderType":"fabric","loaderVersion":"0.16.9","mcVersion":"1.21.4"}}"#,
        )
        .unwrap();
        let found = scan_gdlauncher(&dir.path().join("gd"));
        assert_eq!(found[0].name, "Fancy");
        assert_eq!(found[0].loader.kind, LoaderKind::Fabric);
        assert_eq!(found[0].loader.version.as_deref(), Some("0.16.9"));
    }

    #[test]
    fn reads_a_prism_instance() {
        let dir = tempfile::tempdir().unwrap();
        let instance = dir.path().join("my-pack");
        fs::create_dir_all(instance.join(".minecraft/mods")).unwrap();
        fs::write(instance.join("instance.cfg"), "[General]\nname=My Pack\n").unwrap();
        fs::write(
            instance.join("mmc-pack.json"),
            r#"{"components":[{"uid":"net.minecraft","version":"1.21.4"},{"uid":"net.fabricmc.fabric-loader","version":"0.16.9"}]}"#,
        )
        .unwrap();

        let found = scan_prism(dir.path(), ExternalLauncher::Prism);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "My Pack");
        assert_eq!(found[0].minecraft, "1.21.4");
        assert_eq!(
            found[0].loader,
            Loader {
                kind: LoaderKind::Fabric,
                version: Some("0.16.9".into())
            }
        );
        assert!(found[0].game_dir.ends_with(".minecraft"));
    }

    #[test]
    fn skips_folders_that_are_not_instances() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("_LAUNCHER_TEMP")).unwrap();
        assert!(scan_prism(dir.path(), ExternalLauncher::Prism).is_empty());
    }

    #[test]
    fn reads_a_curseforge_instance() {
        let dir = tempfile::tempdir().unwrap();
        let instance = dir.path().join("pack");
        fs::create_dir_all(&instance).unwrap();
        fs::write(
            instance.join("minecraftinstance.json"),
            r#"{"name":"ATM10","gameVersion":"1.21.1","baseModLoader":{"name":"neoforge-21.1.77"}}"#,
        )
        .unwrap();

        let found = scan_curseforge(dir.path());

        assert_eq!(found[0].name, "ATM10");
        assert_eq!(
            found[0].loader,
            Loader {
                kind: LoaderKind::NeoForge,
                version: Some("21.1.77".into())
            }
        );
    }

    #[test]
    fn strips_the_game_version_from_curseforge_fabric_loaders() {
        let loader = curse_base_loader("fabric-0.16.9-1.21.4", "1.21.4");
        assert_eq!(loader.version.unwrap(), "0.16.9");
    }

    #[test]
    fn a_missing_database_means_no_modrinth_instances() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_modrinth(dir.path()).is_empty());
    }
}

//! Moving in from other launchers. Importing only ever reads the original,
//! so nobody loses anything by trying Waymark.

use std::fs;
use std::path::{Path, PathBuf};

use directories::BaseDirs;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
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
    /// Mojang's own Minecraft Launcher.
    Official,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalInstance {
    pub launcher: ExternalLauncher,
    pub name: String,
    pub game_dir: PathBuf,
    pub minecraft: String,
    pub loader: Loader,
    /// When it was last played there, so it keeps its place in the library.
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_played: Option<OffsetDateTime>,
    /// Time played there, which comes along.
    #[serde(default)]
    pub playtime_secs: u64,
    /// The Waymark instance already made from this one, so it isn't offered twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_as: Option<String>,
}

impl ExternalInstance {
    fn found(
        launcher: ExternalLauncher,
        name: String,
        game_dir: PathBuf,
        minecraft: String,
        loader: Loader,
    ) -> Self {
        Self {
            launcher,
            name,
            game_dir,
            minecraft,
            loader,
            last_played: None,
            playtime_secs: 0,
            imported_as: None,
        }
    }
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
    let name = cfg_value(&cfg, "name").unwrap_or_else(|| {
        dir.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    let mut found = ExternalInstance::found(launcher, name, game_dir, minecraft?, loader);
    let number = |key: &str| cfg_value(&cfg, key)?.parse::<i64>().ok();
    // Prism counts milliseconds here, and seconds for the time played.
    found.last_played = number("lastLaunchTime")
        .filter(|&millis| millis > 0)
        .and_then(|millis| OffsetDateTime::from_unix_timestamp(millis / 1000).ok());
    // Only the last session is written until there's a total.
    found.playtime_secs = number("totalTimePlayed")
        .or_else(|| number("lastTimePlayed"))
        .unwrap_or(0)
        .max(0) as u64;
    Some(found)
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
            let loader = found
                .launcher
                .loader_version
                .map_or_else(Loader::default, |loader| {
                    loader_named(&loader.kind, Some(loader.version))
                });
            Some(ExternalInstance::found(
                ExternalLauncher::AtLauncher,
                found.launcher.name,
                dir,
                found.id,
                loader,
            ))
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
            let loader = loader_named(&found.loader.loader_type, found.loader.loader_version);
            Some(ExternalInstance::found(
                ExternalLauncher::GdLauncher,
                found.loader.source_name.unwrap_or(folder),
                dir,
                found.loader.mc_version,
                loader,
            ))
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
            Some(ExternalInstance::found(
                ExternalLauncher::CurseForge,
                meta.name,
                dir,
                meta.game_version,
                loader,
            ))
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

#[derive(Deserialize)]
struct OfficialProfiles {
    #[serde(default)]
    profiles: std::collections::BTreeMap<String, OfficialProfile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialProfile {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    last_version_id: String,
    #[serde(default)]
    game_dir: Option<PathBuf>,
    #[serde(default)]
    last_used: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialVersion {
    #[serde(default)]
    inherits_from: Option<String>,
    #[serde(default)]
    downloads: Option<OfficialDownloads>,
}

#[derive(Deserialize)]
struct OfficialDownloads {
    client: Option<OfficialDownload>,
}

#[derive(Deserialize)]
struct OfficialDownload {
    url: String,
}

/// The official launcher's installations that follow the newest version.
const LATEST: [&str; 2] = ["latest-release", "latest-snapshot"];

/// The Minecraft version and loader behind one of the official launcher's version
/// ids, when Waymark can set it up itself. A custom client, or a loader Waymark
/// doesn't install, gives nothing.
fn official_version(root: &Path, id: &str) -> Option<(String, Loader)> {
    let file = root.join("versions").join(id).join(format!("{id}.json"));
    let meta: Option<OfficialVersion> = fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let parent = meta.as_ref().and_then(|meta| meta.inherits_from.clone());
    let with = |kind, version: &str| Loader {
        kind,
        version: Some(version.to_string()),
    };
    for (prefix, kind) in [
        ("fabric-loader-", LoaderKind::Fabric),
        ("quilt-loader-", LoaderKind::Quilt),
    ] {
        if let Some(rest) = id.strip_prefix(prefix) {
            // `fabric-loader-0.16.9-1.21.4`: the loader, then the game.
            let (version, minecraft) = match &parent {
                Some(parent) => (rest.strip_suffix(&format!("-{parent}"))?, parent.clone()),
                None => rest.split_once('-').map(|(v, m)| (v, m.to_string()))?,
            };
            return Some((minecraft, with(kind, version)));
        }
    }
    if let Some((minecraft, version)) = id.split_once("-forge-") {
        return Some((minecraft.to_string(), with(LoaderKind::Forge, version)));
    }
    if let Some(version) = id.strip_prefix("neoforge-") {
        return Some((parent?, with(LoaderKind::NeoForge, version)));
    }
    // A version of its own is Mojang's only when its game comes from Mojang.
    let from_mojang = meta
        .and_then(|meta| meta.downloads?.client)
        .is_some_and(|client| client.url.contains(".mojang.com/"));
    (parent.is_none() && from_mojang).then(|| (id.to_string(), Loader::default()))
}

/// Mojang's launcher keeps every installation in one file. Installations that share
/// a game folder share their worlds and mods too, so each folder comes over once,
/// on the version that was played there last.
fn scan_official(root: &Path) -> Vec<ExternalInstance> {
    let Ok(bytes) = fs::read(root.join("launcher_profiles.json")) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_slice::<OfficialProfiles>(&bytes) else {
        return Vec::new();
    };
    let mut profiles: Vec<OfficialProfile> = file.profiles.into_values().collect();
    // The dates are ISO 8601 in UTC, which sort the same as text.
    profiles.sort_by(|a, b| b.last_used.cmp(&a.last_used));
    let mut found: Vec<ExternalInstance> = Vec::new();
    for profile in profiles {
        let game_dir = profile
            .game_dir
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| root.to_path_buf());
        if !game_dir.is_dir() || found.iter().any(|other| other.game_dir == game_dir) {
            continue;
        }
        let (minecraft, loader) = if LATEST.contains(&profile.kind.as_str()) {
            (profile.kind.clone(), Loader::default())
        } else {
            match official_version(root, &profile.last_version_id) {
                Some(version) => version,
                None => continue,
            }
        };
        let name = if game_dir == root {
            "Minecraft Launcher".to_string()
        } else if profile.name.is_empty() {
            profile.last_version_id.clone()
        } else {
            profile.name.clone()
        };
        let mut external = ExternalInstance::found(
            ExternalLauncher::Official,
            name,
            game_dir,
            minecraft,
            loader,
        );
        external.last_played = profile.last_used.as_deref().and_then(|when| {
            OffsetDateTime::parse(when, &time::format_description::well_known::Rfc3339).ok()
        });
        found.push(external);
    }
    found
}

/// What stays behind when copying one of the official launcher's game folders.
/// Its main folder also holds the launcher's own files and the player's sign-in,
/// so from there only the game's own files come along.
fn left_behind(external: &ExternalInstance) -> Vec<String> {
    const GAME_FILES: [&str; 12] = [
        "mods",
        "config",
        "resourcepacks",
        "shaderpacks",
        "saves",
        "screenshots",
        "schematics",
        "texturepacks",
        "options.txt",
        "optionsof.txt",
        "optionsshaders.txt",
        "servers.dat",
    ];
    const LAUNCHER_FOLDERS: [&str; 6] = [
        "versions",
        "libraries",
        "assets",
        "runtime",
        "bin",
        "webcache2",
    ];
    if external.launcher != ExternalLauncher::Official {
        return Vec::new();
    }
    let launcher_folder = external.game_dir.join("launcher_profiles.json").is_file();
    let Ok(entries) = fs::read_dir(&external.game_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            if launcher_folder {
                !GAME_FILES.contains(&name.as_str())
            } else {
                name.starts_with("launcher_") || LAUNCHER_FOLDERS.contains(&name.as_str())
            }
        })
        .collect()
}

/// The official launcher's folder on each system.
fn official_root(base: &BaseDirs) -> PathBuf {
    if cfg!(windows) {
        base.data_dir().join(".minecraft")
    } else if cfg!(target_os = "macos") {
        base.data_dir().join("minecraft")
    } else {
        base.home_dir().join(".minecraft")
    }
}

/// The Modrinth App's own versions used one `profiles` table. Since late 2026 an
/// instance's Minecraft version and loader live in the content set it has applied,
/// and anything still installing has no files worth copying yet.
const MODRINTH_INSTANCES: &str = "
    SELECT i.path, i.name, s.game_version, s.loader, s.loader_version, i.last_played,
           COALESCE(i.submitted_time_played, 0) + COALESCE(i.recent_time_played, 0)
    FROM instances i
    JOIN instance_content_sets s ON s.id = i.applied_content_set_id
    WHERE i.install_stage = 'installed'";
const MODRINTH_PROFILES: &str = "
    SELECT path, name, game_version, mod_loader, mod_loader_version, NULL, 0 FROM profiles";

fn read_modrinth(root: &Path) -> rusqlite::Result<Vec<ExternalInstance>> {
    let database =
        Connection::open_with_flags(root.join("app.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let has_instances = database
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'instances'")?
        .exists([])?;
    let mut query = database.prepare(if has_instances {
        MODRINTH_INSTANCES
    } else {
        MODRINTH_PROFILES
    })?;
    let rows = query.query_map([], |row| {
        let path: String = row.get(0)?;
        let name: String = row.get(1)?;
        let minecraft: String = row.get(2)?;
        let loader: String = row.get(3)?;
        let version: Option<String> = row.get(4)?;
        let last_played: Option<i64> = row.get(5)?;
        let playtime: i64 = row.get(6)?;
        Ok((
            path,
            name,
            minecraft,
            loader,
            version,
            last_played,
            playtime,
        ))
    })?;
    let mut instances = Vec::new();
    for row in rows {
        let (path, name, minecraft, loader, version, last_played, playtime) = row?;
        let kind = match loader.as_str() {
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            "neoforge" => LoaderKind::NeoForge,
            "forge" => LoaderKind::Forge,
            _ => LoaderKind::Vanilla,
        };
        let mut found = ExternalInstance::found(
            ExternalLauncher::Modrinth,
            name,
            root.join("profiles").join(path),
            minecraft,
            Loader { kind, version },
        );
        found.last_played =
            last_played.and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok());
        found.playtime_secs = playtime.max(0) as u64;
        instances.push(found);
    }
    Ok(instances)
}

/// Names the Waymark instance each one was already imported as.
fn mark_imported(found: &mut [ExternalInstance], instances: &[Instance]) {
    for external in found {
        external.imported_as = instances
            .iter()
            .find(|instance| instance.imported_from.as_deref() == Some(external.game_dir.as_path()))
            .map(|instance| instance.id.clone());
    }
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
            // Copied into the store, never linked: a link would share the other
            // launcher's file, and the store marks its files read-only.
            let stored = core.store.add(source, Placement::Copy)?;
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
        found.extend(scan_official(&official_root(&base)));
        mark_imported(&mut found, &self.instances().unwrap_or_default());
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
        let minecraft = match external.minecraft.as_str() {
            "latest-release" => self.version_manifest().await?.latest.release,
            "latest-snapshot" => self.version_manifest().await?.latest.snapshot,
            version => version.to_string(),
        };
        let version_id = self
            .install_loader(
                external.loader.kind,
                &minecraft,
                external.loader.version.as_deref().unwrap_or(""),
                task,
            )
            .await?;
        let mut instance = self.register_instance(
            &external.name,
            &minecraft,
            external.loader.clone(),
            version_id,
            None,
        )?;
        // It picks up where it left off there, and isn't offered for import again.
        instance.imported_from = Some(external.game_dir.clone());
        instance.last_played = external.last_played;
        instance.playtime_secs = external.playtime_secs;
        self.save_instance(&instance)?;
        let core = self.clone();
        let game = self.paths.game(&instance.id);
        let id = instance.id.clone();
        let source = external.game_dir.clone();
        let left = left_behind(external);
        let skip: Vec<&str> = left.iter().map(String::as_str).collect();
        tokio::task::block_in_place(|| {
            copy_game_dir(&core, &source, &game, &skip, task)?;
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

    /// The Modrinth App's database since late 2026, cut down to what's read.
    #[test]
    fn reads_the_modrinth_app_s_instances_and_their_time_played() {
        let dir = tempfile::tempdir().unwrap();
        let database = Connection::open(dir.path().join("app.db")).unwrap();
        database
            .execute_batch(
                "CREATE TABLE instances (id TEXT, path TEXT, applied_content_set_id TEXT,
                     install_stage TEXT, name TEXT, last_played INTEGER,
                     submitted_time_played INTEGER, recent_time_played INTEGER);
                 CREATE TABLE instance_content_sets (id TEXT, game_version TEXT, loader TEXT,
                     loader_version TEXT);
                 INSERT INTO instances VALUES
                     ('a', 'MCSR Ranked', 'set-a', 'installed', 'MCSR Ranked', 1794429937, 25857, 360),
                     ('b', 'Half done', 'set-b', 'installing', 'Half done', NULL, 0, 0);
                 INSERT INTO instance_content_sets VALUES
                     ('set-a', '1.16.1', 'fabric', '0.19.3'),
                     ('set-b', '1.21.4', 'vanilla', NULL);",
            )
            .unwrap();
        drop(database);

        let found = scan_modrinth(dir.path());

        assert_eq!(found.len(), 1, "an instance still installing is left out");
        assert_eq!(found[0].name, "MCSR Ranked");
        assert_eq!(found[0].game_dir, dir.path().join("profiles/MCSR Ranked"));
        assert_eq!(found[0].minecraft, "1.16.1");
        assert_eq!(
            found[0].loader,
            Loader {
                kind: LoaderKind::Fabric,
                version: Some("0.19.3".into())
            }
        );
        assert_eq!(found[0].playtime_secs, 26217);
        assert_eq!(
            found[0].last_played.unwrap().unix_timestamp(),
            1_794_429_937
        );
    }

    #[test]
    fn still_reads_the_modrinth_app_s_older_profiles_table() {
        let dir = tempfile::tempdir().unwrap();
        let database = Connection::open(dir.path().join("app.db")).unwrap();
        database
            .execute_batch(
                "CREATE TABLE profiles (path TEXT, name TEXT, game_version TEXT,
                     mod_loader TEXT, mod_loader_version TEXT);
                 INSERT INTO profiles VALUES ('Pack', 'Pack', '1.21.4', 'quilt', '0.26.0');",
            )
            .unwrap();
        drop(database);

        let found = scan_modrinth(dir.path());

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].loader.kind, LoaderKind::Quilt);
        assert_eq!(found[0].playtime_secs, 0);
    }

    #[test]
    fn a_prism_instance_brings_its_time_played() {
        let dir = tempfile::tempdir().unwrap();
        let instance = dir.path().join("run");
        fs::create_dir_all(instance.join("minecraft")).unwrap();
        fs::write(
            instance.join("instance.cfg"),
            "[General]\nname=Run\nlastLaunchTime=1786398409762\nlastTimePlayed=282\n",
        )
        .unwrap();
        fs::write(
            instance.join("mmc-pack.json"),
            r#"{"components":[{"uid":"net.minecraft","version":"1.16.1"}]}"#,
        )
        .unwrap();

        let found = scan_prism(dir.path(), ExternalLauncher::Prism);

        assert_eq!(found[0].playtime_secs, 282);
        assert_eq!(
            found[0].last_played.unwrap().unix_timestamp(),
            1_786_398_409
        );
    }

    /// A `.minecraft` folder like a real one, cut down: installations that share it,
    /// one with its own folder, and a custom client Waymark can't set up.
    fn official_launcher() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let version = |id: &str, json: &str| {
            fs::create_dir_all(root.join("versions").join(id)).unwrap();
            fs::write(root.join(format!("versions/{id}/{id}.json")), json).unwrap();
        };
        version(
            "1.8",
            r#"{"id":"1.8","downloads":{"client":{"url":"https://launcher.mojang.com/v1/objects/x/client.jar"}}}"#,
        );
        version(
            "fabric-loader-0.19.5-26.3",
            r#"{"id":"fabric-loader-0.19.5-26.3","inheritsFrom":"26.3"}"#,
        );
        version(
            "Wurst MC 1.8",
            r#"{"id":"Wurst MC 1.8","inheritsFrom":"1.8"}"#,
        );
        let laracraft = root.join("LaraCraft");
        fs::create_dir_all(&laracraft).unwrap();
        fs::write(
            root.join("launcher_profiles.json"),
            serde_json::json!({ "profiles": {
                "a": { "type": "latest-release", "lastVersionId": "latest-release", "lastUsed": "2024-05-06T15:59:31.796Z" },
                "b": { "name": "Wurst MC 1.8", "type": "custom", "lastVersionId": "Wurst MC 1.8", "lastUsed": "2025-02-14T18:04:42.814Z" },
                "c": { "name": "Wurst", "type": "custom", "lastVersionId": "1.8", "lastUsed": "2024-10-20T23:47:11.645Z" },
                "d": { "name": "LaraCraft", "type": "custom", "lastVersionId": "fabric-loader-0.19.5-26.3",
                       "gameDir": laracraft, "lastUsed": "2026-10-18T15:07:32.000Z" },
            }})
            .to_string(),
        )
        .unwrap();
        dir
    }

    #[test]
    fn reads_the_official_launcher_one_game_folder_at_a_time() {
        let dir = official_launcher();

        let found = scan_official(dir.path());

        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].name, "LaraCraft");
        assert_eq!(found[0].minecraft, "26.3");
        assert_eq!(
            found[0].loader,
            Loader {
                kind: LoaderKind::Fabric,
                version: Some("0.19.5".into())
            }
        );
        // The shared folder takes the newest installation Waymark can set up: not
        // the custom client played after it, and not the older latest release.
        assert_eq!(found[1].name, "Minecraft Launcher");
        assert_eq!(found[1].game_dir, dir.path());
        assert_eq!(found[1].minecraft, "1.8");
        assert_eq!(found[1].loader, Loader::default());
        assert_eq!(
            found[1].last_played.unwrap().unix_timestamp(),
            1_729_468_031
        );
    }

    #[test]
    fn reads_other_launchers_version_names() {
        let dir = tempfile::tempdir().unwrap();
        let version = |id: &str| official_version(dir.path(), id);
        assert_eq!(
            version("quilt-loader-0.26.0-1.21.4").unwrap(),
            (
                "1.21.4".to_string(),
                Loader {
                    kind: LoaderKind::Quilt,
                    version: Some("0.26.0".into())
                }
            )
        );
        assert_eq!(
            version("1.20.1-forge-47.2.0").unwrap().1,
            Loader {
                kind: LoaderKind::Forge,
                version: Some("47.2.0".into())
            }
        );
        // Without its file nobody can say which game NeoForge or a plain id is for.
        assert!(version("neoforge-21.1.77").is_none());
        assert!(version("1.21.4-OptiFine_HD_U_J2").is_none());
    }

    #[test]
    fn the_sign_in_never_leaves_the_official_launcher_s_folder() {
        let dir = official_launcher();
        let root = dir.path();
        for name in [
            "launcher_msa_credentials_microsoft_store.bin",
            "launcher_accounts_microsoft_store.json",
            "options.txt",
        ] {
            fs::write(root.join(name), b"x").unwrap();
        }
        for folder in ["saves", "mods", "libraries", "webcache2", "logs"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        let main = scan_official(root)
            .into_iter()
            .find(|found| found.game_dir == root)
            .unwrap();

        let left = left_behind(&main);

        for kept in ["saves", "mods", "options.txt"] {
            assert!(!left.contains(&kept.to_string()), "{kept} stays behind");
        }
        for skipped in [
            "launcher_msa_credentials_microsoft_store.bin",
            "launcher_accounts_microsoft_store.json",
            "launcher_profiles.json",
            "libraries",
            "versions",
            "webcache2",
            "logs",
        ] {
            assert!(left.contains(&skipped.to_string()), "{skipped} comes along");
        }

        // A folder of its own holds the game alone, so only launcher files stay behind.
        let laracraft = root.join("LaraCraft");
        for name in ["launcher_log.txt", "journeymap", "options.txt"] {
            fs::write(laracraft.join(name), b"x").unwrap();
        }
        let own = ExternalInstance::found(
            ExternalLauncher::Official,
            "LaraCraft".into(),
            laracraft,
            "26.3".into(),
            Loader::default(),
        );
        assert_eq!(left_behind(&own), ["launcher_log.txt"]);
    }

    #[test]
    fn an_instance_already_imported_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let mut imported = core
            .register_instance("Run", "1.16.1", Loader::default(), "1.16.1".into(), None)
            .unwrap();
        imported.imported_from = Some(PathBuf::from("/prism/run/minecraft"));
        let fresh = |path: &str| {
            ExternalInstance::found(
                ExternalLauncher::Prism,
                "Run".into(),
                PathBuf::from(path),
                "1.16.1".into(),
                Loader::default(),
            )
        };
        let mut found = vec![
            fresh("/prism/run/minecraft"),
            fresh("/prism/other/minecraft"),
        ];

        mark_imported(&mut found, &[imported.clone()]);

        assert_eq!(found[0].imported_as.as_deref(), Some(imported.id.as_str()));
        assert!(found[1].imported_as.is_none());
    }
}

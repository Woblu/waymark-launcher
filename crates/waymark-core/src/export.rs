//! Turns an instance into a folder that runs as a server. Client-only mods are
//! left out, and the player accepts Minecraft's EULA themselves on first run,
//! so nothing here agrees to it on their behalf.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use walkdir::WalkDir;

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::instance::Instance;
use crate::loaders::hide_console;
use crate::lock::{LoaderKind, Lockfile, Side};
use crate::net::{Expected, Fetch};
use crate::store::Placement;

const FABRIC_INSTALLERS: &str = "https://meta.fabricmc.net/v2/versions/installer";
const FABRIC_LOADERS: &str = "https://meta.fabricmc.net/v2/versions/loader";
const QUILT_INSTALLER: &str = "https://quiltmc.org/api/v1/download-latest-installer/java-universal";
const COPIED_FOLDERS: [&str; 3] = ["config", "defaultconfigs", "kubejs"];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerExport {
    pub folder: PathBuf,
    pub mods: usize,
    /// Client-only mods that were left out.
    pub skipped: Vec<String>,
}

#[derive(Deserialize)]
struct FabricInstaller {
    version: String,
    stable: bool,
}

/// What actually starts the server, which differs by loader.
enum Launch {
    Jar(String),
    /// Modern Forge and NeoForge generate their own run scripts.
    RunScript,
}

/// The console Java next to the windowless one the game uses.
fn console_java(java: &Path) -> PathBuf {
    if cfg!(windows) {
        java.with_file_name("java.exe")
    } else {
        java.to_path_buf()
    }
}

/// Mods worth copying, and the client-only ones left behind.
pub(crate) fn split_mods(lock: &Lockfile) -> (Vec<&crate::lock::LockedMod>, Vec<String>) {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    for entry in lock.mods.iter().filter(|m| m.enabled) {
        if entry.side == Side::Client {
            skipped.push(entry.name.clone());
        } else {
            kept.push(entry);
        }
    }
    (kept, skipped)
}

fn scripts(launch: &Launch, java: &Path, memory_mb: u32) -> (String, String) {
    let folder = java.parent().unwrap_or(Path::new("")).display();
    match launch {
        Launch::Jar(jar) => (
            format!(
                "@echo off\r\ncd /d \"%~dp0\"\r\n\"{}\" -Xmx{memory_mb}M -jar {jar} nogui\r\npause\r\n",
                java.display()
            ),
            format!(
                "#!/bin/sh\ncd \"$(dirname \"$0\")\"\nexec \"{}\" -Xmx{memory_mb}M -jar {jar} nogui\n",
                java.display()
            ),
        ),
        Launch::RunScript => (
            format!(
                "@echo off\r\ncd /d \"%~dp0\"\r\nset \"PATH={folder};%PATH%\"\r\ncall \"%~dp0run.bat\" nogui\r\npause\r\n"
            ),
            format!(
                "#!/bin/sh\ncd \"$(dirname \"$0\")\"\nexport PATH=\"{folder}:$PATH\"\nexec sh run.sh nogui\n"
            ),
        ),
    }
}

fn notes(instance: &Instance, skipped: &[String]) -> String {
    let mut text = format!(
        "Waymark made this server folder from {}.\n\n\
         To start it, run start.bat on Windows or start.sh on macOS and Linux.\n\n\
         The first run stops and asks you to accept the Minecraft EULA. Read it at\n\
         https://aka.ms/MinecraftEULA, then open eula.txt and change eula=false to eula=true.\n\n\
         This folder can hold Minecraft's server files, so please don't share it with other people.\n",
        instance.name
    );
    if !skipped.is_empty() {
        text.push_str("\nThese client-only mods were left out:\n");
        for name in skipped {
            text.push_str(&format!("  {name}\n"));
        }
    }
    text
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(from)
            .map_err(|e| Error::Other(e.to_string()))?;
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), target)?;
    }
    Ok(())
}

impl Core {
    /// Builds a server folder inside `parent` and returns where it went.
    pub async fn export_server(&self, id: &str, parent: &Path) -> Result<ServerExport> {
        let instance = self.instance(id)?;
        let task = Task::start(
            &self.events,
            format!("Exporting {} as a server", instance.name),
        );
        match self.build_server(&instance, parent, &task).await {
            Ok(export) => {
                task.finish();
                Ok(export)
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    pub(crate) async fn build_server(
        &self,
        instance: &Instance,
        parent: &Path,
        task: &Task,
    ) -> Result<ServerExport> {
        let folder = parent.join(format!("{}-server", instance.id));
        if folder.exists() {
            return Err(Error::Other(format!("{} already exists", folder.display())));
        }
        fs::create_dir_all(&folder)?;

        // Identifying mods first is what tells us which are client-only.
        let _ = self.identify_mods(&instance.id).await;
        let lock = self.lockfile(&instance.id)?;

        task.progress(0, 1, "Preparing Java");
        let prepared = self.prepare_version(&instance.minecraft, task).await?;
        let java = console_java(&prepared.java);

        task.progress(0, 1, "Installing the server");
        let launch = self.install_server(instance, &folder, &java).await?;

        task.progress(0, 1, "Copying mods and configs");
        let (kept, skipped) = split_mods(&lock);
        let mods_dir = folder.join("mods");
        for entry in &kept {
            self.store
                .place(&entry.hash, &mods_dir.join(&entry.file), Placement::Copy)?;
        }
        let game = self.paths.game(&instance.id);
        for name in COPIED_FOLDERS {
            if game.join(name).is_dir() {
                copy_tree(&game.join(name), &folder.join(name))?;
            }
        }

        let memory = instance.memory_mb.unwrap_or(self.settings()?.memory_mb);
        let (bat, sh) = scripts(&launch, &java, memory);
        fs::write(folder.join("start.bat"), bat)?;
        fs::write(folder.join("start.sh"), sh)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(folder.join("start.sh"), fs::Permissions::from_mode(0o755))?;
        }
        fs::write(folder.join("README.txt"), notes(instance, &skipped))?;

        Ok(ServerExport {
            mods: kept.len(),
            skipped,
            folder,
        })
    }

    async fn install_server(
        &self,
        instance: &Instance,
        folder: &Path,
        java: &Path,
    ) -> Result<Launch> {
        let minecraft = &instance.minecraft;
        let loader_version = instance.loader.version.as_deref().unwrap_or("");
        match instance.loader.kind {
            LoaderKind::Vanilla => {
                let (meta, _) = self.resolve_version(minecraft).await?;
                let server = meta
                    .downloads
                    .as_ref()
                    .and_then(|d| d.get("server"))
                    .ok_or_else(|| Error::NotFound(format!("a server for Minecraft {minecraft}")))?;
                self.net
                    .fetch(&Fetch {
                        url: server.url.clone(),
                        dest: folder.join("server.jar"),
                        hash: server.sha1.clone().map_or(Expected::Nothing, Expected::Sha1),
                        size: server.size,
                    })
                    .await?;
                Ok(Launch::Jar("server.jar".into()))
            }
            LoaderKind::Fabric => {
                let installers: Vec<FabricInstaller> = self.net.json(FABRIC_INSTALLERS).await?;
                let installer = installers.iter().find(|i| i.stable).or(installers.first()).ok_or_else(|| Error::NotFound("a Fabric installer".into()))?;
                let url = format!("{FABRIC_LOADERS}/{minecraft}/{loader_version}/{}/server/jar", installer.version);
                self.net
                    .fetch(&Fetch { url, dest: folder.join("fabric-server.jar"), hash: Expected::Nothing, size: None })
                    .await?;
                Ok(Launch::Jar("fabric-server.jar".into()))
            }
            LoaderKind::Quilt => {
                let installer = self.paths.cache().join("installers").join("quilt-installer.jar");
                self.net
                    .fetch(&Fetch { url: QUILT_INSTALLER.to_string(), dest: installer.clone(), hash: Expected::Nothing, size: None })
                    .await?;
                let target = format!("--install-dir={}", folder.display());
                let arguments = ["install", "server", minecraft.as_str(), loader_version, "--download-server", target.as_str()];
                self.run_java(java, &installer, &arguments).await?;
                Ok(Launch::Jar("quilt-server-launch.jar".into()))
            }
            LoaderKind::LegacyFabric | LoaderKind::Babric | LoaderKind::Ornithe => {
                Err(Error::Other("this loader doesn't make server files, so install its server from its own website".into()))
            }
            LoaderKind::Forge | LoaderKind::NeoForge => {
                let url = self.loader_installer(instance.loader.kind, minecraft, loader_version).await.expect("these loaders ship installers");
                let name = url.rsplit('/').next().unwrap_or("installer.jar");
                let installer = self.paths.cache().join("installers").join(name);
                self.net
                    .fetch(&Fetch { url: url.clone(), dest: installer.clone(), hash: Expected::Nothing, size: None })
                    .await?;
                let target = folder.display().to_string();
                self.run_java(java, &installer, &["--installServer", target.as_str()]).await?;
                let _ = fs::remove_file(folder.join(format!("{name}.log")));
                if folder.join("run.bat").is_file() || folder.join("run.sh").is_file() {
                    return Ok(Launch::RunScript);
                }
                // Forge before 1.17 leaves a plain jar instead of run scripts.
                let jar = fs::read_dir(folder)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .find(|n| n.starts_with("forge-") && n.ends_with(".jar"))
                    .ok_or_else(|| Error::Other("the loader installer left no server to run".into()))?;
                Ok(Launch::Jar(jar))
            }
        }
    }

    async fn run_java(&self, java: &Path, jar: &Path, arguments: &[&str]) -> Result<()> {
        let mut command = Command::new(java);
        command
            .arg("-jar")
            .arg(jar)
            .args(arguments)
            .current_dir(self.paths.cache())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let output = command.output().await?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr);
        Err(Error::Other(format!(
            "the server installer failed: {}",
            detail.lines().last().unwrap_or("no error output")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{Loader, LockedMod, Origin, Source};

    fn locked(name: &str, side: Side, enabled: bool) -> LockedMod {
        LockedMod {
            name: name.into(),
            file: format!("{name}.jar"),
            hash: name.into(),
            size: 1,
            enabled,
            source: Source::Manual,
            origin: Origin::Manual,
            side,
            version: None,
            icon: None,
            group: None,
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        }
    }

    #[test]
    fn client_only_and_switched_off_mods_stay_out_of_a_server() {
        let mut lock = Lockfile::new("1.21.4", Loader::default());
        lock.mods = vec![
            locked("sodium", Side::Client, true),
            locked("lithium", Side::Both, true),
            locked("off", Side::Both, false),
        ];
        let (kept, skipped) = split_mods(&lock);
        assert_eq!(
            kept.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["lithium"]
        );
        assert_eq!(skipped, ["sodium"]);
    }

    #[test]
    fn start_scripts_run_the_right_jar_with_the_chosen_memory() {
        let (bat, sh) = scripts(
            &Launch::Jar("fabric-server.jar".into()),
            Path::new("/java/bin/java"),
            6144,
        );
        assert!(bat.contains("-Xmx6144M -jar fabric-server.jar nogui"));
        assert!(sh.contains("-Xmx6144M -jar fabric-server.jar nogui"));
    }

    #[test]
    fn forge_uses_its_own_run_script_with_our_java_first() {
        let (bat, sh) = scripts(&Launch::RunScript, Path::new("/java/bin/java"), 4096);
        assert!(bat.contains("call \"%~dp0run.bat\" nogui"));
        assert!(sh.contains("export PATH=\"/java/bin:$PATH\""));
    }

    #[test]
    fn the_notes_tell_the_player_to_accept_the_eula_themselves() {
        let instance = Instance {
            id: "x".into(),
            name: "X".into(),
            minecraft: "1.21.4".into(),
            loader: Loader::default(),
            version_id: "x".into(),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            last_played: None,
            playtime_secs: 0,
            memory_mb: None,
            jvm_args: Vec::new(),
            java_path: None,
            icon: None,
            pack: None,
            shared: Vec::new(),
            group: None,
            options: crate::instance::LaunchOptions::default(),
        };
        let text = notes(&instance, &["Sodium".to_string()]);
        assert!(text.contains("eula=true"));
        assert!(text.contains("  Sodium"));
    }
}

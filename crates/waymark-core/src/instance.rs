use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::external::copy_game_dir;
use crate::lock::{LOCK_FILE, Loader, LoaderKind, Lockfile};
use crate::waymarks::{Trail, WaymarkKind};

const INSTANCE_FILE: &str = "instance.toml";
/// An instance's own picture, when the player has chosen one.
pub const ICON_FILE: &str = "icon.png";
/// What `icon` holds when that picture is in use, since it isn't a web address.
pub const CUSTOM_ICON: &str = "custom";
const MAX_ICON_BYTES: u64 = 5 << 20;
/// What a copy leaves behind: it's a fresh start, not a second history.
const NOT_COPIED: [&str; 5] = [
    "logs",
    "crash-reports",
    "backups",
    "screenshots",
    "downloads",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub minecraft: String,
    pub loader: Loader,
    /// The version id that gets launched, which includes the loader.
    pub version_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_played: Option<OffsetDateTime>,
    #[serde(default)]
    pub playtime_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jvm_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<String>,
    /// The folder it's filed under in the library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, flatten)]
    pub options: LaunchOptions,
}

/// How the game is started. These only ever come from the player's own
/// settings, never from a pack, because the commands run on their computer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fullscreen: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_launch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_exit: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewInstance {
    pub name: String,
    pub minecraft: String,
    #[serde(default)]
    pub loader: LoaderKind,
    /// The newest stable loader is used when this is left out.
    #[serde(default)]
    pub loader_version: Option<String>,
}

fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "instance".to_string()
    } else {
        slug
    }
}

impl Core {
    pub fn instances(&self) -> Result<Vec<Instance>> {
        let root = self.paths.instances();
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut instances = Vec::new();
        for entry in fs::read_dir(root)? {
            let path = entry?.path().join(INSTANCE_FILE);
            match fs::read_to_string(&path)
                .map_err(Error::from)
                .and_then(|t| Ok(toml::from_str::<Instance>(&t)?))
            {
                Ok(instance) => instances.push(instance),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "skipping unreadable instance")
                }
            }
        }
        instances.sort_by_key(|i| std::cmp::Reverse(i.last_played.unwrap_or(i.created_at)));
        Ok(instances)
    }

    pub fn instance(&self, id: &str) -> Result<Instance> {
        let path = self.paths.instance(id).join(INSTANCE_FILE);
        if !path.is_file() {
            return Err(Error::NotFound(format!("instance {id}")));
        }
        Ok(toml::from_str(&fs::read_to_string(path)?)?)
    }

    pub fn save_instance(&self, instance: &Instance) -> Result<()> {
        let dir = self.paths.instance(&instance.id);
        let part = dir.join(format!("{INSTANCE_FILE}.part"));
        fs::write(&part, toml::to_string_pretty(instance)?)?;
        fs::rename(part, dir.join(INSTANCE_FILE))?;
        Ok(())
    }

    pub fn trail(&self, id: &str) -> Trail {
        Trail::new(&self.paths.instance(id), self.store.clone())
    }

    pub fn lockfile(&self, id: &str) -> Result<Lockfile> {
        Lockfile::load(&self.paths.instance(id))
    }

    pub async fn create_instance(&self, request: NewInstance) -> Result<Instance> {
        let task = Task::start(&self.events, format!("Creating {}", request.name));
        match self.build_instance(&request, &task).await {
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

    async fn build_instance(&self, request: &NewInstance, task: &Task) -> Result<Instance> {
        let loader_version = match (request.loader, &request.loader_version) {
            (LoaderKind::Vanilla, _) => None,
            (_, Some(version)) => Some(version.clone()),
            (kind, None) => {
                let versions = self.loader_versions(kind, &request.minecraft).await?;
                let best = versions.iter().find(|v| v.stable).or(versions.first());
                let best = best.ok_or_else(|| {
                    Error::NotFound(format!(
                        "{} for Minecraft {}",
                        kind.slug(),
                        request.minecraft
                    ))
                })?;
                Some(best.version.clone())
            }
        };
        let version_id = self
            .install_loader(
                request.loader,
                &request.minecraft,
                loader_version.as_deref().unwrap_or(""),
                task,
            )
            .await?;
        let loader = Loader {
            kind: request.loader,
            version: loader_version,
        };
        let instance =
            self.register_instance(&request.name, &request.minecraft, loader, version_id, None)?;
        self.trail(&instance.id)
            .mark("Instance created", WaymarkKind::Created)?;
        Ok(instance)
    }

    pub(crate) fn register_instance(
        &self,
        name: &str,
        minecraft: &str,
        loader: Loader,
        version_id: String,
        pack: Option<String>,
    ) -> Result<Instance> {
        let id = self.unused_id(name);
        let dir = self.paths.instance(&id);
        fs::create_dir_all(dir.join("game").join("mods"))?;
        let instance = Instance {
            id,
            name: name.to_string(),
            minecraft: minecraft.to_string(),
            loader: loader.clone(),
            version_id,
            created_at: OffsetDateTime::now_utc(),
            last_played: None,
            playtime_secs: 0,
            memory_mb: None,
            jvm_args: Vec::new(),
            java_path: None,
            icon: None,
            pack,
            shared: Vec::new(),
            group: None,
            options: LaunchOptions::default(),
        };
        Lockfile::new(minecraft, loader).save(&dir)?;
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Makes a separate instance with the same content. Mods and packs are
    /// shared through the store, so a copy costs almost no disk space.
    pub fn copy_instance(&self, id: &str, name: &str, worlds: bool) -> Result<Instance> {
        let source = self.instance(id)?;
        if self.game_running(id) {
            return Err(Error::Other(
                "close the game before copying this instance".into(),
            ));
        }
        let task = Task::start(&self.events, format!("Copying {}", source.name));
        match self.copy_files(&source, name, worlds, &task) {
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

    fn copy_files(
        &self,
        source: &Instance,
        name: &str,
        worlds: bool,
        task: &Task,
    ) -> Result<Instance> {
        let new_id = self.unused_id(name);
        let (from, to) = (
            self.paths.instance(&source.id),
            self.paths.instance(&new_id),
        );
        fs::create_dir_all(to.join("game").join("mods"))?;
        let mut skip = NOT_COPIED.to_vec();
        if !worlds {
            skip.push("saves");
        }
        copy_game_dir(self, &from.join("game"), &to.join("game"), &skip, task)?;
        for file in [LOCK_FILE, ICON_FILE, crate::packs::PACK_STATE] {
            if from.join(file).is_file() {
                if let Some(parent) = to.join(file).parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(from.join(file), to.join(file))?;
            }
        }
        let mut instance = source.clone();
        instance.id = new_id;
        instance.name = name.to_string();
        instance.created_at = OffsetDateTime::now_utc();
        instance.last_played = None;
        instance.playtime_secs = 0;
        instance.shared = Vec::new();
        self.save_instance(&instance)?;
        for folder in &source.shared {
            self.set_folder_shared(&instance.id, folder, true)?;
        }
        self.trail(&instance.id).mark(
            &format!("Copied from {}", source.name),
            WaymarkKind::Created,
        )?;
        self.instance(&instance.id)
    }

    pub fn set_instance_group(&self, id: &str, group: Option<&str>) -> Result<Instance> {
        let mut instance = self.instance(id)?;
        instance.group = group
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(String::from);
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Uses a picture as the instance's icon, or goes back to the default.
    pub fn set_instance_icon(&self, id: &str, picture: Option<&Path>) -> Result<Instance> {
        let mut instance = self.instance(id)?;
        let path = self.paths.instance(id).join(ICON_FILE);
        match picture {
            Some(source) => {
                if fs::metadata(source)?.len() > MAX_ICON_BYTES {
                    return Err(Error::Other(
                        "that picture is too big, so choose one under 5 MB".into(),
                    ));
                }
                fs::copy(source, path)?;
                instance.icon = Some(CUSTOM_ICON.to_string());
            }
            None => {
                let _ = fs::remove_file(path);
                instance.icon = None;
            }
        }
        self.save_instance(&instance)?;
        Ok(instance)
    }

    fn unused_id(&self, name: &str) -> String {
        let base = slugify(name);
        if !self.paths.instance(&base).exists() {
            return base;
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| !self.paths.instance(id).exists())
            .expect("ids are unbounded")
    }

    pub fn rename_instance(&self, id: &str, name: &str) -> Result<Instance> {
        let mut instance = self.instance(id)?;
        instance.name = name.to_string();
        self.save_instance(&instance)?;
        Ok(instance)
    }

    /// Moves an instance to the trash, where it can be restored for 30 days.
    pub fn delete_instance(&self, id: &str) -> Result<crate::trash::Trashed> {
        if self.game_running(id) {
            return Err(Error::Other(
                "close the game before deleting this instance".into(),
            ));
        }
        let instance = self.instance(id)?;
        self.send_to_trash(
            &self.paths.instance(id),
            crate::trash::TrashKind::Instance,
            &instance.name,
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::lock::Loader;

    fn core() -> (tempfile::TempDir, Core) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path().join("data"), crate::event::silent()).unwrap();
        (dir, core)
    }

    #[test]
    fn a_copy_is_separate_and_starts_fresh() {
        let (_dir, core) = core();
        let source = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let game = core.paths.game(&source.id);
        fs::write(game.join("mods/sodium.jar"), b"jar").unwrap();
        fs::create_dir_all(game.join("config")).unwrap();
        fs::write(game.join("config/a.toml"), b"a").unwrap();
        fs::create_dir_all(game.join("saves/Home")).unwrap();
        fs::write(game.join("saves/Home/level.dat"), b"x").unwrap();
        fs::create_dir_all(game.join("logs")).unwrap();
        fs::write(game.join("logs/latest.log"), b"log").unwrap();
        let mut original = core.instance(&source.id).unwrap();
        original.playtime_secs = 500;
        original.memory_mb = Some(8192);
        core.save_instance(&original).unwrap();

        let copy = core.copy_instance(&source.id, "Pack Copy", false).unwrap();

        assert_eq!(copy.id, "pack-copy");
        assert_eq!(copy.playtime_secs, 0);
        assert_eq!(copy.memory_mb, Some(8192));
        let copied = core.paths.game(&copy.id);
        assert!(copied.join("mods/sodium.jar").is_file());
        assert!(copied.join("config/a.toml").is_file());
        assert!(!copied.join("saves").exists());
        assert!(!copied.join("logs").exists());
        assert_eq!(core.trail(&copy.id).list().unwrap().len(), 1);

        fs::write(copied.join("config/a.toml"), b"changed").unwrap();
        assert_eq!(fs::read(game.join("config/a.toml")).unwrap(), b"a");
    }

    #[test]
    fn a_copy_can_bring_its_worlds() {
        let (_dir, core) = core();
        let source = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        fs::create_dir_all(core.paths.game(&source.id).join("saves/Home")).unwrap();
        fs::write(
            core.paths.game(&source.id).join("saves/Home/level.dat"),
            b"x",
        )
        .unwrap();
        let copy = core.copy_instance(&source.id, "With Worlds", true).unwrap();
        assert!(
            core.paths
                .game(&copy.id)
                .join("saves/Home/level.dat")
                .is_file()
        );
    }

    #[test]
    fn groups_and_icons_are_saved() {
        let (dir, core) = core();
        let instance = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        assert_eq!(
            core.set_instance_group(&instance.id, Some(" Speedruns "))
                .unwrap()
                .group
                .as_deref(),
            Some("Speedruns")
        );
        assert_eq!(
            core.set_instance_group(&instance.id, Some("  "))
                .unwrap()
                .group,
            None
        );

        let picture = dir.path().join("me.png");
        fs::write(&picture, b"png").unwrap();
        assert_eq!(
            core.set_instance_icon(&instance.id, Some(&picture))
                .unwrap()
                .icon
                .as_deref(),
            Some(CUSTOM_ICON)
        );
        assert_eq!(
            core.local_image(&format!("instance/{}", instance.id))
                .as_deref(),
            Some(&b"png"[..])
        );
        assert_eq!(
            core.set_instance_icon(&instance.id, None).unwrap().icon,
            None
        );
        assert_eq!(core.local_image(&format!("instance/{}", instance.id)), None);
    }

    #[test]
    fn launch_options_survive_a_save() {
        let (_dir, core) = core();
        let mut instance = core
            .register_instance("Pack", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        instance.options.width = Some(1280);
        instance.options.env = vec![("A".to_string(), "b".to_string())];
        instance.options.wrapper = Some("mangohud".to_string());
        core.save_instance(&instance).unwrap();
        assert_eq!(
            core.instance(&instance.id).unwrap().options,
            instance.options
        );
    }

    #[test]
    fn instance_ids_are_readable_folder_names() {
        assert_eq!(slugify("All The Mods 10"), "all-the-mods-10");
        assert_eq!(slugify("  ...  "), "instance");
        assert_eq!(slugify("Über Pack!"), "ber-pack");
    }
}

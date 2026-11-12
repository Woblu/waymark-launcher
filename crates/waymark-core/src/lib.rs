//! The launcher, with no UI attached. The desktop app and the CLI are both thin
//! layers over this crate, so everything here can be tested on its own.

pub mod atlauncher;
pub mod atomic;
pub mod auth;
pub mod bisect;
pub mod brand;
pub mod content;
pub mod crash;
pub mod curseforge;
pub mod diagnostics;
pub mod discord;
pub mod error;
pub mod event;
pub mod export;
pub mod external;
pub mod ftb;
pub mod gamelog;
#[cfg(windows)]
pub(crate) mod hidden_game;
pub mod hosting;
pub mod identify;
pub mod install;
pub mod instance;
pub mod java;
pub mod launch;
pub mod library;
pub mod loaders;
pub mod lock;
pub mod logs;
pub mod modinfo;
pub mod modrinth;
pub mod mojang;
pub mod net;
pub mod old_versions;
pub mod organize;
pub mod pack_export;
pub mod packs;
pub mod paths;
pub mod port_map;
pub mod presets;
pub mod projects;
pub mod screenshots;
pub mod server_list;
pub mod share;
pub mod shared;
pub mod shortcut;
pub mod skins;
pub mod social;
pub mod start_check;
pub mod starters;
pub mod store;
pub mod sync;
pub mod technic;
pub mod trash;
pub mod waymarks;
pub mod worlds;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

pub use error::{Error, Result};
use event::Emitter;
use net::Net;
use paths::Paths;
use store::Store;

/// A theme the player made: two colors the rest of the palette comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub base: String,
    pub accent: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub memory_mb: u32,
    /// How long a new mod release must exist before updates will offer it.
    pub release_delay_hours: u32,
    pub show_snapshots: bool,
    /// Closes the launcher's window while a game runs, which frees most of
    /// its memory for the game.
    pub close_while_playing: bool,
    /// `system`, `dark`, `light`, or the id of a built-in or custom theme.
    pub theme: String,
    pub custom_themes: Vec<CustomTheme>,
    /// A language code, or `system` to follow the computer.
    pub language: String,
    /// Checks for new versions of Waymark and installs them when it closes.
    pub auto_update: bool,
    /// Also takes pre-releases, which reach people who asked for them first.
    pub beta_updates: bool,
    /// Starts a game out of sight after an update, and undoes the update if it won't load.
    pub check_updates: bool,
    /// Starter mods by Minecraft version, plus `*` for every version.
    pub starters: BTreeMap<String, Vec<starters::Starter>>,
    /// Game options follow the player between instances.
    pub share_settings: bool,
    /// The server list does too, while settings are shared.
    pub share_servers: bool,
    /// Options the player chose to share or keep apart, over the defaults.
    pub shared_choices: BTreeMap<String, bool>,
    /// Shows the instance being played on the player's Discord profile.
    pub discord_presence: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            memory_mb: 4096,
            release_delay_hours: 48,
            show_snapshots: false,
            close_while_playing: true,
            theme: "system".to_string(),
            custom_themes: Vec::new(),
            language: "system".to_string(),
            auto_update: true,
            beta_updates: false,
            check_updates: true,
            starters: BTreeMap::new(),
            share_settings: false,
            share_servers: true,
            shared_choices: BTreeMap::new(),
            discord_presence: false,
        }
    }
}

/// Cheap to clone, so it can be handed to background tasks.
#[derive(Clone)]
pub struct Core {
    pub paths: Paths,
    pub net: Net,
    pub store: Store,
    pub events: Emitter,
    pub(crate) games: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    /// Updates being checked out of sight, with what ends each check early.
    pub(crate) checks: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    pub(crate) hosts: Arc<Mutex<HashMap<String, hosting::Host>>>,
    /// The hosted server friends can see, and what they were last told about it.
    pub(crate) open_host: Arc<Mutex<Option<hosting::OpenHost>>>,
    pub(crate) told_friends: Arc<Mutex<Option<String>>>,
    pub(crate) login: Arc<tokio::sync::Mutex<Option<social::Login>>>,
    /// The last verified Minecraft session, and a turn for anyone about to make one.
    pub(crate) recent: Arc<Mutex<Option<auth::Recent>>>,
    pub(crate) signing: Arc<tokio::sync::Mutex<()>>,
    /// Keeps that session between runs.
    pub(crate) vault: Arc<dyn auth::SessionVault>,
    /// Mojang's last answer about what the player wears, so pages can share it.
    pub(crate) profile: Arc<Mutex<Option<skins::KnownProfile>>>,
    /// Discord rich presence, started the first time there's something to show.
    pub(crate) presence: Arc<std::sync::OnceLock<discord::Presence>>,
    /// The game being shown, by instance, so turning presence on mid-game works.
    pub(crate) playing: Arc<Mutex<Option<(String, discord::Activity)>>>,
}

/// The real keychain, except in tests, which must never write to it.
fn session_vault() -> Arc<dyn auth::SessionVault> {
    if cfg!(test) {
        Arc::new(auth::MemoryVault::default())
    } else {
        Arc::new(auth::Keychain)
    }
}

impl Core {
    pub fn new(root: impl Into<PathBuf>, events: Emitter) -> Result<Self> {
        let paths = Paths::new(root);
        fs::create_dir_all(paths.root())?;
        Ok(Core {
            store: Store::new(paths.store()),
            paths,
            net: Net::new()?,
            events,
            games: Arc::default(),
            checks: Arc::default(),
            open_host: Arc::default(),
            told_friends: Arc::default(),
            hosts: Arc::default(),
            login: Arc::default(),
            recent: Arc::default(),
            signing: Arc::default(),
            vault: session_vault(),
            profile: Arc::default(),
            presence: Arc::default(),
            playing: Arc::default(),
        })
    }

    pub fn settings(&self) -> Result<Settings> {
        let path = self.paths.settings();
        match fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(settings) => Ok(settings),
                // A torn or hand-broken file must never stop anyone playing.
                // It's kept beside the new one for a look, and the defaults
                // carry on until the player changes something.
                Err(error) => {
                    let stamp = time::OffsetDateTime::now_utc().unix_timestamp();
                    let aside = path.with_extension(format!("toml.broken-{stamp}"));
                    let _ = fs::rename(&path, &aside);
                    tracing::warn!(%error, kept = %aside.display(), "settings.toml couldn't be read, using defaults");
                    Ok(Settings::default())
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        crate::atomic::write(&self.paths.settings(), toml::to_string_pretty(settings)?.as_bytes())?;
        // Turning presence on or off takes effect at once, even mid-game.
        self.update_presence();
        Ok(())
    }

    /// Deletes stored files nothing refers to any more and reports the bytes freed.
    pub fn reclaim_space(&self) -> Result<u64> {
        let mut live = HashSet::new();
        // Folders, not the instance list: an instance whose instance.toml
        // can't be read right now drops out of the list but still owns files.
        if let Ok(entries) = fs::read_dir(self.paths.instances()) {
            for entry in entries {
                let folder = entry?.path();
                if folder.is_dir() {
                    live.extend(self.folder_hashes(&folder)?);
                }
            }
        }
        live.extend(self.trash_hashes()?);
        self.purge_trash()?;
        self.store.collect_garbage(&live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_when_nothing_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), event::silent()).unwrap();
        assert_eq!(core.settings().unwrap(), Settings::default());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), event::silent()).unwrap();
        let settings = Settings {
            memory_mb: 8192,
            ..Settings::default()
        };
        core.save_settings(&settings).unwrap();
        assert_eq!(core.settings().unwrap(), settings);
    }

    #[test]
    fn broken_settings_fall_back_to_defaults_and_are_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), event::silent()).unwrap();
        fs::write(core.paths.settings(), "memory_mb = [torn").unwrap();

        assert_eq!(core.settings().unwrap(), Settings::default());
        let kept = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".broken-"));
        assert!(kept, "the broken file should be kept for a look");
    }
}

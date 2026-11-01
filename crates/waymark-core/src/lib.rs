//! The launcher, with no UI attached. The desktop app and the CLI are both thin
//! layers over this crate, so everything here can be tested on its own.

pub mod atlauncher;
pub mod auth;
pub mod bisect;
pub mod brand;
pub mod content;
pub mod crash;
pub mod curseforge;
pub mod error;
pub mod event;
pub mod export;
pub mod external;
pub mod ftb;
pub mod gamelog;
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
pub mod organize;
pub mod pack_export;
pub mod packs;
pub mod paths;
pub mod projects;
pub mod screenshots;
pub mod share;
pub mod shared;
pub mod shortcut;
pub mod skins;
pub mod social;
pub mod store;
pub mod trash;
pub mod waymarks;
pub mod worlds;

use std::collections::{HashMap, HashSet};
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
    pub(crate) hosts: Arc<Mutex<HashMap<String, hosting::Host>>>,
    pub(crate) login: Arc<tokio::sync::Mutex<Option<social::Login>>>,
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
            hosts: Arc::default(),
            login: Arc::default(),
        })
    }

    pub fn settings(&self) -> Result<Settings> {
        match fs::read_to_string(self.paths.settings()) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        fs::write(self.paths.settings(), toml::to_string_pretty(settings)?)?;
        Ok(())
    }

    /// Deletes stored files nothing refers to any more and reports the bytes freed.
    pub fn reclaim_space(&self) -> Result<u64> {
        let mut live = HashSet::new();
        for instance in self.instances()? {
            live.extend(self.trail(&instance.id).live_hashes()?);
            live.extend(
                self.lockfile(&instance.id)?
                    .everything()
                    .map(|(_, entry)| entry.hash.clone()),
            );
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
}

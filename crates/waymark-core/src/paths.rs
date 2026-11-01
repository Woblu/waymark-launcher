use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::brand::DATA_DIR_NAME;
use crate::error::{Error, Result};

/// The layout of the data directory. Game versions, libraries and assets are
/// shared by every instance. `versions/` and `libraries/` deliberately match a
/// vanilla `.minecraft`, because the Forge and NeoForge installers write there.
#[derive(Clone, Debug)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Paths { root: root.into() }
    }

    pub fn default_root() -> Result<PathBuf> {
        let base = BaseDirs::new().ok_or_else(|| Error::Other("no home directory".into()))?;
        Ok(base.data_dir().join(DATA_DIR_NAME))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn store(&self) -> PathBuf {
        self.root.join("store")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn icons(&self) -> PathBuf {
        self.root.join("cache").join("icons")
    }
    pub fn trash(&self) -> PathBuf {
        self.root.join("trash")
    }
    pub fn skins(&self) -> PathBuf {
        self.root.join("skins")
    }
    pub fn shared(&self, folder: &str) -> PathBuf {
        self.root.join("shared").join(folder)
    }
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    pub fn java(&self) -> PathBuf {
        self.root.join("java")
    }
    pub fn natives(&self, version: &str) -> PathBuf {
        self.root.join("natives").join(version)
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn accounts(&self) -> PathBuf {
        self.root.join("accounts.json")
    }
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.toml")
    }

    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.root.join("versions").join(id)
    }
    pub fn version_json(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.json"))
    }
    pub fn version_jar(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.jar"))
    }

    pub fn instance(&self, id: &str) -> PathBuf {
        self.instances().join(id)
    }
    pub fn game(&self, id: &str) -> PathBuf {
        self.instance(id).join("game")
    }
}

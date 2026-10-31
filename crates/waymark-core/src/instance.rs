use std::fs;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::Core;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::lock::{Loader, LoaderKind, Lockfile};
use crate::waymarks::{Trail, WaymarkKind};

const INSTANCE_FILE: &str = "instance.toml";

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
    #[serde(default, with = "time::serde::rfc3339::option", skip_serializing_if = "Option::is_none")]
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
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    if slug.is_empty() { "instance".to_string() } else { slug }
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
            match fs::read_to_string(&path).map_err(Error::from).and_then(|t| Ok(toml::from_str::<Instance>(&t)?)) {
                Ok(instance) => instances.push(instance),
                Err(error) => tracing::warn!(path = %path.display(), %error, "skipping unreadable instance"),
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
                let best = best.ok_or_else(|| Error::NotFound(format!("{} for Minecraft {}", kind.slug(), request.minecraft)))?;
                Some(best.version.clone())
            }
        };
        let version_id = self
            .install_loader(request.loader, &request.minecraft, loader_version.as_deref().unwrap_or(""), task)
            .await?;
        let loader = Loader { kind: request.loader, version: loader_version };
        let instance = self.register_instance(&request.name, &request.minecraft, loader, version_id, None)?;
        self.trail(&instance.id).mark("Instance created", WaymarkKind::Created)?;
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
        };
        Lockfile::new(minecraft, loader).save(&dir)?;
        self.save_instance(&instance)?;
        Ok(instance)
    }

    fn unused_id(&self, name: &str) -> String {
        let base = slugify(name);
        if !self.paths.instance(&base).exists() {
            return base;
        }
        (2..).map(|n| format!("{base}-{n}")).find(|id| !self.paths.instance(id).exists()).expect("ids are unbounded")
    }

    pub fn rename_instance(&self, id: &str, name: &str) -> Result<Instance> {
        let mut instance = self.instance(id)?;
        instance.name = name.to_string();
        self.save_instance(&instance)?;
        Ok(instance)
    }

    pub fn delete_instance(&self, id: &str) -> Result<()> {
        if self.game_running(id) {
            return Err(Error::Other("close the game before deleting this instance".into()));
        }
        self.instance(id)?;
        fs::remove_dir_all(self.paths.instance(id))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_ids_are_readable_folder_names() {
        assert_eq!(slugify("All The Mods 10"), "all-the-mods-10");
        assert_eq!(slugify("  ...  "), "instance");
        assert_eq!(slugify("Über Pack!"), "ber-pack");
    }
}

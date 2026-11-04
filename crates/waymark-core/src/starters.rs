//! Starter mods: the player's own lists of mods, resource packs and shaders,
//! added to every new instance made on a Minecraft version. A list names store
//! projects and never a file, so the build for the new instance's loader is
//! worked out when it's made, and anything without one is left out.

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::content::{ContentKind, ContentSource, Planned, installed_keys};
use crate::error::{Error, Result};
use crate::event::Task;
use crate::instance::Instance;
use crate::lock::LoaderKind;
use crate::waymarks::WaymarkKind;

/// The list added on every version, alongside that version's own.
pub const EVERY_VERSION: &str = "*";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Starter {
    pub source: ContentSource,
    pub project: String,
    pub kind: ContentKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub title: String,
    /// Left out when the project has no version for this instance, which is
    /// expected for a list shared between loaders. Anything else says why.
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StarterResult {
    pub added: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// What a new instance on `minecraft` gets: its version's list, then the list for
/// every version, each project once.
pub fn starters_for(
    lists: &std::collections::BTreeMap<String, Vec<Starter>>,
    minecraft: &str,
) -> Vec<Starter> {
    let mut chosen: Vec<Starter> = Vec::new();
    for key in [minecraft, EVERY_VERSION] {
        for starter in lists.get(key).into_iter().flatten() {
            let repeat = chosen
                .iter()
                .any(|other| other.source == starter.source && other.project == starter.project);
            if !repeat {
                chosen.push(starter.clone());
            }
        }
    }
    chosen
}

/// Whether a starter can go into an instance with this loader at all. Vanilla
/// has no mods and nothing to run shaders with, so a list shared between
/// instances leaves those out there without asking a store.
pub fn fits(kind: ContentKind, loader: LoaderKind) -> bool {
    match kind {
        ContentKind::ResourcePack => true,
        ContentKind::Mod | ContentKind::Shader => loader != LoaderKind::Vanilla,
        ContentKind::Modpack | ContentKind::DataPack => false,
    }
}

impl Core {
    /// Adds the starter mods for an instance's version, as one waymark.
    pub async fn add_starters(&self, id: &str) -> Result<StarterResult> {
        let instance = self.instance(id)?;
        let wanted: Vec<Starter> = starters_for(&self.settings()?.starters, &instance.minecraft)
            .into_iter()
            .filter(|starter| fits(starter.kind, instance.loader.kind))
            .collect();
        if wanted.is_empty() {
            return Ok(StarterResult::default());
        }
        let task = Task::start(
            &self.events,
            format!("Adding starter mods to {}", instance.name),
        );
        match self.install_starters(&instance, &wanted, &task).await {
            Ok(result) => {
                task.finish();
                Ok(result)
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    async fn install_starters(
        &self,
        instance: &Instance,
        wanted: &[Starter],
        task: &Task,
    ) -> Result<StarterResult> {
        let lock = self.lockfile(&instance.id)?;
        let mut already = installed_keys(&lock, ContentKind::Mod, None);
        let mut planned: Vec<Planned> = Vec::new();
        let mut result = StarterResult::default();
        for (done, starter) in wanted.iter().enumerate() {
            task.progress(done as u64, wanted.len() as u64, "Working out dependencies");
            let plan = match starter.source {
                ContentSource::Modrinth => {
                    self.plan_modrinth(instance, &already, &starter.project, starter.kind, None)
                        .await
                }
                ContentSource::CurseForge => {
                    self.plan_curseforge(instance, &already, &starter.project, starter.kind, None)
                        .await
                }
                ContentSource::Ftb | ContentSource::AtLauncher | ContentSource::Technic => {
                    Err(Error::Other("this store only has modpacks".into()))
                }
            };
            match plan {
                // Already there, from an earlier list or as another starter's dependency.
                Ok(found) if found.is_empty() => {}
                Ok(found) => {
                    already.extend(found.iter().map(|p| p.project_key.clone()));
                    planned.extend(found);
                    result.added.push(starter.title.clone());
                }
                Err(Error::NotFound(_)) => result.skipped.push(Skipped {
                    title: starter.title.clone(),
                    reason: None,
                }),
                Err(error) => result.skipped.push(Skipped {
                    title: starter.title.clone(),
                    reason: Some(error.to_string()),
                }),
            }
        }
        if planned.is_empty() {
            return Ok(result);
        }
        self.add_planned(instance, lock, &planned, None, task)
            .await?;
        let label = match result.added.as_slice() {
            [one] => format!("Added the starter mod {one}"),
            many => format!("Added {} starter mods", many.len()),
        };
        self.trail(&instance.id)
            .mark(&label, WaymarkKind::Install)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn starter(project: &str, kind: ContentKind) -> Starter {
        Starter {
            source: ContentSource::Modrinth,
            project: project.into(),
            kind,
            title: project.into(),
            icon: None,
        }
    }

    #[test]
    fn a_version_gets_its_own_list_then_the_shared_one_without_repeats() {
        let lists = BTreeMap::from([
            (
                "1.20.1".to_string(),
                vec![
                    starter("sodium", ContentKind::Mod),
                    starter("modmenu", ContentKind::Mod),
                ],
            ),
            (
                EVERY_VERSION.to_string(),
                vec![
                    starter("modmenu", ContentKind::Mod),
                    starter("fresh", ContentKind::ResourcePack),
                ],
            ),
        ]);
        let names = |version: &str| {
            starters_for(&lists, version)
                .into_iter()
                .map(|s| s.project)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("1.20.1"), ["sodium", "modmenu", "fresh"]);
        assert_eq!(names("1.21.4"), ["modmenu", "fresh"]);
        assert!(starters_for(&BTreeMap::new(), "1.20.1").is_empty());
    }

    #[test]
    fn vanilla_takes_resource_packs_but_not_mods_or_shaders() {
        assert!(!fits(ContentKind::Mod, LoaderKind::Vanilla));
        assert!(!fits(ContentKind::Shader, LoaderKind::Vanilla));
        assert!(fits(ContentKind::ResourcePack, LoaderKind::Vanilla));
        assert!(fits(ContentKind::Mod, LoaderKind::Forge));
        assert!(fits(ContentKind::Shader, LoaderKind::Fabric));
        assert!(!fits(ContentKind::Modpack, LoaderKind::Fabric));
        assert!(!fits(ContentKind::DataPack, LoaderKind::Fabric));
    }

    #[test]
    fn lists_survive_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let mut settings = core.settings().unwrap();
        settings.starters.insert(
            "1.20.1".into(),
            vec![Starter {
                icon: Some("https://cdn.modrinth.com/icon.png".into()),
                ..starter("AANobbMI", ContentKind::Mod)
            }],
        );
        settings.starters.insert(
            EVERY_VERSION.into(),
            vec![starter("fresh", ContentKind::ResourcePack)],
        );
        core.save_settings(&settings).unwrap();
        assert_eq!(core.settings().unwrap(), settings);
    }

    #[tokio::test]
    async fn an_instance_with_no_list_gets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance(
                "Plain",
                "1.20.1",
                crate::lock::Loader::default(),
                "1.20.1".into(),
                None,
            )
            .unwrap();
        assert_eq!(
            core.add_starters(&instance.id).await.unwrap(),
            StarterResult::default()
        );
    }

    #[tokio::test]
    async fn vanilla_leaves_out_mods_without_asking_a_store() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let mut settings = core.settings().unwrap();
        settings.starters.insert(
            "1.20.1".into(),
            vec![
                starter("sodium", ContentKind::Mod),
                starter("iris-pack", ContentKind::Shader),
            ],
        );
        core.save_settings(&settings).unwrap();
        let instance = core
            .register_instance(
                "Plain",
                "1.20.1",
                crate::lock::Loader::default(),
                "1.20.1".into(),
                None,
            )
            .unwrap();
        assert_eq!(
            core.add_starters(&instance.id).await.unwrap(),
            StarterResult::default()
        );
        assert!(core.lockfile(&instance.id).unwrap().mods.is_empty());
    }
}

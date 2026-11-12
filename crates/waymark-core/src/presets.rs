//! Ready-made setups a player picks by what they want to play. Which setups there
//! are, and the words that describe them, live in the app. This makes one: a new
//! instance, then the newest build of each project that fits it.
//!
//! Speedrunning setups use the exact jars speedrun.com allows instead of a store's
//! builds. They come from the list the speedrunning community's own tools read
//! (tildejustin/mcsr-meta), and each one is checked against its SHA-512 hash.

use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Core;
use crate::content::ContentKind;
use crate::error::{Error, Result};
use crate::event::Task;
use crate::instance::{Instance, NewInstance};
use crate::lock::LoaderKind;
use crate::net::{Expected, Fetch};
use crate::starters::{Skipped, Starter};

/// The branch is named after the list's format, so a new format can't break this one.
const LEGAL_MODS: &str =
    "https://raw.githubusercontent.com/tildejustin/mcsr-meta/schema-7/mods.json";
/// Where the list keeps its jars. One from anywhere else is never fetched, even
/// with a matching hash, and OptiFine's download pages aren't jars at all.
const LEGAL_HOSTS: [&str; 3] = [
    "https://github.com/Minecraft-Java-Edition-Speedrunning/legal-mods/",
    "https://github.com/tildejustin/mc_af-legal-mods/",
    "https://cdn.modrinth.com/data/",
];

/// The mods a working MCSR Ranked instance runs with. Ranked refuses to start
/// with any mod it doesn't accept, so it gets these and nothing more.
const RANKED_MODS: [&str; 8] = [
    "speedrunigt",
    "speedrunapi",
    "atum",
    "sodium",
    "lithium",
    "starlight",
    "lazydfu",
    "krypton",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speedrun {
    /// Random seed: a new world every run.
    Rsg,
    /// Set seed: the same world every run.
    Ssg,
    /// MCSR Ranked's races, which need its own mod as well.
    Ranked,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetRequest {
    pub name: String,
    pub minecraft: String,
    pub loader: LoaderKind,
    /// Store projects, each added in its newest build for the instance.
    #[serde(default)]
    pub items: Vec<Starter>,
    #[serde(default)]
    pub speedrun: Option<Speedrun>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetResult {
    pub instance: Instance,
    pub added: Vec<String>,
    /// Left out because nothing fits this version, or for the reason given.
    pub skipped: Vec<Skipped>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LegalList {
    mods: Vec<LegalMod>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LegalMod {
    modid: String,
    name: String,
    #[serde(default)]
    traits: Vec<String>,
    #[serde(default)]
    incompatibilities: Vec<String>,
    #[serde(default)]
    recommended: Option<bool>,
    #[serde(default)]
    obsolete: Option<bool>,
    versions: Vec<LegalVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LegalVersion {
    target_version: Vec<String>,
    url: String,
    sha512: String,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    recommended: Option<bool>,
    #[serde(default)]
    obsolete: Option<bool>,
}

/// One allowed jar, ready to fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LegalJar {
    modid: String,
    name: String,
    url: String,
    sha512: String,
    file: String,
}

impl LegalMod {
    /// Its build for this version, when it has one that's still current.
    fn build_for(&self, minecraft: &str) -> Option<&LegalVersion> {
        self.versions.iter().find(|version| {
            version.target_version.iter().any(|v| v == minecraft) && version.obsolete != Some(true)
        })
    }

    fn jar(&self, version: &LegalVersion) -> Option<LegalJar> {
        // A `..` would climb out of that repository once the address is resolved.
        let known = LEGAL_HOSTS.iter().any(|host| version.url.starts_with(host));
        if !known || version.url.contains("..") {
            return None;
        }
        let file = version.url.rsplit('/').next()?.to_string();
        let plain = !file.is_empty()
            && !file.contains("..")
            && !file.contains('\\')
            && file.ends_with(".jar");
        plain.then(|| LegalJar {
            modid: self.modid.clone(),
            name: self.name.clone(),
            url: version.url.clone(),
            sha512: version.sha512.to_ascii_lowercase(),
            file,
        })
    }
}

/// What the community's tools install for this kind of run: every recommended mod
/// that's current for the version, without the ones meant for the other kind of
/// run, another system or accessibility, and with whatever those need.
fn legal_mods(list: &LegalList, minecraft: &str, run: Speedrun, mac: bool) -> Vec<LegalJar> {
    let fits = |item: &LegalMod| {
        item.traits.iter().all(|t| match t.as_str() {
            "rsg-only" => run != Speedrun::Ssg,
            "ssg-only" => run == Speedrun::Ssg,
            "mac-only" => mac,
            _ => false,
        })
    };
    let wanted = |item: &LegalMod| match run {
        Speedrun::Ranked => {
            RANKED_MODS.contains(&item.modid.as_str()) || mac && item.modid == "sodiummac"
        }
        _ => item.recommended != Some(false) && item.obsolete != Some(true),
    };
    let mut chosen: Vec<&LegalMod> = list
        .mods
        .iter()
        .filter(|item| fits(item) && wanted(item))
        .filter(|item| {
            item.build_for(minecraft)
                .is_some_and(|build| build.recommended != Some(false))
        })
        .collect();
    // A Mac build stands in for the mod it's named after.
    if mac {
        let replaced: Vec<String> = chosen
            .iter()
            .filter_map(|item| item.modid.strip_suffix("mac").map(str::to_string))
            .collect();
        chosen.retain(|item| !replaced.contains(&item.modid));
    }
    // Whatever the chosen ones need comes along, recommended or not.
    let mut index = 0;
    while index < chosen.len() {
        let needs = chosen[index]
            .build_for(minecraft)
            .map(|build| build.dependencies.clone())
            .unwrap_or_default();
        for need in needs {
            if chosen.iter().any(|item| item.modid == need) {
                continue;
            }
            if let Some(found) = list
                .mods
                .iter()
                .find(|item| item.modid == need && item.build_for(minecraft).is_some())
            {
                chosen.push(found);
            }
        }
        index += 1;
    }
    // Two that can't run together: the one listed first stays.
    let mut kept: Vec<&LegalMod> = Vec::new();
    for item in chosen {
        let clash = kept.iter().any(|other| {
            other.incompatibilities.contains(&item.modid)
                || item.incompatibilities.contains(&other.modid)
        });
        if !clash {
            kept.push(item);
        }
    }
    kept.iter()
        .filter_map(|item| item.jar(item.build_for(minecraft)?))
        .collect()
}

impl Core {
    /// Releases every one of these projects has a build for with this loader,
    /// newest first. A preset offers these, and starts on the first.
    pub async fn preset_versions(
        &self,
        loader: LoaderKind,
        projects: &[String],
    ) -> Result<Vec<String>> {
        let manifest = self.version_manifest().await?;
        let found = crate::modrinth::projects_in_full(&self.net, projects).await?;
        if found.len() < projects.len() {
            return Err(Error::NotFound("one of this preset's projects".into()));
        }
        let fits = |version: &str| {
            found.iter().all(|project| {
                project.game_versions.iter().any(|v| v == version)
                    && project.loaders.iter().any(|l| l == loader.slug())
            })
        };
        Ok(manifest
            .versions
            .into_iter()
            .filter(|entry| entry.kind == "release" && fits(&entry.id))
            .map(|entry| entry.id)
            .collect())
    }

    /// Makes a new instance from a preset, as one task.
    pub async fn install_preset(&self, request: PresetRequest) -> Result<PresetResult> {
        let instance = self
            .create_instance(NewInstance {
                name: request.name.clone(),
                minecraft: request.minecraft.clone(),
                loader: request.loader,
                loader_version: None,
            })
            .await?;
        let task = Task::start(&self.events, format!("Setting up {}", instance.name));
        match self.fill_preset(&instance, &request, &task).await {
            Ok((added, skipped)) => {
                task.finish();
                Ok(PresetResult {
                    instance: self.instance(&instance.id)?,
                    added,
                    skipped,
                })
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    async fn fill_preset(
        &self,
        instance: &Instance,
        request: &PresetRequest,
        task: &Task,
    ) -> Result<(Vec<String>, Vec<Skipped>)> {
        let mut added = Vec::new();
        let mut skipped = Vec::new();
        if let Some(run) = request.speedrun {
            added.extend(self.add_legal_mods(instance, run, task).await?);
        }
        if !request.items.is_empty() {
            let result = self
                .install_starters(instance, &request.items, Some("Set up from a preset"), task)
                .await?;
            added.extend(result.added);
            skipped.extend(result.skipped);
        }
        Ok((added, skipped))
    }

    /// Fetches the allowed jars for this run, checks each against its hash, and
    /// adds them all as one waymark.
    async fn add_legal_mods(
        &self,
        instance: &Instance,
        run: Speedrun,
        task: &Task,
    ) -> Result<Vec<String>> {
        task.progress(0, 1, "Reading speedrun.com's list of allowed mods");
        let list: LegalList = self
            .net
            .cached_json(
                LEGAL_MODS,
                &self.paths.cache().join("legal-mods.json"),
                Duration::from_secs(3600),
            )
            .await?;
        let jars = legal_mods(&list, &instance.minecraft, run, cfg!(target_os = "macos"));
        if jars.is_empty() {
            return Err(Error::Other(format!(
                "speedrun.com doesn't list any mods for Minecraft {}",
                instance.minecraft
            )));
        }
        let folder = self.paths.cache().join("legal-mods");
        let mut paths = Vec::new();
        for (done, jar) in jars.iter().enumerate() {
            task.progress(done as u64, jars.len() as u64, "Downloading allowed mods");
            // Named by hash as well, so two builds with one file name never meet.
            let dest = folder.join(&jar.sha512[..16]).join(&jar.file);
            self.net
                .fetch(&Fetch {
                    url: jar.url.clone(),
                    dest: dest.clone(),
                    hash: Expected::Sha512(jar.sha512.clone()),
                    size: None,
                })
                .await?;
            paths.push(dest);
        }
        let id = instance.id.clone();
        tokio::task::block_in_place(|| self.add_local_files(&id, ContentKind::Mod, &paths))?;
        let names: BTreeSet<String> = jars.into_iter().map(|jar| jar.name).collect();
        Ok(names.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(target: &str, file: &str, deps: &[&str]) -> LegalVersion {
        LegalVersion {
            target_version: vec![target.into()],
            url: format!("{}raw/abc/legal-mods/{file}", LEGAL_HOSTS[0]),
            sha512: "AB".repeat(64),
            dependencies: deps.iter().map(|d| d.to_string()).collect(),
            recommended: None,
            obsolete: None,
        }
    }

    fn item(id: &str, traits: &[&str], versions: Vec<LegalVersion>) -> LegalMod {
        LegalMod {
            modid: id.into(),
            name: id.into(),
            traits: traits.iter().map(|t| t.to_string()).collect(),
            incompatibilities: Vec::new(),
            recommended: None,
            obsolete: None,
            versions,
        }
    }

    /// Cut down from the real list for 1.16.1.
    fn list() -> LegalList {
        let mut dynamicmenufps = item(
            "dynamicmenufps",
            &[],
            vec![version("1.16.1", "dmf.jar", &[])],
        );
        dynamicmenufps.recommended = Some(false);
        dynamicmenufps.incompatibilities = vec!["sleepbackground".into()];
        let mut phosphor = item(
            "phosphor",
            &[],
            vec![version("1.16.1", "phosphor.jar", &[])],
        );
        phosphor.obsolete = Some(true);
        let mut worldpreview = item(
            "worldpreview",
            &["rsg-only"],
            vec![version("1.16.1", "wp.jar", &["speedrunapi"])],
        );
        worldpreview.incompatibilities = vec!["setspawnmod".into()];
        let mut hermes_core = item("hermes-core", &[], vec![version("1.16.1", "hc.jar", &[])]);
        hermes_core.versions[0].recommended = Some(false);
        LegalList {
            mods: vec![
                item(
                    "speedrunigt",
                    &[],
                    vec![version("1.16.1", "SpeedRunIGT-16.0+1.16.1.jar", &[])],
                ),
                item(
                    "speedrunapi",
                    &[],
                    vec![version("1.16.1", "speedrunapi.jar", &[])],
                ),
                item(
                    "atum",
                    &[],
                    vec![version("1.16.1", "atum.jar", &["speedrunapi"])],
                ),
                item(
                    "sodium",
                    &[],
                    vec![version("1.16.1", "sodium.jar", &["speedrunapi"])],
                ),
                item(
                    "sodiummac",
                    &["mac-only"],
                    vec![version("1.16.1", "sodiummac.jar", &["speedrunapi"])],
                ),
                item("lithium", &[], vec![version("1.16.1", "lithium.jar", &[])]),
                item(
                    "sleepbackground",
                    &[],
                    vec![version("1.16.1", "sb.jar", &["speedrunapi"])],
                ),
                item(
                    "setspawnmod",
                    &["ssg-only"],
                    vec![version("1.16.1", "ssm.jar", &[])],
                ),
                item(
                    "extra-options",
                    &["accessibility"],
                    vec![version("1.16.1", "eo.jar", &[])],
                ),
                item("only-newer", &[], vec![version("1.21.4", "new.jar", &[])]),
                worldpreview,
                dynamicmenufps,
                phosphor,
                hermes_core,
            ],
        }
    }

    fn ids(jars: &[LegalJar]) -> Vec<&str> {
        let mut ids: Vec<&str> = jars.iter().map(|jar| jar.modid.as_str()).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn random_seed_gets_every_current_mod_meant_for_it() {
        let jars = legal_mods(&list(), "1.16.1", Speedrun::Rsg, false);
        assert_eq!(
            ids(&jars),
            [
                "atum",
                "lithium",
                "sleepbackground",
                "sodium",
                "speedrunapi",
                "speedrunigt",
                "worldpreview"
            ]
        );
        let igt = jars.iter().find(|jar| jar.modid == "speedrunigt").unwrap();
        assert_eq!(igt.file, "SpeedRunIGT-16.0+1.16.1.jar");
        assert_eq!(igt.sha512, "ab".repeat(64));
    }

    #[test]
    fn set_seed_swaps_the_random_seed_tools_for_its_own() {
        let jars = legal_mods(&list(), "1.16.1", Speedrun::Ssg, false);
        let found = ids(&jars);
        assert!(found.contains(&"setspawnmod"));
        assert!(!found.contains(&"worldpreview"));
    }

    #[test]
    fn ranked_gets_only_what_it_accepts() {
        let jars = legal_mods(&list(), "1.16.1", Speedrun::Ranked, false);
        assert_eq!(
            ids(&jars),
            ["atum", "lithium", "sodium", "speedrunapi", "speedrunigt"]
        );
    }

    #[test]
    fn a_mac_gets_the_mac_build_in_place_of_the_other() {
        let jars = legal_mods(&list(), "1.16.1", Speedrun::Rsg, true);
        let found = ids(&jars);
        assert!(found.contains(&"sodiummac"));
        assert!(!found.contains(&"sodium"));
    }

    #[test]
    fn a_dependency_comes_along_even_when_not_chosen_on_its_own() {
        let mut list = list();
        list.mods.retain(|item| item.modid != "speedrunapi");
        let mut api = item("speedrunapi", &[], vec![version("1.16.1", "api.jar", &[])]);
        api.recommended = Some(false);
        list.mods.push(api);
        let jars = legal_mods(&list, "1.16.1", Speedrun::Rsg, false);
        assert!(ids(&jars).contains(&"speedrunapi"));
    }

    #[test]
    fn a_jar_from_anywhere_else_is_never_fetched() {
        let fetched = |url: &str| {
            let mut list = list();
            list.mods[0].versions[0].url = url.into();
            ids(&legal_mods(&list, "1.16.1", Speedrun::Rsg, false)).contains(&"speedrunigt")
        };
        assert!(fetched(
            "https://cdn.modrinth.com/data/hvFnDODi/versions/0.1.2/lazydfu-0.1.2.jar"
        ));
        assert!(!fetched("https://example.com/SpeedRunIGT.jar"));
        assert!(!fetched(
            "https://optifine.net/download?f=OptiFine_1.16.1_HD_U_G1.jar"
        ));
        assert!(!fetched(&format!("{}raw/abc/../evil.jar", LEGAL_HOSTS[0])));
    }

    /// The app's presets against the real store: each one's core projects share
    /// at least one release. Ignored by default because it needs the network.
    #[tokio::test]
    #[ignore]
    async fn every_preset_in_the_app_has_a_version() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let presets: [(LoaderKind, &[&str]); 7] = [
            (LoaderKind::Fabric, &["AANobbMI", "gvQqBUqZ", "uXXizFIs"]),
            (LoaderKind::Fabric, &["p2rxzX0q", "AANobbMI"]),
            (LoaderKind::Fabric, &["AANobbMI", "YL57xq9U"]),
            (LoaderKind::Fabric, &["mOgUt4GM"]),
            (LoaderKind::Fabric, &["bEpr0Arc"]),
            (LoaderKind::Fabric, &["N6n5dqoA"]),
            (LoaderKind::Fabric, &["AANobbMI"]),
        ];
        for (loader, projects) in presets {
            let ids: Vec<String> = projects.iter().map(|id| id.to_string()).collect();
            let versions = core.preset_versions(loader, &ids).await.unwrap();
            println!("{projects:?}: {:?}", &versions[..versions.len().min(3)]);
            assert!(!versions.is_empty(), "{projects:?}");
        }
    }

    /// A store preset made for real: Performance's Faster on the newest release.
    /// Ignored by default because it needs the network.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn a_store_preset_makes_an_instance_with_its_mods() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let item = |project: &str, title: &str| Starter {
            source: crate::content::ContentSource::Modrinth,
            project: project.into(),
            kind: ContentKind::Mod,
            title: title.into(),
            icon: None,
        };
        let core_ids = ["AANobbMI".to_string(), "gvQqBUqZ".into(), "uXXizFIs".into()];
        let newest = core
            .preset_versions(LoaderKind::Fabric, &core_ids)
            .await
            .unwrap()[0]
            .clone();
        let result = core
            .install_preset(PresetRequest {
                name: "Faster".into(),
                minecraft: newest.clone(),
                loader: LoaderKind::Fabric,
                items: vec![
                    item("AANobbMI", "Sodium"),
                    item("gvQqBUqZ", "Lithium"),
                    item("uXXizFIs", "FerriteCore"),
                    item("nmDcB62a", "ModernFix"),
                ],
                speedrun: None,
            })
            .await
            .unwrap();
        println!(
            "{newest}: added {:?}, skipped {:?}",
            result.added, result.skipped
        );
        for name in ["Sodium", "Lithium", "FerriteCore"] {
            assert!(result.added.iter().any(|added| added == name), "{name}");
        }
        let lock = core.lockfile(&result.instance.id).unwrap();
        assert!(lock.mods.len() >= 3);
    }

    /// The real list, as the tools read it today. Ignored by default because it
    /// needs the network; run it when the list's format might have changed.
    #[tokio::test]
    #[ignore]
    async fn the_real_list_still_reads() {
        let list: LegalList = reqwest::get(LEGAL_MODS)
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let jars = legal_mods(&list, "1.16.1", Speedrun::Rsg, false);
        assert!(ids(&jars).contains(&"speedrunigt"), "{:?}", ids(&jars));
    }
}

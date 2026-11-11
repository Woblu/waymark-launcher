//! Every old build of Minecraft, from the first RubyDung to 1.5.2, including the ones
//! Mojang doesn't list. The table is made by `tools/old-versions.mjs` from BetaCraft's
//! lists, and most of the recovered jars are Omniarchive's. Waymark hosts none of them.
//!
//! Every file is pinned by hash in the table, so a server can't change what a player runs.
//! The builds run through LegacyFix, BetaCraft's own patches, which bring back skins,
//! sound, sign-in and window resizing. It only speaks to Mojang about the player's session,
//! and the table sets it to keep Classic's saves local and to ask no outside server for skins.
//!
//! Mojang's own releases, 1.0 to 1.5.2, launch the way Mojang ships them unless the player
//! turns LegacyFix on for that instance. Speedrun leaderboards accept runs on them and
//! don't allow a Java agent or a changed game, which is how LegacyFix works.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::mojang::{
    Argument, Arguments, AssetIndexRef, Download, JavaVersion, Library, Logging, VersionEntry,
    VersionMeta,
};

const TABLE: &str = include_str!("../data/old-versions.json");
/// Keeps these apart from Mojang's versions of the same name, which Forge and the
/// other loaders build on and which they expect to find unpatched.
const PROFILE_SUFFIX: &str = "-old";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Template {
    main_class: String,
    arguments: Arguments,
    libraries: Vec<Library>,
    java_version: JavaVersion,
    #[serde(default)]
    logging: Option<Logging>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OldVersion {
    pub id: String,
    pub kind: String,
    pub era: String,
    pub released: String,
    template: usize,
    assets: String,
    client: Download,
    /// Arguments only this build needs.
    #[serde(default)]
    extra: Vec<String>,
    #[serde(default)]
    jvm: Vec<String>,
}

#[derive(Deserialize)]
struct Table {
    templates: Vec<Template>,
    assets: BTreeMap<String, AssetIndexRef>,
    versions: Vec<OldVersion>,
}

static OLD: LazyLock<Table> =
    LazyLock::new(|| serde_json::from_str(TABLE).expect("the table ships with the app"));

/// A version a player can pick, whoever lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GameVersion {
    pub id: String,
    pub kind: String,
    pub era: String,
    pub released: String,
}

pub fn all() -> &'static [OldVersion] {
    &OLD.versions
}

pub fn find(id: &str) -> Option<&'static OldVersion> {
    OLD.versions.iter().find(|version| version.id == id)
}

impl OldVersion {
    /// One of Mojang's own releases, which launch without LegacyFix unless the player asks.
    pub fn is_mojang_release(&self) -> bool {
        self.kind == "release"
    }
}

/// A release that can run with or without LegacyFix, like 1.5.2.
pub fn mojang_release(id: &str) -> Option<&'static OldVersion> {
    find(id).filter(|version| version.is_mojang_release())
}

/// The profile to launch a build with. It stands alone, so its jar is its own.
pub fn profile(version: &OldVersion) -> VersionMeta {
    let template = &OLD.templates[version.template];
    let mut arguments = template.arguments.clone();
    arguments
        .game
        .extend(version.extra.iter().cloned().map(Argument::Plain));
    arguments
        .jvm
        .extend(version.jvm.iter().cloned().map(Argument::Plain));
    VersionMeta {
        id: format!("{}{PROFILE_SUFFIX}", version.id),
        main_class: Some(template.main_class.clone()),
        arguments: Some(arguments),
        libraries: template.libraries.clone(),
        asset_index: OLD.assets.get(&version.assets).cloned(),
        assets: Some(version.assets.clone()),
        downloads: Some(BTreeMap::from([(
            "client".to_string(),
            version.client.clone(),
        )])),
        java_version: Some(template.java_version.clone()),
        logging: template.logging.clone(),
        kind: Some(version.kind.clone()),
        ..VersionMeta::default()
    }
}

fn era_of(kind: &str) -> &'static str {
    match kind {
        "old_alpha" => "alpha",
        "old_beta" => "beta",
        "snapshot" => "snapshot",
        _ => "release",
    }
}

/// Mojang's list with every old build folded in, newest first. A build in the table
/// replaces Mojang's entry for it, so nothing shows up twice.
pub fn merged(mojang: Vec<VersionEntry>) -> Vec<GameVersion> {
    let mut versions: Vec<GameVersion> = mojang
        .into_iter()
        .filter(|entry| find(&entry.id).is_none())
        .map(|entry| GameVersion {
            era: era_of(&entry.kind).to_string(),
            id: entry.id,
            kind: entry.kind,
            released: entry.release_time,
        })
        .collect();
    versions.extend(OLD.versions.iter().map(|version| GameVersion {
        id: version.id.clone(),
        kind: version.kind.clone(),
        era: version.era.clone(),
        released: version.released.clone(),
    }));
    versions.sort_by(|a, b| b.released.cmp(&a.released));
    versions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Mojang lists these two under a second name, so both names stay findable.
    const MOJANG_REPEATS: [&str; 2] = ["a1.2.1", "rd-20090515"];

    #[test]
    fn every_build_is_complete_and_appears_once() {
        let mut ids = HashSet::new();
        let mut jars = HashSet::new();
        for version in all() {
            assert!(ids.insert(version.id.as_str()), "{} twice", version.id);
            let sha1 = version.client.sha1.as_deref().unwrap();
            let first = jars.insert(sha1);
            assert!(
                first || MOJANG_REPEATS.contains(&version.id.as_str()),
                "{} repeats a jar",
                version.id
            );
            assert_eq!(sha1.len(), 40, "{}", version.id);
            assert!(version.client.url.starts_with("https://"), "{}", version.id);
            assert!(version.template < OLD.templates.len(), "{}", version.id);
            assert!(OLD.assets.contains_key(&version.assets), "{}", version.id);
        }
        assert!(all().len() >= 297);
    }

    #[test]
    fn no_build_can_reach_a_betacraft_server_for_skins_or_saves() {
        for template in &OLD.templates {
            let flags = serde_json::to_string(&template.arguments.jvm).unwrap();
            assert!(flags.contains("-Dlf.proxy.levelServer=local"), "{flags}");
            assert!(flags.contains("-Dlf.proxy.overrides.mode=local"), "{flags}");
        }
    }

    /// Exactly the releases Mojang lists, which speedrun leaderboards accept runs on.
    #[test]
    fn mojang_s_releases_from_1_0_to_1_5_2_can_run_without_legacy_fix() {
        let releases: Vec<&str> = all()
            .iter()
            .filter(|version| version.is_mojang_release())
            .map(|version| version.id.as_str())
            .collect();
        assert_eq!(
            releases,
            [
                "1.5.2", "1.5.1", "1.4.7", "1.4.6", "1.4.5", "1.4.4", "1.4.2", "1.3.2", "1.3.1",
                "1.2.5", "1.2.4", "1.2.3", "1.2.2", "1.2.1", "1.1", "1.0"
            ]
        );
        assert!(mojang_release("1.5.2").is_some());
        assert!(mojang_release("b1.7.3").is_none());
        assert!(mojang_release("1.5.2-pre-250903").is_none());
        assert!(mojang_release("1.6.4").is_none());
    }

    #[test]
    fn a_profile_stands_alone_and_keeps_its_own_name() {
        let beta = find("b1.7.3").unwrap();
        let profile = profile(beta);
        assert_eq!(profile.id, "b1.7.3-old");
        assert!(profile.inherits_from.is_none());
        let client = &profile.downloads.as_ref().unwrap()["client"];
        assert_eq!(client.sha1, beta.client.sha1);
        assert!(profile.main_class.is_some() && !profile.libraries.is_empty());
    }

    #[test]
    fn old_builds_replace_mojang_entries_and_sort_by_date() {
        let entry = |id: &str, kind: &str, released: &str| VersionEntry {
            id: id.to_string(),
            kind: kind.to_string(),
            url: String::new(),
            sha1: None,
            release_time: released.to_string(),
        };
        let merged = merged(vec![
            entry("1.21.4", "release", "2024-12-03T10:12:57+00:00"),
            entry("b1.7.3", "old_beta", "2011-07-07T22:00:00+00:00"),
        ]);
        assert_eq!(merged[0].id, "1.21.4");
        assert_eq!(merged.iter().filter(|v| v.id == "b1.7.3").count(), 1);
        assert_eq!(merged.len(), all().len() + 1);
        assert!(
            merged
                .windows(2)
                .all(|pair| pair[0].released >= pair[1].released)
        );
    }
}

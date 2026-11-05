//! Game settings shared between instances. Each option in options.txt is shared
//! on its own, so keybinds, video and sound follow the player while anything they
//! keep apart, or an instance that keeps its own, stays put. The server list is
//! shared as a whole file.
//!
//! Changes go both ways: an instance gets the shared values when it starts, and
//! whatever the game changed during a session is taken back when it closes.
//! Minecraft 1.13 rewrote how options are saved, so 1.13 and newer share one set
//! and older releases another. Builds from before 1.0 share nothing.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Core;
use crate::Settings;
use crate::error::Result;
use crate::instance::Instance;

const OPTIONS: &str = "options.txt";
const SERVERS: &str = "servers.dat";
/// What an instance was given at launch, kept beside its waymarks.
const GIVEN: &str = ".waymarks/shared-settings.json";

/// The data version Minecraft upgrades the file from. Each instance keeps its own.
const NEVER: [&str; 1] = ["version"];
/// Not shared unless the player chooses: they belong to one pack or one moment.
pub const NOT_BY_DEFAULT: [&str; 4] = [
    "resourcePacks",
    "incompatibleResourcePacks",
    "lastServer",
    "tutorialStep",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    /// Minecraft 1.13 and newer, which name keys like `key.keyboard.w`.
    Modern,
    /// 1.0 to 1.12.2, which store keys as numbers.
    Legacy,
}

impl Family {
    fn file(self) -> &'static str {
        match self {
            Family::Modern => "options-modern.json",
            Family::Legacy => "options-legacy.json",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Family::Modern => "Minecraft 1.13 and newer",
            Family::Legacy => "Minecraft 1.12.2 and older",
        }
    }
}

/// Which set of shared options a Minecraft version uses, if any.
pub fn family(minecraft: &str) -> Option<Family> {
    let number = |text: &str| -> Option<u32> {
        let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    // Weekly snapshots, like 17w43a: the new key names arrived in 17w43a.
    if let Some((year, rest)) = minecraft.split_once('w')
        && year.len() == 2
        && let (Some(year), Some(week)) = (number(year), number(rest))
    {
        return Some(if (year, week) >= (17, 43) {
            Family::Modern
        } else {
            Family::Legacy
        });
    }
    let (major, rest) = minecraft.split_once('.')?;
    let major = number(major).filter(|_| major.chars().all(|c| c.is_ascii_digit()))?;
    match major {
        1 => match number(rest) {
            Some(minor) if minor >= 13 => Some(Family::Modern),
            Some(_) => Some(Family::Legacy),
            // Builds like 1.RV-Pre1 have no release number to go by.
            None => None,
        },
        // Versions named after the year, from 26.1 on.
        25.. => Some(Family::Modern),
        _ => None,
    }
}

/// Whether one option is shared under the player's choices.
pub fn shared(key: &str, settings: &Settings) -> bool {
    !NEVER.contains(&key)
        && settings
            .shared_choices
            .get(key)
            .copied()
            .unwrap_or(!NOT_BY_DEFAULT.contains(&key))
}

/// `key:value` lines, split at the first colon, in the order they appear.
fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Puts `wanted` into an options file and leaves every other line as it was. A new
/// file starts with the data version the values were saved under, because without
/// one Minecraft would try to upgrade them from the oldest format and fail.
fn apply(text: Option<&str>, wanted: &BTreeMap<String, String>, version: Option<&str>) -> String {
    let mut out = Vec::new();
    let mut placed = Vec::new();
    match text {
        Some(text) if !text.trim().is_empty() => {
            for line in text.lines() {
                match line.split_once(':') {
                    Some((key, _)) if wanted.contains_key(key) => {
                        out.push(format!("{key}:{}", wanted[key]));
                        placed.push(key.to_string());
                    }
                    _ => out.push(line.to_string()),
                }
            }
        }
        _ => {
            if let Some(version) = version {
                out.push(format!("version:{version}"));
            }
        }
    }
    for (key, value) in wanted {
        if !placed.contains(key) {
            out.push(format!("{key}:{value}"));
        }
    }
    out.join("\n") + "\n"
}

/// The shared options this session changed: anything whose value differs from
/// what the game was given, including options a mod added.
fn changed(
    text: &str,
    given: &BTreeMap<String, String>,
    is_shared: impl Fn(&str) -> bool,
) -> BTreeMap<String, String> {
    parse(text)
        .into_iter()
        .filter(|(key, value)| is_shared(key) && given.get(key) != Some(value))
        .collect()
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SharedOptions {
    /// The data version the latest values were saved under.
    version: Option<String>,
    values: BTreeMap<String, String>,
    /// The instance the set started from, to tell the player.
    from: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Given {
    options: BTreeMap<String, String>,
    servers: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedOption {
    pub key: String,
    pub value: String,
    pub shared: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSet {
    pub family: Family,
    pub label: &'static str,
    pub from: Option<String>,
    pub options: Vec<SharedOption>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSettings {
    pub sets: Vec<SharedSet>,
    /// Where the shared server list started, if there is one.
    pub servers_from: Option<String>,
}

impl Core {
    fn shared_dir(&self) -> PathBuf {
        self.paths.root().join("shared-settings")
    }

    fn read_shared(&self, family: Family) -> Option<SharedOptions> {
        let text = fs::read(self.shared_dir().join(family.file())).ok()?;
        serde_json::from_slice(&text).ok()
    }

    fn write_shared(&self, family: Family, shared: &SharedOptions) -> Result<()> {
        let path = self.shared_dir().join(family.file());
        crate::atomic::write(&path, &serde_json::to_vec_pretty(shared)?)?;
        Ok(())
    }

    /// Instances that take part, most recently played first.
    fn sharing_instances(&self) -> Result<Vec<Instance>> {
        Ok(self
            .instances()?
            .into_iter()
            .filter(|instance| !instance.keeps_own_settings)
            .collect())
    }

    /// The shared set for a family. The first time it's needed it starts from the
    /// most recently played instance that has settings of its own.
    fn shared_options(&self, family: Family, settings: &Settings) -> Result<SharedOptions> {
        if let Some(found) = self.read_shared(family) {
            return Ok(found);
        }
        let mut start = SharedOptions::default();
        for instance in self.sharing_instances()? {
            if crate::sync::family(&instance.minecraft) != Some(family) {
                continue;
            }
            let Ok(text) = fs::read_to_string(self.paths.game(&instance.id).join(OPTIONS)) else {
                continue;
            };
            let lines = parse(&text);
            start.version = lines
                .iter()
                .find(|(key, _)| key == "version")
                .map(|(_, value)| value.clone());
            start.values = lines
                .into_iter()
                .filter(|(key, _)| shared(key, settings))
                .collect();
            start.from = Some(instance.name.clone());
            break;
        }
        if !start.values.is_empty() {
            self.write_shared(family, &start)?;
        }
        Ok(start)
    }

    /// The shared server list, started from the most recently played instance that has one.
    fn shared_servers(&self) -> Result<Option<Vec<u8>>> {
        let path = self.shared_dir().join(SERVERS);
        if let Ok(bytes) = fs::read(&path) {
            return Ok(Some(bytes));
        }
        for instance in self.sharing_instances()? {
            if crate::sync::family(&instance.minecraft).is_none() {
                continue;
            }
            if let Ok(bytes) = fs::read(self.paths.game(&instance.id).join(SERVERS)) {
                fs::create_dir_all(self.shared_dir())?;
                fs::write(&path, &bytes)?;
                fs::write(self.shared_dir().join("servers-from.txt"), &instance.name)?;
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    fn takes_part(&self, instance: &Instance, settings: &Settings) -> bool {
        settings.share_settings
            && !instance.keeps_own_settings
            && crate::sync::family(&instance.minecraft).is_some()
    }

    /// Before a launch: gives the instance the shared options and server list, and
    /// notes what it was given so its own changes can be told apart later.
    pub(crate) fn share_settings_in(&self, instance: &Instance) -> Result<()> {
        let settings = self.settings()?;
        if !self.takes_part(instance, &settings) {
            return Ok(());
        }
        let Some(family) = crate::sync::family(&instance.minecraft) else {
            return Ok(());
        };
        let game = self.paths.game(&instance.id);
        let mut given = Given::default();

        let set = self.shared_options(family, &settings)?;
        let wanted: BTreeMap<String, String> = set
            .values
            .into_iter()
            .filter(|(key, _)| shared(key, &settings))
            .collect();
        if !wanted.is_empty() {
            let path = game.join(OPTIONS);
            let before = fs::read_to_string(&path).ok();
            let after = apply(before.as_deref(), &wanted, set.version.as_deref());
            if before.as_deref() != Some(after.as_str()) {
                fs::create_dir_all(&game)?;
                fs::write(&path, after)?;
            }
            given.options = wanted;
        }

        if settings.share_servers
            && let Some(bytes) = self.shared_servers()?
        {
            let path = game.join(SERVERS);
            if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
                fs::create_dir_all(&game)?;
                fs::write(&path, &bytes)?;
            }
            given.servers = Some(digest(&bytes));
        }

        let note = self.paths.instance(&instance.id).join(GIVEN);
        if let Some(parent) = note.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(note, serde_json::to_vec(&given)?)?;
        Ok(())
    }

    /// After the game closes: takes back whatever the session changed.
    pub(crate) fn share_settings_out(&self, instance: &Instance) -> Result<()> {
        let settings = self.settings()?;
        if !self.takes_part(instance, &settings) {
            return Ok(());
        }
        let Some(family) = crate::sync::family(&instance.minecraft) else {
            return Ok(());
        };
        let game = self.paths.game(&instance.id);
        let given: Given = fs::read(self.paths.instance(&instance.id).join(GIVEN))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();

        if let Ok(text) = fs::read_to_string(game.join(OPTIONS)) {
            let fresh = changed(&text, &given.options, |key| shared(key, &settings));
            if !fresh.is_empty() {
                let mut set = self.shared_options(family, &settings)?;
                set.values.extend(fresh);
                if let Some((_, version)) =
                    parse(&text).into_iter().find(|(key, _)| key == "version")
                {
                    set.version = Some(version);
                }
                if set.from.is_none() {
                    set.from = Some(instance.name.clone());
                }
                self.write_shared(family, &set)?;
            }
        }

        if settings.share_servers
            && let Ok(bytes) = fs::read(game.join(SERVERS))
            && given.servers.as_deref() != Some(digest(&bytes).as_str())
        {
            fs::create_dir_all(self.shared_dir())?;
            fs::write(self.shared_dir().join(SERVERS), &bytes)?;
            if !self.shared_dir().join("servers-from.txt").is_file() {
                fs::write(self.shared_dir().join("servers-from.txt"), &instance.name)?;
            }
        }
        Ok(())
    }

    /// Every shared option and whether it's shared, for choosing in Settings.
    pub fn shared_settings(&self) -> Result<SharedSettings> {
        let settings = self.settings()?;
        let mut sets = Vec::new();
        for family in [Family::Modern, Family::Legacy] {
            let set = if settings.share_settings {
                self.shared_options(family, &settings)?
            } else {
                self.read_shared(family).unwrap_or_default()
            };
            if set.values.is_empty() {
                continue;
            }
            sets.push(SharedSet {
                family,
                label: family.label(),
                from: set.from,
                options: set
                    .values
                    .into_iter()
                    .filter(|(key, _)| !NEVER.contains(&key.as_str()))
                    .map(|(key, value)| SharedOption {
                        shared: shared(&key, &settings),
                        key,
                        value,
                    })
                    .collect(),
            });
        }
        if settings.share_settings && settings.share_servers {
            self.shared_servers()?;
        }
        Ok(SharedSettings {
            sets,
            servers_from: fs::read_to_string(self.shared_dir().join("servers-from.txt")).ok(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;

    #[test]
    fn versions_fall_into_the_right_set() {
        for modern in [
            "1.13",
            "1.16.1",
            "1.21.4",
            "1.13-pre1",
            "17w43a",
            "20w14infinite",
            "26.1",
            "26.1.2",
            "1.14_combat-212796",
        ] {
            assert_eq!(family(modern), Some(Family::Modern), "{modern}");
        }
        for legacy in ["1.12.2", "1.8.9", "1.0", "1.7.10", "17w06a", "12w34a"] {
            assert_eq!(family(legacy), Some(Family::Legacy), "{legacy}");
        }
        for none in [
            "b1.7.3",
            "a1.2.6",
            "c0.30_01c",
            "rd-132211",
            "inf-20100618",
            "2.0-red",
            "3D Shareware v1.34",
            "1.RV-Pre1",
        ] {
            assert_eq!(family(none), None, "{none}");
        }
    }

    #[test]
    fn applying_replaces_and_adds_but_keeps_the_rest() {
        let wanted = BTreeMap::from([
            ("fov".to_string(), "0.5".to_string()),
            ("key_key.jump".to_string(), "key.keyboard.space".to_string()),
        ]);
        let before =
            "version:3465\nfov:0.0\nlastServer:play.example.net:25565\nmouseSensitivity:0.5\n";
        assert_eq!(
            apply(Some(before), &wanted, Some("4189")),
            "version:3465\nfov:0.5\nlastServer:play.example.net:25565\nmouseSensitivity:0.5\nkey_key.jump:key.keyboard.space\n"
        );
        assert_eq!(
            apply(None, &wanted, Some("4189")),
            "version:4189\nfov:0.5\nkey_key.jump:key.keyboard.space\n"
        );
        assert_eq!(
            apply(None, &wanted, None),
            "fov:0.5\nkey_key.jump:key.keyboard.space\n"
        );
    }

    #[test]
    fn only_what_the_session_changed_goes_back() {
        let given = BTreeMap::from([
            ("fov".to_string(), "0.5".to_string()),
            ("gamma".to_string(), "1.0".to_string()),
        ]);
        let after = "version:3465\nfov:0.8\ngamma:1.0\nkey_mod.zoom:key.keyboard.c\nresourcePacks:[\"vanilla\"]\n";
        let settings = Settings::default();
        let fresh = changed(after, &given, |key| shared(key, &settings));
        assert_eq!(
            fresh,
            BTreeMap::from([
                ("fov".to_string(), "0.8".to_string()),
                ("key_mod.zoom".to_string(), "key.keyboard.c".to_string()),
            ])
        );
    }

    #[test]
    fn choices_override_the_defaults_but_never_the_data_version() {
        let mut settings = Settings::default();
        assert!(shared("fov", &settings));
        assert!(!shared("resourcePacks", &settings));
        assert!(!shared("version", &settings));
        settings.shared_choices.insert("fov".into(), false);
        settings.shared_choices.insert("resourcePacks".into(), true);
        settings.shared_choices.insert("version".into(), true);
        assert!(!shared("fov", &settings));
        assert!(shared("resourcePacks", &settings));
        assert!(!shared("version", &settings));
    }

    struct Setup {
        _dir: tempfile::TempDir,
        core: Core,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let core = Core::new(dir.path().join("data"), crate::event::silent()).unwrap();
            let mut settings = core.settings().unwrap();
            settings.share_settings = true;
            core.save_settings(&settings).unwrap();
            Setup { _dir: dir, core }
        }

        /// An instance, played `ago` seconds back, with these settings files.
        fn instance(
            &self,
            name: &str,
            minecraft: &str,
            ago: i64,
            options: Option<&str>,
            servers: Option<&[u8]>,
        ) -> Instance {
            let mut instance = self
                .core
                .register_instance(name, minecraft, Loader::default(), minecraft.into(), None)
                .unwrap();
            instance.last_played =
                Some(time::OffsetDateTime::now_utc() - time::Duration::seconds(ago));
            self.core.save_instance(&instance).unwrap();
            let game = self.core.paths.game(&instance.id);
            if let Some(options) = options {
                fs::write(game.join(OPTIONS), options).unwrap();
            }
            if let Some(servers) = servers {
                fs::write(game.join(SERVERS), servers).unwrap();
            }
            instance
        }

        fn options(&self, instance: &Instance) -> String {
            fs::read_to_string(self.core.paths.game(&instance.id).join(OPTIONS)).unwrap_or_default()
        }

        fn servers(&self, instance: &Instance) -> Vec<u8> {
            fs::read(self.core.paths.game(&instance.id).join(SERVERS)).unwrap_or_default()
        }
    }

    #[test]
    fn settings_follow_the_player_between_instances_both_ways() {
        let s = Setup::new();
        let main = s.instance(
            "Main",
            "1.21.4",
            10,
            Some("version:4189\nfov:0.5\nkey_key.jump:key.keyboard.space\nresourcePacks:[\"vanilla\",\"file/main.zip\"]\n"),
            Some(b"main servers"),
        );
        let other = s.instance(
            "Other",
            "1.20.1",
            500,
            Some("version:3465\nfov:0.0\nresourcePacks:[\"vanilla\"]\n"),
            None,
        );

        // Other starts with Main's settings, the most recently played, but keeps its packs and version.
        s.core.share_settings_in(&other).unwrap();
        let given = s.options(&other);
        assert!(given.contains("fov:0.5"), "{given}");
        assert!(given.contains("key_key.jump:key.keyboard.space"), "{given}");
        assert!(
            given.contains("version:3465") && !given.contains("4189"),
            "{given}"
        );
        assert!(given.contains("resourcePacks:[\"vanilla\"]"), "{given}");
        assert_eq!(s.servers(&other), b"main servers");

        // Playing Other changes the field of view and the server list.
        let played = given.replace("fov:0.5", "fov:0.8");
        fs::write(s.core.paths.game(&other.id).join(OPTIONS), played).unwrap();
        fs::write(s.core.paths.game(&other.id).join(SERVERS), b"other servers").unwrap();
        s.core.share_settings_out(&other).unwrap();

        // Main gets both changes the next time it starts.
        s.core.share_settings_in(&main).unwrap();
        let back = s.options(&main);
        assert!(
            back.contains("fov:0.8") && back.contains("version:4189"),
            "{back}"
        );
        assert!(back.contains("file/main.zip"), "{back}");
        assert_eq!(s.servers(&main), b"other servers");

        // A brand-new instance gets a file that names the version its values came from.
        let fresh = s.instance("Fresh", "1.21.1", 900, None, None);
        s.core.share_settings_in(&fresh).unwrap();
        let first = s.options(&fresh);
        assert!(first.starts_with("version:"), "{first}");
        assert!(first.contains("fov:0.8"), "{first}");

        let overview = s.core.shared_settings().unwrap();
        assert_eq!(overview.sets.len(), 1);
        assert_eq!(overview.sets[0].from.as_deref(), Some("Main"));
        assert!(
            overview.sets[0]
                .options
                .iter()
                .all(|option| option.key != "version")
        );
        assert_eq!(overview.servers_from.as_deref(), Some("Main"));
    }

    #[test]
    fn an_instance_can_keep_its_own_and_old_releases_have_their_own_set() {
        let s = Setup::new();
        s.instance("Main", "1.21.4", 10, Some("version:4189\nfov:0.5\n"), None);
        let mut own = s.instance(
            "Speedrun",
            "1.16.1",
            20,
            Some("version:2586\nfov:1.0\n"),
            None,
        );
        own.keeps_own_settings = true;
        s.core.save_instance(&own).unwrap();
        let pvp = s.instance(
            "PvP",
            "1.8.9",
            30,
            Some("fov:0.25\nkey_key.jump:57\n"),
            None,
        );
        let older = s.instance("Older", "1.12.2", 40, Some("fov:0.0\n"), None);

        s.core.share_settings_in(&own).unwrap();
        assert_eq!(s.options(&own), "version:2586\nfov:1.0\n");

        s.core.share_settings_in(&older).unwrap();
        let legacy = s.options(&older);
        assert!(
            legacy.contains("fov:0.25") && legacy.contains("key_key.jump:57"),
            "{legacy}"
        );
        assert!(!legacy.contains("0.5"), "{legacy}");
        assert_eq!(s.options(&pvp), "fov:0.25\nkey_key.jump:57\n");
    }

    #[test]
    fn nothing_moves_until_sharing_is_on_or_for_options_kept_apart() {
        let s = Setup::new();
        let mut settings = s.core.settings().unwrap();
        settings.share_settings = false;
        s.core.save_settings(&settings).unwrap();
        s.instance(
            "Main",
            "1.21.4",
            10,
            Some("version:4189\nfov:0.5\ngamma:0.9\n"),
            None,
        );
        let other = s.instance(
            "Other",
            "1.21.4",
            20,
            Some("version:4189\nfov:0.0\ngamma:0.1\n"),
            None,
        );
        s.core.share_settings_in(&other).unwrap();
        assert_eq!(s.options(&other), "version:4189\nfov:0.0\ngamma:0.1\n");

        settings.share_settings = true;
        settings.shared_choices.insert("gamma".into(), false);
        s.core.save_settings(&settings).unwrap();
        s.core.share_settings_in(&other).unwrap();
        assert_eq!(s.options(&other), "version:4189\nfov:0.5\ngamma:0.1\n");
    }
}

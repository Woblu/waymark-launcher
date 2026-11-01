//! Mojang's version metadata, and the rules for reading it. Loader profiles
//! use the same format, so a modded version is a child merged over vanilla.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const RESOURCES_URL: &str = "https://resources.download.minecraft.net";
const LIBRARIES_URL: &str = "https://libraries.minecraft.net/";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<VersionEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub release_time: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMeta {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<BTreeMap<String, Download>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Ruled { rules: Vec<Rule>, value: OneOrMany },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LoggingFile,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibraryDownloads>,
    /// The Maven repository, used by loader profiles instead of `downloads`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<LibraryFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<BTreeMap<String, LibraryFile>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibraryFile {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeMap<String, bool>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Disallow,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OsRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Launcher capabilities that version rules can ask about.
#[derive(Clone, Copy, Debug, Default)]
pub struct Features {
    pub custom_resolution: bool,
}

pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        _ => "linux",
    }
}

fn os_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "x86_64",
    }
}

impl OsRule {
    fn matches(&self) -> bool {
        let name = self.name.as_deref().is_none_or(|n| n == os_name());
        let arch = self.arch.as_deref().is_none_or(|a| a == os_arch());
        // Version rules exist for old Windows 10 and pre-Sierra macOS checks.
        // Every system that can run this launcher answers them the same way.
        let version = self
            .version
            .as_deref()
            .is_none_or(|v| os_name() == "windows" && v.starts_with("^10"));
        name && arch && version
    }
}

impl Rule {
    fn applies(&self, features: Features) -> bool {
        let os = self.os.as_ref().is_none_or(OsRule::matches);
        let wanted = self.features.as_ref().is_none_or(|wanted| {
            wanted.iter().all(|(name, expected)| {
                let have = name == "has_custom_resolution" && features.custom_resolution;
                have == *expected
            })
        });
        os && wanted
    }
}

/// The last rule that applies decides. No rules means allowed; any rules at
/// all means denied until one allows it.
pub fn allowed(rules: &[Rule], features: Features) -> bool {
    if rules.is_empty() {
        return true;
    }
    rules
        .iter()
        .rfind(|rule| rule.applies(features))
        .is_some_and(|rule| rule.action == Action::Allow)
}

/// A file to fetch and where it goes, relative to the libraries directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryArtifact {
    pub path: String,
    /// `None` for files only an installer can produce.
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

pub fn maven_path(name: &str) -> Option<String> {
    let (coordinates, extension) = name.split_once('@').unwrap_or((name, "jar"));
    let parts: Vec<&str> = coordinates.split(':').collect();
    let (group, artifact, version) = (parts.first()?, parts.get(1)?, parts.get(2)?);
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Some(format!(
        "{}/{artifact}/{version}/{artifact}-{version}{classifier}.{extension}",
        group.replace('.', "/")
    ))
}

impl Library {
    pub fn is_allowed(&self, features: Features) -> bool {
        allowed(&self.rules, features)
    }

    /// Identifies a library regardless of version, so a loader's newer copy
    /// replaces the one vanilla asks for.
    fn identity(&self) -> String {
        let coordinates = self.name.split('@').next().unwrap_or(&self.name);
        let mut parts: Vec<&str> = coordinates.split(':').collect();
        if parts.len() > 2 {
            parts.remove(2);
        }
        parts.join(":")
    }

    pub fn artifact(&self) -> Option<LibraryArtifact> {
        if let Some(file) = self.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            return Some(LibraryArtifact {
                path: file.path.clone().or_else(|| maven_path(&self.name))?,
                url: file.url.clone().filter(|u| !u.is_empty()),
                sha1: file.sha1.clone(),
                size: file.size,
            });
        }
        if self.downloads.is_some() || self.natives.is_some() {
            return None;
        }
        let path = maven_path(&self.name)?;
        let base = self.url.as_deref().unwrap_or(LIBRARIES_URL);
        Some(LibraryArtifact {
            url: Some(format!("{}/{path}", base.trim_end_matches('/'))),
            path,
            sha1: self.sha1.clone(),
            size: self.size,
        })
    }

    /// The native library for this platform, on versions that package them
    /// as classifiers. Newer versions list natives as ordinary libraries.
    pub fn native(&self) -> Option<LibraryArtifact> {
        let classifier = self.natives.as_ref()?.get(os_name())?;
        let classifier = classifier.replace(
            "${arch}",
            if cfg!(target_pointer_width = "64") {
                "64"
            } else {
                "32"
            },
        );
        let Some(downloads) = &self.downloads else {
            // Old Forge profiles name their natives without saying where they are.
            let path = maven_path(&format!("{}:{classifier}", self.name))?;
            let base = self.url.as_deref().unwrap_or(LIBRARIES_URL);
            return Some(LibraryArtifact {
                url: Some(format!("{}/{path}", base.trim_end_matches('/'))),
                path,
                sha1: None,
                size: None,
            });
        };
        let file = downloads.classifiers.as_ref()?.get(&classifier)?;
        Some(LibraryArtifact {
            path: file.path.clone()?,
            url: file.url.clone().filter(|u| !u.is_empty()),
            sha1: file.sha1.clone(),
            size: file.size,
        })
    }
}

impl VersionMeta {
    /// Lays this version over the one it inherits from. The child wins, and
    /// its libraries come first so they take priority on the classpath.
    pub fn merged_over(self, parent: VersionMeta) -> VersionMeta {
        // A version can list a library twice, once per system, so only the child replaces one.
        let replaced: HashSet<String> = self.libraries.iter().map(Library::identity).collect();
        // Old versions ship one big ASM jar, which clashes with a loader's newer split ones.
        let split_asm = replaced.contains("org.ow2.asm:asm");
        let inherited = parent.libraries.into_iter().filter(|library| {
            let identity = library.identity();
            let clashes = split_asm && identity == "org.ow2.asm:asm-all";
            !replaced.contains(&identity) && !clashes
        });
        let libraries = self.libraries.into_iter().chain(inherited).collect();

        let arguments = match (parent.arguments, self.arguments) {
            (Some(mut base), Some(extra)) => {
                base.game.extend(extra.game);
                base.jvm.extend(extra.jvm);
                Some(base)
            }
            (base, extra) => extra.or(base),
        };

        VersionMeta {
            id: self.id,
            inherits_from: None,
            main_class: self.main_class.or(parent.main_class),
            minecraft_arguments: self.minecraft_arguments.or(parent.minecraft_arguments),
            arguments,
            libraries,
            asset_index: self.asset_index.or(parent.asset_index),
            assets: self.assets.or(parent.assets),
            downloads: self.downloads.or(parent.downloads),
            java_version: self.java_version.or(parent.java_version),
            logging: self.logging.or(parent.logging),
            kind: self.kind.or(parent.kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(json: &str) -> Vec<Rule> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_library_with_no_rules_is_allowed() {
        assert!(allowed(&[], Features::default()));
    }

    #[test]
    fn the_last_matching_rule_wins() {
        let this_os = os_name();
        let rules = rule(&format!(
            r#"[{{"action":"allow"}},{{"action":"disallow","os":{{"name":"{this_os}"}}}}]"#
        ));
        assert!(!allowed(&rules, Features::default()));
    }

    #[test]
    fn a_rule_for_another_os_does_not_apply() {
        let other = if os_name() == "linux" { "osx" } else { "linux" };
        let rules = rule(&format!(
            r#"[{{"action":"allow","os":{{"name":"{other}"}}}}]"#
        ));
        assert!(!allowed(&rules, Features::default()));
    }

    #[test]
    fn feature_rules_only_match_when_the_feature_is_on() {
        let rules = rule(r#"[{"action":"allow","features":{"has_custom_resolution":true}}]"#);
        assert!(!allowed(&rules, Features::default()));
        assert!(allowed(
            &rules,
            Features {
                custom_resolution: true
            }
        ));
    }

    #[test]
    fn unknown_features_stay_off() {
        let rules = rule(r#"[{"action":"allow","features":{"is_demo_user":true}}]"#);
        assert!(!allowed(
            &rules,
            Features {
                custom_resolution: true
            }
        ));
    }

    #[test]
    fn builds_maven_paths() {
        assert_eq!(
            maven_path("org.ow2.asm:asm:9.7").unwrap(),
            "org/ow2/asm/asm/9.7/asm-9.7.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar"
        );
        assert_eq!(
            maven_path("net.example:thing:1@zip").unwrap(),
            "net/example/thing/1/thing-1.zip"
        );
    }

    #[test]
    fn loader_libraries_resolve_against_their_repository() {
        let library = Library {
            name: "net.fabricmc:fabric-loader:0.16.9".into(),
            url: Some("https://maven.fabricmc.net/".into()),
            ..Library::default()
        };
        assert_eq!(
            library.artifact().unwrap().url.unwrap(),
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
        );
    }

    #[test]
    fn installer_generated_libraries_have_nothing_to_download() {
        let library: Library = serde_json::from_str(
            r#"{"name":"net.neoforged:neoforge:21.4.1:client","downloads":{"artifact":{"path":"a/b.jar","url":"","sha1":"x","size":1}}}"#,
        )
        .unwrap();
        assert_eq!(library.artifact().unwrap().url, None);
    }

    #[test]
    fn a_parent_library_listed_once_per_system_stays_whole() {
        let parent = VersionMeta {
            id: "1.16.5".into(),
            libraries: vec![
                Library {
                    name: "org.lwjgl:lwjgl-glfw:3.2.1".into(),
                    ..Library::default()
                },
                Library {
                    name: "org.lwjgl:lwjgl-glfw:3.2.2".into(),
                    ..Library::default()
                },
            ],
            ..VersionMeta::default()
        };
        let child = VersionMeta {
            id: "1.16.5-forge".into(),
            ..VersionMeta::default()
        };

        assert_eq!(child.merged_over(parent).libraries.len(), 2);
    }

    #[test]
    fn a_child_library_replaces_the_parent_copy() {
        let parent = VersionMeta {
            id: "1.21.4".into(),
            main_class: Some("net.minecraft.client.main.Main".into()),
            libraries: vec![
                Library {
                    name: "org.ow2.asm:asm:9.3".into(),
                    ..Library::default()
                },
                Library {
                    name: "com.mojang:brigadier:1.3".into(),
                    ..Library::default()
                },
            ],
            ..VersionMeta::default()
        };
        let child = VersionMeta {
            id: "fabric-loader-0.16.9-1.21.4".into(),
            main_class: Some("net.fabricmc.loader.impl.launch.knot.KnotClient".into()),
            libraries: vec![Library {
                name: "org.ow2.asm:asm:9.7".into(),
                ..Library::default()
            }],
            ..VersionMeta::default()
        };

        let merged = child.merged_over(parent);

        assert_eq!(merged.id, "fabric-loader-0.16.9-1.21.4");
        assert_eq!(
            merged.main_class.unwrap(),
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        let names: Vec<&str> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["org.ow2.asm:asm:9.7", "com.mojang:brigadier:1.3"]);
    }

    #[test]
    fn natives_named_without_an_address_come_from_mojang() {
        let library: Library = serde_json::from_str(&format!(
            r#"{{"name":"net.java.jinput:jinput-platform:2.0.5","natives":{{"{}":"natives-test"}}}}"#,
            os_name()
        ))
        .unwrap();
        let native = library.native().unwrap();
        assert_eq!(
            native.url.unwrap(),
            "https://libraries.minecraft.net/net/java/jinput/jinput-platform/2.0.5/jinput-platform-2.0.5-natives-test.jar"
        );
        assert!(library.artifact().is_none());
    }

    #[test]
    fn natives_with_classifiers_in_the_name_stay_separate() {
        let parent = VersionMeta {
            libraries: vec![
                Library {
                    name: "org.lwjgl:lwjgl:3.3.3".into(),
                    ..Library::default()
                },
                Library {
                    name: "org.lwjgl:lwjgl:3.3.3:natives-windows".into(),
                    ..Library::default()
                },
            ],
            ..VersionMeta::default()
        };
        assert_eq!(
            VersionMeta::default().merged_over(parent).libraries.len(),
            2
        );
    }

    #[test]
    fn parses_ruled_arguments() {
        let arguments: Arguments = serde_json::from_str(
            r#"{"game":["--demo",{"rules":[{"action":"allow"}],"value":["--width","1"]}],"jvm":[{"rules":[{"action":"allow"}],"value":"-Dx=y"}]}"#,
        )
        .unwrap();
        assert!(matches!(
            arguments.game[1],
            Argument::Ruled {
                value: OneOrMany::Many(_),
                ..
            }
        ));
        assert!(matches!(
            arguments.jvm[0],
            Argument::Ruled {
                value: OneOrMany::One(_),
                ..
            }
        ));
    }
}

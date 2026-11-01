//! Share codes: a whole instance in a line of text, with no server in between.
//! A code only names store projects and versions, which is the rule the
//! lockfile exists for: game files and mods are never passed between people,
//! and whoever pastes a code downloads everything from the stores themselves.
//!
//! Codes come from other people, so nothing in one is trusted. Only ids and
//! names are read, and launch commands can't travel in a code at all.

use std::io::{Read, Write};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde::{Deserialize, Serialize};

use crate::Core;
use crate::content::{ContentKind, ContentSource};
use crate::error::{Error, Result};
use crate::instance::{Instance, NewInstance};
use crate::lock::{Loader, Lockfile, SHELVES, Source};

const PREFIX: &str = "waymark1.";
/// Far more than any real instance, so a crafted code can't fill the memory.
const MAX_BYTES: u64 = 1 << 20;
const MAX_NAME: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SharedItem {
    kind: ContentKind,
    source: ContentSource,
    project: String,
    version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SharedInstance {
    name: String,
    minecraft: String,
    loader: Loader,
    items: Vec<SharedItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareCode {
    pub code: String,
    /// What can't travel in a code, because no store has it.
    pub left_out: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedResult {
    pub instance: Instance,
    /// What couldn't be installed, usually because it's gone from the store.
    pub failed: Vec<String>,
}

fn describe(name: &str, lock: &Lockfile) -> (SharedInstance, Vec<String>) {
    let mut items = Vec::new();
    let mut left_out = Vec::new();
    for kind in SHELVES
        .into_iter()
        .filter(|kind| *kind != ContentKind::DataPack)
    {
        for entry in lock.shelf(kind).iter().filter(|entry| entry.enabled) {
            match &entry.source {
                Source::Modrinth { project, version } => {
                    items.push(SharedItem {
                        kind,
                        source: ContentSource::Modrinth,
                        project: project.clone(),
                        version: version.clone(),
                    });
                }
                Source::CurseForge { project, file } => {
                    items.push(SharedItem {
                        kind,
                        source: ContentSource::CurseForge,
                        project: project.to_string(),
                        version: file.to_string(),
                    });
                }
                Source::Manual => left_out.push(entry.name.clone()),
            }
        }
    }
    (
        SharedInstance {
            name: name.to_string(),
            minecraft: lock.minecraft.clone(),
            loader: lock.loader.clone(),
            items,
        },
        left_out,
    )
}

fn encode(shared: &SharedInstance) -> Result<String> {
    let mut packer = DeflateEncoder::new(Vec::new(), Compression::best());
    packer.write_all(&serde_json::to_vec(shared)?)?;
    Ok(format!(
        "{PREFIX}{}",
        URL_SAFE_NO_PAD.encode(packer.finish()?)
    ))
}

/// Ids are checked because they end up in web addresses.
fn plain(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_NAME
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn decode(code: &str) -> Result<SharedInstance> {
    let wrong = || Error::Other("that isn't a Waymark share code".into());
    let body = code.trim().strip_prefix(PREFIX).ok_or_else(wrong)?;
    let packed = URL_SAFE_NO_PAD.decode(body).map_err(|_| wrong())?;
    let mut text = Vec::new();
    DeflateDecoder::new(&packed[..])
        .take(MAX_BYTES)
        .read_to_end(&mut text)
        .map_err(|_| wrong())?;
    let shared: SharedInstance = serde_json::from_slice(&text).map_err(|_| wrong())?;

    let usable = |item: &SharedItem| {
        plain(&item.project)
            && plain(&item.version)
            && matches!(
                item.kind,
                ContentKind::Mod | ContentKind::ResourcePack | ContentKind::Shader
            )
            && matches!(
                item.source,
                ContentSource::Modrinth | ContentSource::CurseForge
            )
    };
    if !plain(&shared.minecraft) || !shared.items.iter().all(usable) {
        return Err(wrong());
    }
    if shared
        .loader
        .version
        .as_deref()
        .is_some_and(|version| !plain(version))
    {
        return Err(wrong());
    }
    Ok(SharedInstance {
        name: shared
            .name
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_NAME)
            .collect(),
        ..shared
    })
}

impl Core {
    pub fn share_code(&self, id: &str) -> Result<ShareCode> {
        let instance = self.instance(id)?;
        let (shared, left_out) = describe(&instance.name, &self.lockfile(id)?);
        Ok(ShareCode {
            code: encode(&shared)?,
            left_out,
        })
    }

    /// Builds a new instance from a code.
    pub async fn install_share(&self, code: &str) -> Result<SharedResult> {
        let shared = decode(code)?;
        let instance = self
            .create_instance(NewInstance {
                name: if shared.name.trim().is_empty() {
                    "Shared instance".to_string()
                } else {
                    shared.name.clone()
                },
                minecraft: shared.minecraft.clone(),
                loader: shared.loader.kind,
                loader_version: shared.loader.version.clone(),
            })
            .await?;
        let mut failed = Vec::new();
        for item in &shared.items {
            if let Err(error) = self
                .install_content(
                    &instance.id,
                    item.source,
                    &item.project,
                    item.kind,
                    Some(&item.version),
                    None,
                )
                .await
            {
                failed.push(format!("{}: {error}", item.project));
            }
        }
        Ok(SharedResult { instance, failed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{LoaderKind, LockedMod, Origin, Side};

    fn entry(name: &str, source: Source) -> LockedMod {
        LockedMod {
            name: name.into(),
            file: format!("{name}.jar"),
            hash: name.into(),
            size: 1,
            enabled: true,
            source,
            origin: Origin::Manual,
            side: Side::Both,
            version: None,
            icon: None,
            group: None,
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        }
    }

    fn lock() -> Lockfile {
        let mut lock = Lockfile::new(
            "1.21.4",
            Loader {
                kind: LoaderKind::Fabric,
                version: Some("0.16.9".into()),
            },
        );
        lock.mods.push(entry(
            "sodium",
            Source::Modrinth {
                project: "AANobbMI".into(),
                version: "abc".into(),
            },
        ));
        lock.mods.push(entry(
            "jei",
            Source::CurseForge {
                project: 238222,
                file: 77,
            },
        ));
        lock.mods.push(entry("mine", Source::Manual));
        lock
    }

    #[test]
    fn a_code_round_trips_and_says_what_it_left_out() {
        let (shared, left_out) = describe("My Pack", &lock());
        assert_eq!(left_out, ["mine"]);
        assert_eq!(shared.items.len(), 2);
        let code = encode(&shared).unwrap();
        assert!(code.starts_with(PREFIX));
        assert_eq!(decode(&format!("  {code}\n")).unwrap(), shared);
    }

    #[test]
    fn switched_off_mods_stay_home() {
        let mut lock = lock();
        lock.mods[0].enabled = false;
        assert_eq!(describe("P", &lock).0.items.len(), 1);
    }

    #[test]
    fn a_big_instance_still_fits_in_a_message() {
        let mut lock = Lockfile::new("1.21.4", Loader::default());
        for number in 0..200 {
            lock.mods.push(entry(
                &number.to_string(),
                Source::Modrinth {
                    project: format!("{number:08}"),
                    version: format!("{:08}", number * 7),
                },
            ));
        }
        let code = encode(&describe("Big", &lock).0).unwrap();
        assert!(code.len() < 12_000, "{} characters", code.len());
    }

    fn packed(text: &str) -> String {
        let mut packer = DeflateEncoder::new(Vec::new(), Compression::fast());
        packer.write_all(text.as_bytes()).unwrap();
        format!(
            "{PREFIX}{}",
            URL_SAFE_NO_PAD.encode(packer.finish().unwrap())
        )
    }

    #[test]
    fn nonsense_and_tricks_are_refused() {
        for bad in ["", "hello", "waymark1.", "waymark1.!!!", "waymark2.abc"] {
            assert!(decode(bad).is_err(), "{bad}");
        }
        let item = |kind: &str, project: &str, version: &str| {
            format!(
                r#"{{"name":"x","minecraft":"1.21.4","loader":{{"kind":"fabric"}},"items":[{{"kind":"{kind}","source":"modrinth","project":"{project}","version":"{version}"}}]}}"#
            )
        };
        assert!(decode(&packed(&item("mod", "good", "1"))).is_ok());
        assert!(decode(&packed(&item("mod", "../../evil", "1"))).is_err());
        assert!(decode(&packed(&item("mod", "good", "1/../../x"))).is_err());
        assert!(decode(&packed(&item("mod", "a b", "1"))).is_err());
        assert!(decode(&packed(&item("modpack", "good", "1"))).is_err());
        assert!(decode(&packed(&item("datapack", "good", "1"))).is_err());
    }

    #[test]
    fn a_code_cannot_carry_commands() {
        let hostile = r#"{"name":"x","minecraft":"1.21.4","loader":{"kind":"vanilla"},"items":[],"preLaunch":"calc.exe","wrapper":"calc.exe"}"#;
        let shared = decode(&packed(hostile)).unwrap();
        assert!(!serde_json::to_string(&shared).unwrap().contains("calc"));
    }

    #[test]
    fn a_huge_code_stops_at_the_limit() {
        let bomb = format!(r#"{{"name":"{}"}}"#, "a".repeat(40 << 20));
        assert!(decode(&packed(&bomb)).is_err());
    }
}

//! Works out what a jar actually is. Mods that arrive by hand or from another
//! launcher have no store details, so they're matched against Modrinth by file
//! hash, which also brings them under update checks. Anything Modrinth doesn't
//! know takes its name and icon from its own jar. A jar with no icon borrows
//! the image of the Modrinth project with the same mod id, and only the image,
//! because a backport of Sodium isn't Sodium and mustn't update to it.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;

use sha2::{Digest, Sha512};

use crate::Core;
use crate::content::side_from;
use crate::error::{Error, Result};
use crate::lock::{LockedMod, Source, display_name};
use crate::modinfo::{self, ModInfo};
use crate::modrinth;

const IDENTIFIED_FILE: &str = ".waymarks/identified.json";
const LOOKUP_BATCH: usize = 80;

/// The marker a mod's `icon` carries when the image lives in the icon cache.
pub const LOCAL_ICON: &str = "local:";

fn sha512_of(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha512::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(hex::encode(hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

/// Hashes across all cores, so a big imported pack doesn't take long.
fn hash_all(files: &[PathBuf]) -> Vec<Option<String>> {
    if files.is_empty() {
        return Vec::new();
    }
    let threads = thread::available_parallelism().map_or(4, usize::from);
    thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(files.len().div_ceil(threads))
            .map(|batch| scope.spawn(move || batch.iter().map(|p| sha512_of(p)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("hashing thread panicked")).collect()
    })
}

/// Whether a mod still needs looking up.
fn unknown(entry: &LockedMod, done: &HashSet<String>) -> bool {
    !done.contains(&entry.hash) && (matches!(entry.source, Source::Manual) || entry.icon.is_none())
}

/// Uses the jar's own name and version where all we have is the filename.
fn name_from_jar(entry: &mut LockedMod, info: &ModInfo) -> bool {
    let mut changed = false;
    if entry.name == display_name(&entry.file)
        && let Some(name) = &info.name
    {
        entry.name = name.clone();
        changed = true;
    }
    if entry.version.is_none() && info.version.is_some() {
        entry.version = info.version.clone();
        changed = true;
    }
    changed
}

/// Mod ids and Modrinth slugs aren't the same namespace, so a shared id only
/// counts if the names agree too. Comparing by prefix lets "Sodium 1.16.1
/// Backport" match "Sodium".
fn same_mod(name: &str, title: &str) -> bool {
    let plain = |text: &str| text.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    let (name, title) = (plain(name), plain(title));
    !name.is_empty() && !title.is_empty() && (name.starts_with(&title) || title.starts_with(&name))
}

impl Core {
    /// The cached image for a hash, if it's a real hash and we have one.
    pub fn cached_icon(&self, hash: &str) -> Option<Vec<u8>> {
        let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        valid.then(|| fs::read(self.paths.icons().join(format!("{hash}.png"))).ok())?
    }

    /// Fills in names, icons and versions for mods we know nothing about, and
    /// returns true if anything changed. Failing to reach Modrinth isn't an
    /// error: icons from jars still apply, and the rest is retried next time.
    pub async fn identify_mods(&self, id: &str) -> Result<bool> {
        let instance_dir = self.paths.instance(id);
        let done_path = instance_dir.join(IDENTIFIED_FILE);
        let mut done: HashSet<String> = fs::read(&done_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();

        let mut lock = self.lockfile(id)?;
        let mods_dir = self.paths.game(id).join("mods");
        let targets: Vec<usize> = (0..lock.mods.len()).filter(|&i| unknown(&lock.mods[i], &done)).collect();
        if targets.is_empty() {
            return Ok(false);
        }
        let files: Vec<PathBuf> = targets.iter().map(|&i| mods_dir.join(lock.mods[i].disk_name())).collect();

        let hashing = files.clone();
        let hashes = tokio::task::spawn_blocking(move || hash_all(&hashing))
            .await
            .map_err(|e| Error::Other(e.to_string()))?;

        let known: Vec<String> = hashes.iter().flatten().cloned().collect();
        let mut online = true;
        let mut versions = BTreeMap::new();
        for chunk in known.chunks(LOOKUP_BATCH) {
            match modrinth::versions_for_files(&self.net, chunk).await {
                Ok(found) => versions.extend(found),
                Err(_) => online = false,
            }
        }
        let projects: Vec<String> = versions.values().map(|v: &modrinth::Version| v.project_id.clone()).collect();
        let mut details = BTreeMap::new();
        for chunk in projects.chunks(LOOKUP_BATCH) {
            if let Ok(found) = modrinth::projects(&self.net, chunk).await {
                details.extend(found.into_iter().map(|p| (p.id.clone(), p)));
            }
        }

        fs::create_dir_all(self.paths.icons())?;
        let mut changed = false;
        let mut without_images = Vec::new();
        for (position, &index) in targets.iter().enumerate() {
            let entry = &mut lock.mods[index];
            let remote = hashes[position].as_ref();
            let match_found = remote.and_then(|hash| versions.get(hash));
            if let Some(version) = match_found {
                if let Some(project) = details.get(&version.project_id) {
                    entry.name = project.title.clone();
                    entry.icon = project.icon_url.clone();
                    entry.side = side_from(project);
                }
                entry.version = Some(version.version_number.clone());
                entry.remote_hash = remote.cloned();
                entry.source = Source::Modrinth { project: version.project_id.clone(), version: version.id.clone() };
                changed = true;
            }
            let jar = mods_dir.join(entry.disk_name());
            let info = modinfo::read(&jar).unwrap_or_default();
            if match_found.is_none() {
                changed |= name_from_jar(entry, &info);
            }
            if entry.icon.is_none()
                && let Some(icon) = self.icon_from_jar(&jar, &info, &entry.hash)
            {
                entry.icon = Some(icon);
                changed = true;
            }
            if entry.icon.is_none()
                && let Some(mod_id) = info.id
            {
                without_images.push((index, mod_id));
            }
        }

        let slugs: Vec<String> = without_images.iter().map(|(_, mod_id)| mod_id.clone()).collect();
        let mut namesakes = BTreeMap::new();
        for chunk in slugs.chunks(LOOKUP_BATCH) {
            match modrinth::projects(&self.net, chunk).await {
                Ok(found) => namesakes.extend(found.into_iter().filter_map(|p| Some((p.slug.clone()?, p)))),
                Err(_) => online = false,
            }
        }
        for (index, mod_id) in without_images {
            let entry = &mut lock.mods[index];
            if let Some(project) = namesakes.get(&mod_id)
                && project.icon_url.is_some()
                && same_mod(&entry.name, &project.title)
            {
                entry.icon = project.icon_url.clone();
                changed = true;
            }
        }

        if online {
            done.extend(targets.iter().map(|&index| lock.mods[index].hash.clone()));
        }
        if changed {
            lock.save(&instance_dir)?;
        }
        if let Some(parent) = done_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&done_path, serde_json::to_vec(&done)?)?;
        Ok(changed)
    }

    fn icon_from_jar(&self, jar: &Path, info: &ModInfo, hash: &str) -> Option<String> {
        let bytes = modinfo::read_icon(jar, info.icon.as_ref()?)?;
        fs::write(self.paths.icons().join(format!("{hash}.png")), bytes).ok()?;
        Some(format!("{LOCAL_ICON}{hash}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{Origin, Side};
    use std::io::Write;

    fn entry(source: Source, icon: Option<&str>) -> LockedMod {
        LockedMod {
            name: "m".into(),
            file: "m.jar".into(),
            hash: "abc".into(),
            size: 1,
            enabled: true,
            source,
            origin: Origin::Manual,
            side: Side::Both,
            version: None,
            icon: icon.map(String::from),
            group: None,
            tags: Vec::new(),
            requires: Vec::new(),
            remote_hash: None,
        }
    }

    #[test]
    fn hand_placed_mods_need_identifying_and_store_mods_with_icons_do_not() {
        let done = HashSet::new();
        let store = Source::Modrinth { project: "p".into(), version: "v".into() };
        assert!(unknown(&entry(Source::Manual, None), &done));
        assert!(unknown(&entry(store.clone(), None), &done));
        assert!(!unknown(&entry(store, Some("https://x/icon.png")), &done));
    }

    #[test]
    fn a_mod_is_only_looked_up_once() {
        let done = HashSet::from(["abc".to_string()]);
        assert!(!unknown(&entry(Source::Manual, None), &done));
    }

    #[test]
    fn a_jars_name_replaces_a_filename_but_not_a_proper_title() {
        let info = ModInfo { name: Some("Sodium".into()), version: Some("2.5".into()), ..ModInfo::default() };

        let mut from_file = entry(Source::Manual, None);
        from_file.file = "sodium-2.5.jar".into();
        from_file.name = "sodium 2.5".into();
        assert!(name_from_jar(&mut from_file, &info));
        assert_eq!(from_file.name, "Sodium");
        assert_eq!(from_file.version.as_deref(), Some("2.5"));

        let mut titled = entry(Source::Manual, None);
        titled.file = "sodium-2.5.jar".into();
        titled.name = "Sodium Extra Special".into();
        titled.version = Some("3".into());
        assert!(!name_from_jar(&mut titled, &info));
        assert_eq!(titled.name, "Sodium Extra Special");
        assert_eq!(titled.version.as_deref(), Some("3"));
    }

    #[test]
    fn an_image_is_only_borrowed_when_the_names_agree() {
        assert!(same_mod("Sodium", "Sodium"));
        assert!(same_mod("Krypton 1.16.1 Backport", "Krypton"));
        assert!(same_mod("Starlight", "Starlight (Fabric)"));
        assert!(same_mod("SpeedRunIGT", "SpeedRunIGT"));
        assert!(!same_mod("Core Library", "Lithium"));
        assert!(!same_mod("", "Lithium"));
    }

    #[test]
    fn hashes_a_file_with_sha512() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        fs::write(&path, b"abc").unwrap();
        assert!(sha512_of(&path).unwrap().starts_with("ddaf35a193617aba"));
    }

    #[test]
    fn only_real_hashes_can_reach_the_icon_cache() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        fs::write(dir.path().join("secret.png"), b"x").unwrap();
        assert!(core.cached_icon("../secret").is_none());
        assert!(core.cached_icon(&"z".repeat(64)).is_none());
    }

    #[test]
    fn takes_the_icon_out_of_a_jar() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        fs::create_dir_all(core.paths.icons()).unwrap();
        let jar = dir.path().join("m.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&jar).unwrap());
        zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(br#"{"name":"M","icon":"icon.png"}"#).unwrap();
        zip.start_file("icon.png", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2]).unwrap();
        zip.finish().unwrap();

        let hash = "a".repeat(64);
        let info = modinfo::read(&jar).unwrap();
        assert_eq!(core.icon_from_jar(&jar, &info, &hash).unwrap(), format!("local:{hash}"));
        assert!(core.cached_icon(&hash).is_some());
    }
}

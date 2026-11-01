//! Technic Platform packs. A pack is one zip at an address its author chose, or a
//! Solder server's list of zips, and either way they're laid over each other in
//! the game folder. The loader is in `bin`, which is only ever read to find out
//! which loader a pack wants. Nothing from a pack is run or patched into the game:
//! the real loader comes from its own publisher, the same as for any other pack.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use std::time::Duration;

use reqwest::Url;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::atlauncher::plain;
use crate::content::{ContentHit, ContentKind, ContentPage, ContentSource, SearchQuery};
use crate::error::{Error, Result};
use crate::lock::{Loader, LoaderKind};
use crate::net::{Net, is_local};
use crate::projects::{BodyFormat, Creator, Link, ProjectPage, VersionRow};

const API: &str = "https://api.technicpack.net";
/// The launcher build the API wants to hear about. A high one sees every pack.
const BUILD: &str = "999";
/// The packs Technic's own launcher shows before anyone has searched.
const FEATURED: &str = "https://solder.technicpack.net/api/modpack?include=full";
/// Folders that belong to the launcher, so a pack's copies stay out of the game.
pub const RESERVED: [&str; 2] = ["bin/", "cache/"];
/// More than a Forge installer ever is, and all of it gets held in memory.
const MAX_LOADER_JAR: u64 = 64 << 20;
/// Solder turns away anything quick and asks for a few seconds' wait.
const SOLDER_WAIT: Duration = Duration::from_secs(7);
const SOLDER_RETRIES: u32 = 3;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub platform_url: Option<String>,
    #[serde(default)]
    pub minecraft: Option<String>,
    #[serde(default)]
    pub installs: Option<u64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub icon: Option<Art>,
    #[serde(default)]
    pub solder: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Art {
    #[serde(default)]
    pub url: Option<String>,
}

impl Pack {
    pub fn title(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.name)
    }

    pub fn icon_url(&self) -> Option<String> {
        self.icon.as_ref()?.url.clone()
    }

    fn solder_root(&self) -> Option<&str> {
        self.solder
            .as_deref()
            .filter(|root| !root.trim().is_empty())
    }

    fn zip_url(&self) -> Option<&str> {
        self.url.as_deref().filter(|url| !url.trim().is_empty())
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SolderPack {
    #[serde(default)]
    pub recommended: Option<String>,
    #[serde(default)]
    pub latest: Option<String>,
    #[serde(default)]
    pub builds: Vec<String>,
}

impl SolderPack {
    /// The build a player gets: the author's recommendation, or else the newest.
    pub fn current(&self) -> Option<&str> {
        let named = |build: &Option<String>| build.clone().filter(|name| !name.is_empty());
        let chosen = named(&self.recommended).or_else(|| named(&self.latest));
        self.builds
            .iter()
            .find(|build| Some(*build) == chosen.as_ref())
            .or(self.builds.last())
            .map(String::as_str)
    }
}

#[derive(Debug, Deserialize)]
struct SolderBuild {
    #[serde(default)]
    minecraft: Option<String>,
    #[serde(default)]
    mods: Vec<SolderMod>,
}

#[derive(Debug, Deserialize)]
struct SolderMod {
    url: String,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    filesize: Option<u64>,
}

#[derive(Deserialize)]
struct SearchResults {
    #[serde(default)]
    modpacks: Vec<SearchHit>,
}

#[derive(Deserialize)]
struct SearchHit {
    slug: String,
}

#[derive(Deserialize)]
struct Featured {
    #[serde(default)]
    modpacks: BTreeMap<String, serde_json::Value>,
}

/// One zip of a pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive {
    pub url: String,
    pub md5: Option<String>,
    pub size: Option<u64>,
}

/// What it takes to install a pack's current version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub version: String,
    pub minecraft: Option<String>,
    pub archives: Vec<Archive>,
}

/// What a pack's `bin` folder says, gathered across its zips.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bin {
    pub version_json: Option<String>,
    pub forge_properties: Option<String>,
    pub fml_properties: Option<String>,
    pub has_jar: bool,
    /// Every file in `modpack.jar` and its checksum, for jars without a profile.
    pub jar_files: BTreeMap<String, u32>,
}

impl Bin {
    /// Takes on another zip's files, which win over what's here already.
    pub fn overlay(&mut self, newer: Bin) {
        self.version_json = newer.version_json.or(self.version_json.take());
        self.forge_properties = newer.forge_properties.or(self.forge_properties.take());
        self.fml_properties = newer.fml_properties.or(self.fml_properties.take());
        self.has_jar |= newer.has_jar;
        if !newer.jar_files.is_empty() {
            self.jar_files = newer.jar_files;
        }
    }

    /// Whether this pack's game jar is Forge's own file and nothing more. Packs
    /// from this era could change the game however they liked and still say
    /// they're Forge, and swapping in the real thing would drop those changes.
    pub fn is_plain_forge(&self, forge: &Path) -> Result<bool> {
        let official = jar_files(&mut zip::ZipArchive::new(File::open(forge)?)?)?;
        Ok(self.jar_files == official)
    }
}

/// What's in a jar by name and checksum, leaving out the signatures that differ
/// between otherwise identical builds.
fn jar_files<R: Read + Seek>(jar: &mut zip::ZipArchive<R>) -> Result<BTreeMap<String, u32>> {
    let mut files = BTreeMap::new();
    for i in 0..jar.len() {
        let entry = jar.by_index_raw(i)?;
        let name = entry.name()?;
        if !entry.is_dir() && !name.starts_with("META-INF/") {
            files.insert(name.to_string(), entry.crc32());
        }
    }
    Ok(files)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionFile {
    #[serde(default)]
    id: String,
    #[serde(default)]
    inherits_from: Option<String>,
    #[serde(default)]
    main_class: Option<String>,
    #[serde(default)]
    libraries: Vec<VersionLibrary>,
}

#[derive(Debug, Deserialize)]
struct VersionLibrary {
    name: String,
}

/// Authors host packs wherever they like, so there's no list of good hosts to
/// check against. Instead every address has to work over https, which an older
/// `http` one is upgraded to, and none may reach into the player's own network.
pub fn secure(url: &str) -> Result<String> {
    let refused = || Error::Other(format!("refused an unsafe pack address: {url}"));
    let mut parsed = Url::parse(url.trim()).map_err(|_| refused())?;
    if parsed.scheme() == "http" {
        parsed.set_scheme("https").map_err(|()| refused())?;
    }
    if parsed.scheme() != "https" || !parsed.username().is_empty() || is_local(&parsed) {
        return Err(refused());
    }
    Ok(parsed.into())
}

/// An address under a Solder server. Build names come from the pack, so they go
/// in as path segments, which keeps them from changing where the address goes.
fn solder_url(root: &str, parts: &[&str], query: Option<&str>) -> Result<String> {
    let mut url = Url::parse(&secure(root)?).map_err(|error| Error::Other(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|()| Error::Other(format!("refused an unsafe pack address: {root}")))?
        .pop_if_empty()
        .extend(parts);
    url.set_query(query);
    Ok(url.into())
}

pub async fn pack(net: &Net, slug: &str) -> Result<Pack> {
    net.json(&format!("{API}/modpack/{}?build={BUILD}", plain(slug)?))
        .await
}

async fn solder_json<T: DeserializeOwned>(net: &Net, url: &str) -> Result<T> {
    let mut retries = 0;
    loop {
        match net.json(url).await {
            Err(Error::Status { status: 429, .. }) if retries < SOLDER_RETRIES => {
                retries += 1;
                tokio::time::sleep(SOLDER_WAIT).await;
            }
            answer => return answer,
        }
    }
}

/// A pack's Solder listing, if that's how it's published.
pub async fn solder_pack(net: &Net, pack: &Pack) -> Result<Option<SolderPack>> {
    let Some(root) = pack.solder_root() else {
        return Ok(None);
    };
    let url = solder_url(root, &["modpack", &pack.name], None)?;
    solder_json(net, &url)
        .await
        .map(Some)
        .map_err(|error| unreachable_pack(pack.title(), &url, error))
}

async fn solder_build(net: &Net, pack: &Pack, build: &str, mods: bool) -> Result<SolderBuild> {
    let root = pack.solder_root().unwrap_or_default();
    let query = mods.then_some("include=mods");
    let url = solder_url(root, &["modpack", &pack.name, build], query)?;
    solder_json(net, &url)
        .await
        .map_err(|error| unreachable_pack(pack.title(), &url, error))
}

/// Which build or version of a pack is current.
pub async fn current_version(net: &Net, pack: &Pack) -> Result<String> {
    match solder_pack(net, pack).await? {
        Some(solder) => solder
            .current()
            .map(str::to_string)
            .ok_or_else(|| Error::NotFound(format!("a build of {}", pack.title()))),
        None => Ok(pack.version.clone().unwrap_or_default()),
    }
}

/// Works out which files a pack's current version is made of.
pub async fn plan(net: &Net, pack: &Pack) -> Result<Plan> {
    if pack.solder_root().is_some() {
        let version = current_version(net, pack).await?;
        let build = solder_build(net, pack, &version, true).await?;
        return Ok(Plan {
            version,
            minecraft: build.minecraft,
            archives: build
                .mods
                .into_iter()
                .map(|file| {
                    Ok(Archive {
                        url: secure(&file.url)?,
                        md5: file.md5.filter(|md5| !md5.is_empty()),
                        size: file.filesize.filter(|size| *size > 0),
                    })
                })
                .collect::<Result<_>>()?,
        });
    }
    let url = pack.zip_url().ok_or_else(|| {
        Error::Other(format!(
            "{} has no download, so it can't be installed",
            pack.title()
        ))
    })?;
    Ok(Plan {
        version: pack.version.clone().unwrap_or_default(),
        minecraft: pack.minecraft.clone(),
        archives: vec![Archive {
            url: secure(url)?,
            md5: None,
            size: None,
        }],
    })
}

/// Says plainly what went wrong when a download fails, because an author's own
/// server dropping off or having no https is far more likely than a bug here.
pub fn unreachable_pack(title: &str, url: &str, error: Error) -> Error {
    // Out of a whole pack, the error itself knows which file it was.
    let failed = match &error {
        Error::Status { url, .. } => url.clone(),
        Error::Http(http) => http.url().map_or(url.to_string(), Url::to_string),
        _ => url.to_string(),
    };
    let host = Url::parse(&failed)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string());
    match error {
        Error::Status { status: 429, .. } => Error::Other(format!(
            "{host} is busy, so {title} couldn't be fetched. Try again in a minute"
        )),
        Error::Status { status, .. } => Error::Other(format!(
            "{host} answered with error {status}, so {title} couldn't be fetched. Its author may \
             have moved it"
        )),
        Error::Http(_) => Error::Other(format!(
            "{title} couldn't be fetched securely from {host}. Its server may be down or may not \
             support https"
        )),
        other => other,
    }
}

/// An entry's real name. Packs made on Windows can spell their folders any way.
fn entry_named<R: Read + Seek>(archive: &zip::ZipArchive<R>, name: &str) -> Option<String> {
    archive
        .file_names()
        .flatten()
        .find(|entry| entry.eq_ignore_ascii_case(name))
        .map(|entry| entry.into_owned())
}

fn entry_text<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let name = entry_named(archive, name)?;
    let mut bytes = Vec::new();
    archive.by_name(&name).ok()?.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads what a zip's `bin` folder says about the game, without unpacking it.
pub fn read_bin(archive: &Path) -> Result<Bin> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    let mut bin = Bin {
        version_json: entry_text(&mut zip, "bin/version.json"),
        ..Bin::default()
    };
    let Some(name) = entry_named(&zip, "bin/modpack.jar") else {
        return Ok(bin);
    };
    let entry = zip.by_name(&name)?;
    bin.has_jar = true;
    if entry.size() > MAX_LOADER_JAR {
        return Ok(bin);
    }
    let mut bytes = Vec::new();
    entry.take(MAX_LOADER_JAR).read_to_end(&mut bytes)?;
    // A jar that can't be read says nothing, which `game_of` then refuses.
    if let Ok(mut jar) = zip::ZipArchive::new(Cursor::new(bytes)) {
        bin.version_json = bin
            .version_json
            .or_else(|| entry_text(&mut jar, "version.json"));
        bin.forge_properties = entry_text(&mut jar, "forgeversion.properties");
        bin.fml_properties = entry_text(&mut jar, "fmlversion.properties");
        if bin.version_json.is_none() {
            bin.jar_files = jar_files(&mut jar)?;
        }
    }
    Ok(bin)
}

fn property<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key)
            .then(|| value.trim())
            .filter(|value| !value.is_empty())
    })
}

/// Forge's version, from the properties file old builds carry instead of a profile.
fn forge_build(properties: &str) -> Option<String> {
    let number = |part: &str| property(properties, &format!("forge.{part}.number"));
    Some(format!(
        "{}.{}.{}.{}",
        number("major")?,
        number("minor")?,
        number("revision")?,
        number("build")?
    ))
}

/// Trims a Maven version like `1.7.10-10.13.4.1492-1.7.10` to the loader's own number.
fn loader_number(version: &str, minecraft: &str) -> String {
    let version = version
        .strip_prefix(&format!("{minecraft}-"))
        .unwrap_or(version);
    version
        .strip_suffix(&format!("-{minecraft}"))
        .unwrap_or(version)
        .to_string()
}

/// The loader a profile uses. LiteLoader rides along with Forge in some packs, so
/// it only counts when there's nothing else.
fn loader_in(file: &VersionFile, minecraft: &str) -> Option<Loader> {
    let library = |wanted: &[(&str, &str, LoaderKind)]| {
        file.libraries.iter().find_map(|library| {
            let mut parts = library.name.split(':');
            let (group, artifact, version) = (parts.next()?, parts.next()?, parts.next()?);
            let kind = wanted
                .iter()
                .find(|(g, a, _)| *g == group && *a == artifact)?
                .2;
            Some(Loader {
                kind,
                version: Some(loader_number(version, minecraft)),
            })
        })
    };
    library(&[
        ("net.minecraftforge", "forge", LoaderKind::Forge),
        ("net.minecraftforge", "minecraftforge", LoaderKind::Forge),
        ("net.neoforged", "neoforge", LoaderKind::NeoForge),
        ("net.fabricmc", "fabric-loader", LoaderKind::Fabric),
        ("org.quiltmc", "quilt-loader", LoaderKind::Quilt),
        ("com.cleanroommc", "cleanroom", LoaderKind::Cleanroom),
    ])
    .or_else(|| {
        let (_, version) = file.id.split_once("-forge-")?;
        Some(Loader {
            kind: LoaderKind::Forge,
            version: Some(loader_number(version, minecraft)),
        })
    })
    .or_else(|| library(&[("com.mumfrey", "liteloader", LoaderKind::LiteLoader)]))
}

/// Minecraft's version from a profile, which old ones only put in their id.
fn profile_minecraft(file: &VersionFile) -> Option<String> {
    file.inherits_from.clone().or_else(|| {
        let (minecraft, _) = file.id.split_once('-')?;
        minecraft
            .starts_with(|c: char| c.is_ascii_digit())
            .then(|| minecraft.to_string())
    })
}

/// What a pack gets told when its game jar is more than a loader.
pub fn patches_the_game(title: &str) -> Error {
    Error::Other(format!(
        "{title} patches the game's own files in a way Waymark doesn't support yet"
    ))
}

/// Reads the Minecraft version and loader a pack uses, or says why it can't be set up.
pub fn game_of(title: &str, stated: Option<&str>, bin: &Bin) -> Result<(String, Loader)> {
    let file = bin
        .version_json
        .as_deref()
        .and_then(|text| serde_json::from_str::<VersionFile>(text).ok());
    let minecraft = file
        .as_ref()
        .and_then(profile_minecraft)
        .or_else(|| {
            let fml = bin.fml_properties.as_deref()?;
            Some(property(fml, "fmlbuild.mcversion")?.to_string())
        })
        .or_else(|| stated.map(str::to_string))
        .filter(|minecraft| !minecraft.is_empty())
        .ok_or_else(|| Error::Other(format!("{title} doesn't say what game it's for")))?;

    if let Some(file) = &file {
        if let Some(loader) = loader_in(file, &minecraft) {
            return Ok((minecraft, loader));
        }
        let vanilla = file
            .main_class
            .as_deref()
            .is_none_or(|main| main.starts_with("net.minecraft.client."));
        if !vanilla {
            return Err(Error::Other(format!(
                "{title} uses a loader Waymark doesn't recognise"
            )));
        }
    }
    if let Some(version) = bin.forge_properties.as_deref().and_then(forge_build) {
        let forge = Loader {
            kind: LoaderKind::Forge,
            version: Some(version),
        };
        return Ok((minecraft, forge));
    }
    if bin.has_jar && file.is_none() {
        return Err(patches_the_game(title));
    }
    Ok((minecraft, Loader::default()))
}

fn hit(pack: &Pack) -> ContentHit {
    ContentHit {
        source: ContentSource::Technic,
        id: pack.name.clone(),
        slug: pack.name.clone(),
        title: pack.title().to_string(),
        summary: first_line(pack),
        author: pack.user.clone().unwrap_or_default(),
        downloads: pack.installs.unwrap_or(0),
        icon: pack.icon_url(),
        banner: None,
        kind: ContentKind::Modpack,
        categories: Vec::new(),
    }
}

fn first_line(pack: &Pack) -> String {
    pack.description
        .as_deref()
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The Minecraft a pack really uses. The platform's own field is often years out
/// of date, so a pack on Solder is asked directly.
pub async fn real_minecraft(net: &Net, pack: &Pack) -> Option<String> {
    if pack.solder_root().is_none() {
        return pack.minecraft.clone();
    }
    let version = current_version(net, pack).await.ok()?;
    solder_build(net, pack, &version, false)
        .await
        .ok()?
        .minecraft
}

pub async fn search(net: &Net, query: &SearchQuery) -> Result<ContentPage> {
    if query.kind != ContentKind::Modpack {
        return Ok(ContentPage {
            hits: Vec::new(),
            total: 0,
        });
    }
    let text = query.text.trim();
    let slugs: Vec<String> = if text.is_empty() {
        let featured: Featured = net.json(FEATURED).await?;
        featured.modpacks.into_keys().collect()
    } else {
        let found: SearchResults = net
            .client()
            .get(format!("{API}/search"))
            .query(&[("build", BUILD), ("q", text)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        found.modpacks.into_iter().map(|pack| pack.slug).collect()
    };
    let total = slugs.len() as u64;
    let page: Vec<&String> = slugs
        .iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();
    let found = futures_util::future::join_all(page.iter().map(|slug| pack(net, slug))).await;
    let packs: Vec<Pack> = found.into_iter().flatten().collect();
    let mut hits = Vec::with_capacity(packs.len());
    for pack in &packs {
        let fits = match &query.minecraft {
            Some(wanted) => real_minecraft(net, pack).await.as_ref() == Some(wanted),
            None => true,
        };
        if fits {
            hits.push(hit(pack));
        }
    }
    Ok(ContentPage { hits, total })
}

pub fn page(pack: &Pack, minecraft: Option<String>) -> ProjectPage {
    let site = pack
        .platform_url
        .clone()
        .filter(|url| url.starts_with("https://"));
    ProjectPage {
        source: ContentSource::Technic,
        id: pack.name.clone(),
        slug: pack.name.clone(),
        kind: ContentKind::Modpack,
        title: pack.title().to_string(),
        summary: first_line(pack),
        body: pack.description.clone().unwrap_or_default(),
        format: BodyFormat::Markdown,
        icon: pack.icon_url(),
        downloads: pack.installs.unwrap_or(0),
        followers: 0,
        published: None,
        updated: None,
        categories: Vec::new(),
        loaders: Vec::new(),
        game_versions: minecraft.into_iter().collect(),
        client: None,
        server: None,
        license: None,
        links: site
            .iter()
            .map(|url| Link {
                label: "Technic".into(),
                url: url.clone(),
            })
            .collect(),
        gallery: Vec::new(),
        creators: pack
            .user
            .iter()
            .map(|name| Creator {
                name: name.clone(),
                role: None,
                avatar: None,
                url: None,
            })
            .collect(),
        page: site.unwrap_or_default(),
    }
}

fn row(version: &str, recommended: bool, minecraft: Option<String>) -> VersionRow {
    VersionRow {
        id: version.to_string(),
        name: version.to_string(),
        number: version.to_string(),
        channel: if recommended { "release" } else { "beta" }.to_string(),
        published: String::new(),
        downloads: 0,
        loaders: Vec::new(),
        game_versions: minecraft.into_iter().collect(),
        file: String::new(),
        size: 0,
        available: true,
    }
}

/// Newest first, to match the other stores. Solder lists builds oldest first.
/// Only the current build's Minecraft is looked up, because asking for every
/// build gets a player turned away by the server.
pub async fn rows(net: &Net, pack: &Pack) -> Result<Vec<VersionRow>> {
    let Some(solder) = solder_pack(net, pack).await? else {
        let version = pack.version.clone().unwrap_or_default();
        return Ok(vec![row(&version, true, pack.minecraft.clone())]);
    };
    let Some(current) = solder.current() else {
        return Ok(Vec::new());
    };
    let minecraft = solder_build(net, pack, current, false)
        .await
        .ok()
        .and_then(|details| details.minecraft);
    Ok(solder
        .builds
        .iter()
        .rev()
        .map(|build| {
            let chosen = build == current;
            row(build, chosen, minecraft.clone().filter(|_| chosen))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn jar(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
        for (name, text) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        bytes
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn bin(version_json: &str) -> Bin {
        Bin {
            version_json: Some(version_json.to_string()),
            has_jar: true,
            ..Bin::default()
        }
    }

    #[test]
    fn addresses_are_upgraded_or_refused() {
        assert_eq!(
            secure("http://apocgaming.net/mp/AS1.zip").unwrap(),
            "https://apocgaming.net/mp/AS1.zip"
        );
        assert_eq!(
            secure("https://www.dropbox.com/s/a/b.zip?dl=1").unwrap(),
            "https://www.dropbox.com/s/a/b.zip?dl=1"
        );
        for unsafe_address in [
            "ftp://example.com/pack.zip",
            "file:///C:/Windows/win.ini",
            "not an address",
            "https://user:pass@example.com/pack.zip",
        ] {
            assert!(secure(unsafe_address).is_err(), "{unsafe_address}");
        }
    }

    #[test]
    fn a_pack_cannot_reach_the_players_own_network() {
        for local in [
            "http://localhost/pack.zip",
            "https://LOCALHOST:8080/pack.zip",
            "https://printer.local/pack.zip",
            "http://127.0.0.1/pack.zip",
            "http://2130706433/pack.zip",
            "http://0x7f.1/pack.zip",
            "https://10.0.0.1/pack.zip",
            "https://192.168.1.1/pack.zip",
            "https://172.16.0.1/pack.zip",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/pack.zip",
            "https://[fd00::1]/pack.zip",
            "https://[::ffff:127.0.0.1]/pack.zip",
        ] {
            assert!(secure(local).is_err(), "{local}");
        }
        assert!(secure("https://185.151.174.65:25589/pack.zip").is_ok());
    }

    #[test]
    fn a_failed_fetch_says_what_really_happened() {
        let url = "https://solder.example.com/api/modpack/pack";
        let told = |status| {
            unreachable_pack(
                "Pack",
                url,
                Error::Status {
                    url: url.to_string(),
                    status,
                },
            )
            .to_string()
        };
        assert!(told(429).contains("busy"));
        assert!(told(404).contains("404") && told(404).contains("solder.example.com"));
        assert!(!told(429).contains("https") && !told(404).contains("https"));
        let elsewhere = unreachable_pack(
            "Pack",
            url,
            Error::Status {
                url: "https://files.example.org/mod.zip".to_string(),
                status: 500,
            },
        );
        assert!(elsewhere.to_string().contains("files.example.org"));
        let unrelated = unreachable_pack("Pack", url, Error::NotFound("x".into()));
        assert!(matches!(unrelated, Error::NotFound(_)));
    }

    #[test]
    fn build_names_cannot_change_where_an_address_goes() {
        let root = "https://solder.example.com/api/";
        assert_eq!(
            solder_url(root, &["modpack", "pack", "1.0"], Some("include=mods")).unwrap(),
            "https://solder.example.com/api/modpack/pack/1.0?include=mods"
        );
        let hostile = solder_url(root, &["modpack", "pack", "../../evil?x=1"], None).unwrap();
        assert!(hostile.starts_with("https://solder.example.com/api/modpack/pack/"));
        assert!(!hostile.contains('?'));
        assert!(solder_url("http://localhost/api/", &["modpack"], None).is_err());
    }

    #[test]
    fn the_recommended_build_is_the_current_one() {
        let solder = |recommended: Option<&str>, latest: Option<&str>| SolderPack {
            recommended: recommended.map(str::to_string),
            latest: latest.map(str::to_string),
            builds: vec!["1.0".into(), "1.1".into(), "2.0".into()],
        };
        assert_eq!(solder(Some("1.1"), Some("2.0")).current(), Some("1.1"));
        assert_eq!(solder(None, Some("2.0")).current(), Some("2.0"));
        assert_eq!(solder(Some(""), Some("1.0")).current(), Some("1.0"));
        assert_eq!(solder(None, None).current(), Some("2.0"));
        // A name that isn't among the builds falls back rather than being trusted.
        assert_eq!(solder(Some("../x"), None).current(), Some("2.0"));
        assert_eq!(SolderPack::default().current(), None);
    }

    #[test]
    fn a_modern_forge_installer_is_read_not_run() {
        let dir = tempfile::tempdir().unwrap();
        let profile = r#"{"id":"1.12.2-forge-14.23.5.2860","inheritsFrom":"1.12.2","mainClass":"net.minecraft.launchwrapper.Launch","libraries":[{"name":"net.minecraftforge:forge:1.12.2-14.23.5.2860"}]}"#;
        let path = dir.path().join("forge.zip");
        write_zip(
            &path,
            &[(
                "bin/modpack.jar",
                &jar(&[("version.json", profile), ("install_profile.json", "{}")]),
            )],
        );
        let (minecraft, loader) =
            game_of("Pack", Some("1.12.2"), &read_bin(&path).unwrap()).unwrap();
        assert_eq!(minecraft, "1.12.2");
        assert_eq!(loader.kind, LoaderKind::Forge);
        assert_eq!(loader.version.as_deref(), Some("14.23.5.2860"));
    }

    #[test]
    fn forge_versions_lose_their_minecraft_suffix() {
        let old = bin(
            r#"{"id":"1.7.10-Forge10.13.4.1492-1.7.10","inheritsFrom":"1.7.10","mainClass":"net.minecraft.launchwrapper.Launch","libraries":[{"name":"net.minecraftforge:forge:1.7.10-10.13.4.1492-1.7.10"}]}"#,
        );
        assert_eq!(
            game_of("Pack", Some("1.7.10"), &old)
                .unwrap()
                .1
                .version
                .as_deref(),
            Some("10.13.4.1492")
        );
    }

    #[test]
    fn forge_1_6_profiles_name_their_library_differently() {
        let old = bin(
            r#"{"id":"1.6.4-Forge9.11.1.965","mainClass":"net.minecraft.launchwrapper.Launch","libraries":[{"name":"net.minecraftforge:minecraftforge:9.11.1.965"},{"name":"net.minecraft:launchwrapper:1.8"}]}"#,
        );
        let (minecraft, loader) = game_of("Pack", None, &old).unwrap();
        assert_eq!(minecraft, "1.6.4");
        assert_eq!(loader.version.as_deref(), Some("9.11.1.965"));
    }

    #[test]
    fn a_stale_platform_version_loses_to_what_the_pack_says() {
        let pack = bin(
            r#"{"id":"x","inheritsFrom":"1.20.1","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{"name":"net.fabricmc:fabric-loader:0.14.21"}]}"#,
        );
        let (minecraft, loader) = game_of("Pack", Some("1.4.7"), &pack).unwrap();
        assert_eq!(minecraft, "1.20.1");
        assert_eq!(loader.kind, LoaderKind::Fabric);
    }

    #[test]
    fn forge_wins_over_liteloader_riding_along() {
        let pack = bin(
            r#"{"id":"x","inheritsFrom":"1.7.10","mainClass":"net.minecraft.launchwrapper.Launch","libraries":[{"name":"com.mumfrey:liteloader:1.7.10"},{"name":"net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10"}]}"#,
        );
        assert_eq!(
            game_of("Pack", None, &pack).unwrap().1.kind,
            LoaderKind::Forge
        );
    }

    #[test]
    fn jar_era_forge_is_read_from_its_properties() {
        let pack = Bin {
            forge_properties: Some(
                "forge.major.number=7\nforge.minor.number=8\nforge.revision.number=1\nforge.build.number=737\n"
                    .into(),
            ),
            fml_properties: Some("fmlbuild.mcversion=1.5.2\n".into()),
            has_jar: true,
            ..Bin::default()
        };
        let (minecraft, loader) = game_of("Hexxit", Some("1.4.7"), &pack).unwrap();
        assert_eq!(minecraft, "1.5.2");
        assert_eq!(loader.kind, LoaderKind::Forge);
        assert_eq!(loader.version.as_deref(), Some("7.8.1.737"));
    }

    #[test]
    fn a_game_jar_is_only_plain_forge_if_every_file_matches() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path().join("forge.zip");
        fs::write(
            &forge,
            jar(&[("a.class", "one"), ("META-INF/FORGE.SF", "signature")]),
        )
        .unwrap();
        let read = |entries: &[(&str, &str)]| {
            let path = dir.path().join("pack.zip");
            write_zip(&path, &[("bin/modpack.jar", &jar(entries))]);
            read_bin(&path).unwrap()
        };
        assert!(read(&[("a.class", "one")]).is_plain_forge(&forge).unwrap());
        // Big Dig's jar says it's Forge, but has classes Forge never shipped.
        let extra = read(&[("a.class", "one"), ("extra.class", "two")]);
        assert!(!extra.is_plain_forge(&forge).unwrap());
        let changed = read(&[("a.class", "two")]);
        assert!(!changed.is_plain_forge(&forge).unwrap());
    }

    #[test]
    fn a_custom_game_jar_is_refused_rather_than_broken() {
        let pack = Bin {
            fml_properties: Some("fmlbuild.mcclientversion=1.2.5\n".into()),
            has_jar: true,
            ..Bin::default()
        };
        let error = game_of("Tekkit Classic", Some("1.2.5"), &pack).unwrap_err();
        assert!(error.to_string().contains("Tekkit Classic"));
    }

    #[test]
    fn an_unknown_loader_is_refused_by_name() {
        let pack = bin(
            r#"{"id":"x","inheritsFrom":"1.16.5","mainClass":"com.example.Mystery","libraries":[]}"#,
        );
        assert!(game_of("Pack", None, &pack).is_err());
    }

    #[test]
    fn a_pack_with_no_loader_is_plain_minecraft() {
        let (minecraft, loader) = game_of("Vanilla", Some("26.3"), &Bin::default()).unwrap();
        assert_eq!(minecraft, "26.3");
        assert_eq!(loader.kind, LoaderKind::Vanilla);
        assert!(game_of("Pack", None, &Bin::default()).is_err());
    }

    #[test]
    fn the_first_zip_in_a_pack_wins() {
        let mut gathered = Bin::default();
        gathered.overlay(bin("later"));
        gathered.overlay(Bin {
            version_json: Some("earlier".into()),
            ..Bin::default()
        });
        assert_eq!(gathered.version_json.as_deref(), Some("earlier"));
        assert!(gathered.has_jar);
    }

    #[test]
    fn packs_zipped_on_windows_can_spell_bin_differently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.zip");
        write_zip(
            &path,
            &[(
                "Bin/ModPack.jar",
                &jar(&[("forgeversion.properties", "forge.major.number=7")]),
            )],
        );
        let read = read_bin(&path).unwrap();
        assert!(read.has_jar && read.forge_properties.is_some());
    }

    #[test]
    fn a_broken_jar_says_nothing_rather_than_failing_the_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.zip");
        write_zip(&path, &[("bin/modpack.jar", b"not a zip")]);
        let read = read_bin(&path).unwrap();
        assert!(read.has_jar && read.version_json.is_none());
    }
}

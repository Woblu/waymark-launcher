//! Worlds and servers, read straight from the game's own files. A world's
//! `level.dat` is gzipped NBT and `servers.dat` is plain NBT. Fields the game
//! adds later are kept, because the server list is rewritten rather than
//! replaced, and a world that can't be read is still listed by its folder name.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use fastnbt::Value;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use walkdir::WalkDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::Core;
use crate::error::{Error, Result};
use crate::install::safe_relative;

const LEVEL_FILE: &str = "level.dat";
const SERVERS_FILE: &str = "servers.dat";
const DEFAULT_PORT: u16 = 25565;
const PING_TIMEOUT: Duration = Duration::from_secs(4);
/// More than any real status reply, so a bad server can't exhaust memory.
const MAX_STATUS_BYTES: usize = 1 << 20;
/// Any protocol number works for a status request.
const STATUS_PROTOCOL: i32 = -1;
const STATUS_STATE: i32 = 1;
const RECENT_WORLDS: usize = 4;

#[derive(Deserialize)]
struct LevelFile {
    #[serde(rename = "Data")]
    data: LevelData,
}

#[derive(Default, Deserialize)]
struct LevelData {
    #[serde(rename = "LevelName", default)]
    name: Option<String>,
    #[serde(rename = "LastPlayed", default)]
    last_played: Option<i64>,
    #[serde(rename = "GameType", default)]
    game_type: Option<i32>,
    #[serde(default)]
    hardcore: Option<i8>,
    #[serde(rename = "Version", default)]
    version: Option<LevelVersion>,
}

#[derive(Deserialize)]
struct LevelVersion {
    #[serde(rename = "Name", default)]
    name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct World {
    pub folder: String,
    pub name: String,
    /// Milliseconds since 1970, as the game writes it.
    pub last_played: Option<i64>,
    pub mode: &'static str,
    pub version: Option<String>,
    pub has_icon: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worlds {
    pub worlds: Vec<World>,
    /// Whether this version can open a world straight from the launcher.
    pub can_join: bool,
}

/// A world somewhere in the library, for jumping straight back in.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentWorld {
    pub instance: String,
    pub instance_name: String,
    #[serde(flatten)]
    pub world: World,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedServer {
    pub index: usize,
    pub name: String,
    pub address: String,
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub online: bool,
    pub players: Option<u32>,
    pub capacity: Option<u32>,
    pub version: Option<String>,
    pub motd: String,
    pub latency_ms: Option<u32>,
    pub icon: Option<String>,
}

/// Where to go straight after launching.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Join {
    World { folder: String },
    Server { address: String },
}

/// Quick Play arrived in snapshot 23w14a, which became 1.20.
pub fn supports_quick_play(minecraft: &str) -> bool {
    let mut parts = minecraft
        .split(['.', 'w', '-'])
        .map(|part| part.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(1), Some(minor)) => minor >= 20,
        (Some(1), None) => false,
        (Some(major), _) => major >= 23,
        _ => false,
    }
}

/// The game's arguments for going straight somewhere. Older versions can still
/// join a server, using the options they've always had.
pub fn join_arguments(join: &Join, minecraft: &str) -> Result<Vec<String>> {
    let quick = supports_quick_play(minecraft);
    Ok(match join {
        Join::World { folder } if quick => vec!["--quickPlaySingleplayer".into(), folder.clone()],
        Join::World { .. } => {
            return Err(Error::Other(
                "opening a world directly needs Minecraft 1.20 or newer".into(),
            ));
        }
        Join::Server { address } if quick => vec!["--quickPlayMultiplayer".into(), address.clone()],
        Join::Server { address } => {
            let (host, port) = split_address(address);
            vec!["--server".into(), host, "--port".into(), port.to_string()]
        }
    })
}

/// Splits `host:port`, leaving a bare IPv6 address whole.
pub fn split_address(address: &str) -> (String, u16) {
    let address = address.trim();
    if let Some(rest) = address.strip_prefix('[')
        && let Some((host, after)) = rest.split_once(']')
    {
        let port = after.strip_prefix(':').and_then(|p| p.parse().ok());
        return (host.to_string(), port.unwrap_or(DEFAULT_PORT));
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            (host.to_string(), port.parse().unwrap_or(DEFAULT_PORT))
        }
        _ => (address.to_string(), DEFAULT_PORT),
    }
}

fn mode_name(game_type: Option<i32>, hardcore: bool) -> &'static str {
    match game_type {
        _ if hardcore => "Hardcore",
        Some(1) => "Creative",
        Some(2) => "Adventure",
        Some(3) => "Spectator",
        _ => "Survival",
    }
}

fn read_level(path: &Path) -> Option<LevelData> {
    let bytes = fs::read(path).ok()?;
    let mut text = Vec::new();
    GzDecoder::new(&bytes[..]).read_to_end(&mut text).ok()?;
    fastnbt::from_bytes::<LevelFile>(&text)
        .ok()
        .map(|file| file.data)
}

fn read_world(folder: &Path) -> Option<World> {
    let level = folder.join(LEVEL_FILE);
    if !level.is_file() {
        return None;
    }
    let name = folder.file_name()?.to_string_lossy().into_owned();
    let data = read_level(&level).unwrap_or_default();
    Some(World {
        name: data
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| name.clone()),
        last_played: data.last_played.filter(|at| *at > 0),
        mode: mode_name(data.game_type, data.hardcore.unwrap_or(0) != 0),
        version: data.version.and_then(|v| v.name),
        has_icon: folder.join("icon.png").is_file(),
        folder: name,
    })
}

fn newest_first(worlds: &mut [World]) {
    worlds.sort_by_key(|world| std::cmp::Reverse(world.last_played.unwrap_or(0)));
}

fn read_servers(path: &Path) -> Result<Vec<HashMap<String, Value>>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let root: Value = fastnbt::from_bytes(&bytes)
        .map_err(|error| Error::Other(format!("the server list can't be read: {error}")))?;
    let Value::Compound(mut root) = root else {
        return Ok(Vec::new());
    };
    let Some(Value::List(list)) = root.remove("servers") else {
        return Ok(Vec::new());
    };
    Ok(list
        .into_iter()
        .filter_map(|server| {
            if let Value::Compound(fields) = server {
                Some(fields)
            } else {
                None
            }
        })
        .collect())
}

fn write_servers(path: &Path, servers: Vec<HashMap<String, Value>>) -> Result<()> {
    let list = Value::List(servers.into_iter().map(Value::Compound).collect());
    let root = Value::Compound(HashMap::from([("servers".to_string(), list)]));
    let bytes = fastnbt::to_bytes(&root)
        .map_err(|error| Error::Other(format!("the server list can't be saved: {error}")))?;
    let part = path.with_extension("dat.part");
    fs::write(&part, bytes)?;
    fs::rename(part, path)?;
    Ok(())
}

fn text_field(fields: &HashMap<String, Value>, name: &str) -> Option<String> {
    match fields.get(name) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn is_hidden(fields: &HashMap<String, Value>) -> bool {
    matches!(fields.get("hidden"), Some(Value::Byte(1)))
}

fn new_server(name: &str, address: &str) -> HashMap<String, Value> {
    HashMap::from([
        ("name".to_string(), Value::String(name.to_string())),
        ("ip".to_string(), Value::String(address.to_string())),
    ])
}

fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

async fn read_varint<R: AsyncReadExt + Unpin>(stream: &mut R) -> Result<i32> {
    let mut value = 0_u32;
    for shift in 0..5 {
        let byte = stream.read_u8().await?;
        value |= u32::from(byte & 0x7f) << (7 * shift);
        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err(Error::Other("the server sent a bad number".into()))
}

fn packet(id: i32, body: &[u8]) -> Vec<u8> {
    let mut inner = Vec::with_capacity(body.len() + 2);
    write_varint(&mut inner, id);
    inner.extend_from_slice(body);
    let mut out = Vec::with_capacity(inner.len() + 3);
    write_varint(&mut out, inner.len() as i32);
    out.extend_from_slice(&inner);
    out
}

fn handshake(host: &str, port: u16) -> Vec<u8> {
    let mut body = Vec::with_capacity(host.len() + 8);
    write_varint(&mut body, STATUS_PROTOCOL);
    write_varint(&mut body, host.len() as i32);
    body.extend_from_slice(host.as_bytes());
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, STATUS_STATE);
    packet(0, &body)
}

/// A message of the day can be plain text or nested chat components.
fn chat_text(value: &Json, out: &mut String) {
    match value {
        Json::String(text) => out.push_str(text),
        Json::Array(parts) => parts.iter().for_each(|part| chat_text(part, out)),
        Json::Object(fields) => {
            if let Some(text) = fields.get("text") {
                chat_text(text, out);
            }
            if let Some(extra) = fields.get("extra") {
                chat_text(extra, out);
            }
        }
        _ => {}
    }
}

/// Removes the section-sign colour codes old servers use.
fn plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '§' {
            characters.next();
        } else {
            out.push(character);
        }
    }
    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn status_from(reply: &Json, latency_ms: u32) -> ServerStatus {
    let mut motd = String::new();
    if let Some(description) = reply.get("description") {
        chat_text(description, &mut motd);
    }
    let count = |key: &str| {
        reply
            .get("players")?
            .get(key)?
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
    };
    ServerStatus {
        online: true,
        players: count("online"),
        capacity: count("max"),
        version: reply
            .get("version")
            .and_then(|v| v.get("name"))
            .and_then(Json::as_str)
            .map(plain_text),
        motd: plain_text(&motd),
        latency_ms: Some(latency_ms),
        icon: reply
            .get("favicon")
            .and_then(Json::as_str)
            .filter(|icon| icon.starts_with("data:image/png;base64,"))
            .map(String::from),
    }
}

/// Servers on the default port often move it behind an SRV record, which is
/// what the game itself checks first.
async fn resolve(host: &str, port: u16) -> (String, u16) {
    if port != DEFAULT_PORT || host.parse::<std::net::IpAddr>().is_ok() {
        return (host.to_string(), port);
    }
    let Ok(builder) = hickory_resolver::TokioResolver::builder_tokio() else {
        return (host.to_string(), port);
    };
    let name = format!("_minecraft._tcp.{host}.");
    match builder.build().srv_lookup(name).await {
        Ok(found) => found.iter().next().map_or_else(
            || (host.to_string(), port),
            |record| {
                (
                    record
                        .target()
                        .to_string()
                        .trim_end_matches('.')
                        .to_string(),
                    record.port(),
                )
            },
        ),
        Err(_) => (host.to_string(), port),
    }
}

async fn ping(host: &str, port: u16) -> Result<ServerStatus> {
    let (target, target_port) = resolve(host, port).await;
    let started = Instant::now();
    let mut stream = TcpStream::connect((target.as_str(), target_port)).await?;
    let latency = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    stream.write_all(&handshake(host, port)).await?;
    stream.write_all(&packet(0, &[])).await?;

    let length = read_varint(&mut stream).await?;
    let id = read_varint(&mut stream).await?;
    let size = read_varint(&mut stream).await?;
    if id != 0 || length < 0 || size < 0 || size as usize > MAX_STATUS_BYTES {
        return Err(Error::Other("the server sent something unexpected".into()));
    }
    let mut text = vec![0; size as usize];
    stream.read_exact(&mut text).await?;
    Ok(status_from(&serde_json::from_slice(&text)?, latency))
}

impl Core {
    pub fn worlds(&self, id: &str) -> Result<Worlds> {
        let instance = self.instance(id)?;
        let saves = self.paths.game(id).join("saves");
        let mut worlds = Vec::new();
        if let Ok(entries) = fs::read_dir(saves) {
            worlds.extend(entries.filter_map(|entry| read_world(&entry.ok()?.path())));
        }
        newest_first(&mut worlds);
        Ok(Worlds {
            worlds,
            can_join: supports_quick_play(&instance.minecraft),
        })
    }

    /// The most recently played worlds across every instance that can open one directly.
    pub fn recent_worlds(&self) -> Result<Vec<RecentWorld>> {
        let mut recent = Vec::new();
        for instance in self.instances()? {
            if !supports_quick_play(&instance.minecraft) {
                continue;
            }
            for world in self
                .worlds(&instance.id)?
                .worlds
                .into_iter()
                .take(RECENT_WORLDS)
            {
                recent.push(RecentWorld {
                    instance: instance.id.clone(),
                    instance_name: instance.name.clone(),
                    world,
                });
            }
        }
        recent.sort_by_key(|item| std::cmp::Reverse(item.world.last_played.unwrap_or(0)));
        recent.truncate(RECENT_WORLDS);
        Ok(recent)
    }

    fn world_path(&self, id: &str, folder: &str) -> Result<std::path::PathBuf> {
        if safe_relative(folder)?.components().count() != 1 {
            return Err(Error::NotFound(format!("the world {folder}")));
        }
        let path = self.paths.game(id).join("saves").join(folder);
        if !path.is_dir() {
            return Err(Error::NotFound(format!("the world {folder}")));
        }
        Ok(path)
    }

    pub fn delete_world(&self, id: &str, folder: &str) -> Result<crate::trash::Trashed> {
        if self.game_running(id) {
            return Err(Error::Other(
                "close the game before deleting a world".into(),
            ));
        }
        let path = self.world_path(id, folder)?;
        let name = read_world(&path).map_or_else(|| folder.to_string(), |world| world.name);
        self.send_to_trash(&path, crate::trash::TrashKind::World, &name, Some(id))
    }

    /// Zips a world beside the instance's game files, in a `backups` folder.
    pub fn back_up_world(&self, id: &str, folder: &str) -> Result<std::path::PathBuf> {
        let world = self.world_path(id, folder)?;
        let backups = self.paths.game(id).join("backups");
        fs::create_dir_all(&backups)?;
        let stamp = OffsetDateTime::now_utc().unix_timestamp();
        let target = backups.join(format!("{folder}-{stamp}.zip"));
        let part = target.with_extension("zip.part");

        let mut zip = ZipWriter::new(fs::File::create(&part)?);
        let options = SimpleFileOptions::default();
        for entry in WalkDir::new(&world)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            // The game holds this one open, and a world opens fine without it.
            if entry.file_name() == "session.lock" || !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&world)
                .map_err(|e| Error::Other(e.to_string()))?;
            let name = Path::new(folder)
                .join(relative)
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options)
                .map_err(|e| Error::Other(e.to_string()))?;
            zip.write_all(&fs::read(entry.path())?)?;
        }
        zip.finish().map_err(|e| Error::Other(e.to_string()))?;
        fs::rename(part, &target)?;
        Ok(backups)
    }

    pub fn servers(&self, id: &str) -> Result<Vec<SavedServer>> {
        let list = read_servers(&self.paths.game(id).join(SERVERS_FILE))?;
        Ok(list
            .iter()
            .enumerate()
            .filter(|(_, fields)| !is_hidden(fields))
            .map(|(index, fields)| SavedServer {
                index,
                name: text_field(fields, "name").unwrap_or_default(),
                address: text_field(fields, "ip").unwrap_or_default(),
                icon: text_field(fields, "icon"),
            })
            .collect())
    }

    /// Changes a list the game also writes, so only with the game closed.
    fn change_servers(
        &self,
        id: &str,
        change: impl FnOnce(&mut Vec<HashMap<String, Value>>) -> Result<()>,
    ) -> Result<Vec<SavedServer>> {
        if self.game_running(id) {
            return Err(Error::Other(
                "close the game before changing the server list".into(),
            ));
        }
        let path = self.paths.game(id).join(SERVERS_FILE);
        let mut list = read_servers(&path)?;
        change(&mut list)?;
        write_servers(&path, list)?;
        self.servers(id)
    }

    pub fn add_server(&self, id: &str, name: &str, address: &str) -> Result<Vec<SavedServer>> {
        self.change_servers(id, |list| {
            list.push(new_server(name, address));
            Ok(())
        })
    }

    pub fn edit_server(
        &self,
        id: &str,
        index: usize,
        name: &str,
        address: &str,
    ) -> Result<Vec<SavedServer>> {
        self.change_servers(id, |list| {
            let fields = list
                .get_mut(index)
                .ok_or_else(|| Error::NotFound("that server".into()))?;
            if text_field(fields, "ip").as_deref() != Some(address) {
                // The picture belongs to the old address.
                fields.remove("icon");
            }
            fields.extend(new_server(name, address));
            Ok(())
        })
    }

    pub fn remove_server(&self, id: &str, index: usize) -> Result<Vec<SavedServer>> {
        self.change_servers(id, |list| {
            if index >= list.len() {
                return Err(Error::NotFound("that server".into()));
            }
            list.remove(index);
            Ok(())
        })
    }

    /// Asks a server how it's doing. A server that doesn't answer is just offline.
    pub async fn server_status(&self, address: &str) -> ServerStatus {
        let (host, port) = split_address(address);
        match tokio::time::timeout(PING_TIMEOUT, ping(&host, port)).await {
            Ok(Ok(status)) => status,
            _ => ServerStatus::default(),
        }
    }

    /// Reads a picture for the UI. Each part of the path comes from a web
    /// page's request, so anything that isn't a plain file name is refused.
    pub fn local_image(&self, path: &str) -> Option<Vec<u8>> {
        let parts: Vec<&str> = path.split('/').collect();
        let plain = |part: &&str| safe_relative(part).is_ok_and(|p| p.components().count() == 1);
        if !parts.iter().all(plain) {
            return None;
        }
        let file = match parts.as_slice() {
            [hash] => return self.cached_icon(hash),
            ["world", instance, folder] => self
                .paths
                .game(instance)
                .join("saves")
                .join(folder)
                .join("icon.png"),
            ["instance", instance] => self
                .paths
                .instance(instance)
                .join(crate::instance::ICON_FILE),
            ["screenshot", instance, file] => {
                self.paths.game(instance).join("screenshots").join(file)
            }
            ["skin", file] => self.paths.skins().join(file),
            _ => return None,
        };
        let lower = file.extension()?.to_string_lossy().to_ascii_lowercase();
        if !matches!(lower.as_str(), "png" | "jpg" | "jpeg") {
            return None;
        }
        fs::read(file).ok()
    }
}

/// Used by tests and by anything that needs a `level.dat` of its own.
#[cfg(test)]
fn gzip_level(name: &str) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    let data = HashMap::from([
        ("LevelName".to_string(), Value::String(name.to_string())),
        ("LastPlayed".to_string(), Value::Long(1_700_000_000_000)),
        ("GameType".to_string(), Value::Int(1)),
        ("hardcore".to_string(), Value::Byte(0)),
        (
            "Version".to_string(),
            Value::Compound(HashMap::from([(
                "Name".to_string(),
                Value::String("1.21.4".to_string()),
            )])),
        ),
        ("SomethingNew".to_string(), Value::Int(7)),
    ]);
    let root = Value::Compound(HashMap::from([("Data".to_string(), Value::Compound(data))]));
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&fastnbt::to_bytes(&root).unwrap())
        .unwrap();
    encoder.finish().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, Core, String) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Test", "1.21.4", Loader::default(), "1.21.4".into(), None)
            .unwrap();
        (dir, core, instance.id)
    }

    fn add_world(core: &Core, id: &str, folder: &str, name: &str) {
        let path = core.paths.game(id).join("saves").join(folder);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(LEVEL_FILE), gzip_level(name)).unwrap();
    }

    #[test]
    fn reads_a_world_and_ignores_fields_it_does_not_know() {
        let (_dir, core, id) = setup();
        add_world(&core, &id, "New World", "My Base");
        let found = core.worlds(&id).unwrap();
        assert!(found.can_join);
        let world = &found.worlds[0];
        assert_eq!(world.name, "My Base");
        assert_eq!(world.folder, "New World");
        assert_eq!(world.mode, "Creative");
        assert_eq!(world.version.as_deref(), Some("1.21.4"));
        assert_eq!(world.last_played, Some(1_700_000_000_000));
    }

    #[test]
    fn a_damaged_world_is_still_listed_by_its_folder() {
        let (_dir, core, id) = setup();
        let path = core.paths.game(&id).join("saves/Broken");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(LEVEL_FILE), b"not nbt").unwrap();
        fs::create_dir_all(core.paths.game(&id).join("saves/NotAWorld")).unwrap();
        let worlds = core.worlds(&id).unwrap().worlds;
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].name, "Broken");
    }

    #[test]
    fn quick_play_needs_a_recent_version() {
        for version in ["1.20", "1.20.1", "1.21.4", "26.3", "23w14a", "25w31a"] {
            assert!(supports_quick_play(version), "{version}");
        }
        for version in ["1.19.4", "1.16.1", "1.8.9", "1", "b1.7.3", "22w46a"] {
            assert!(!supports_quick_play(version), "{version}");
        }
    }

    #[test]
    fn old_versions_join_servers_the_old_way() {
        let server = Join::Server {
            address: "play.example.com:25570".into(),
        };
        assert_eq!(
            join_arguments(&server, "1.21.4").unwrap(),
            ["--quickPlayMultiplayer", "play.example.com:25570"]
        );
        assert_eq!(
            join_arguments(&server, "1.16.1").unwrap(),
            ["--server", "play.example.com", "--port", "25570"]
        );
        let world = Join::World {
            folder: "Home".into(),
        };
        assert_eq!(
            join_arguments(&world, "1.21.4").unwrap(),
            ["--quickPlaySingleplayer", "Home"]
        );
        assert!(join_arguments(&world, "1.16.1").is_err());
    }

    #[test]
    fn addresses_split_into_host_and_port() {
        assert_eq!(
            split_address("play.example.com"),
            ("play.example.com".to_string(), 25565)
        );
        assert_eq!(
            split_address("play.example.com:1234"),
            ("play.example.com".to_string(), 1234)
        );
        assert_eq!(split_address("[::1]:1234"), ("::1".to_string(), 1234));
        assert_eq!(split_address("[::1]"), ("::1".to_string(), 25565));
        assert_eq!(split_address("::1"), ("::1".to_string(), 25565));
    }

    #[test]
    fn the_server_list_keeps_fields_it_does_not_understand() {
        let (_dir, core, id) = setup();
        let path = core.paths.game(&id).join(SERVERS_FILE);
        let mut server = new_server("Hub", "hub.example.com");
        server.insert("acceptTextures".to_string(), Value::Byte(1));
        write_servers(&path, vec![server]).unwrap();

        core.add_server(&id, "Other", "other.example.com").unwrap();
        core.edit_server(&id, 0, "Renamed", "hub.example.com")
            .unwrap();

        let raw = read_servers(&path).unwrap();
        assert!(matches!(raw[0].get("acceptTextures"), Some(Value::Byte(1))));
        let servers = core.servers(&id).unwrap();
        assert_eq!(
            servers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["Renamed", "Other"]
        );
        assert_eq!(core.remove_server(&id, 0).unwrap()[0].index, 0);
    }

    #[test]
    fn changing_the_address_drops_the_old_picture() {
        let (_dir, core, id) = setup();
        let path = core.paths.game(&id).join(SERVERS_FILE);
        let mut server = new_server("Hub", "hub.example.com");
        server.insert("icon".to_string(), Value::String("abc".to_string()));
        write_servers(&path, vec![server]).unwrap();
        assert_eq!(
            core.edit_server(&id, 0, "Hub", "hub.example.com").unwrap()[0]
                .icon
                .as_deref(),
            Some("abc")
        );
        assert_eq!(
            core.edit_server(&id, 0, "Hub", "new.example.com").unwrap()[0].icon,
            None
        );
    }

    #[test]
    fn status_replies_become_plain_text() {
        let reply = json!({
            "version": { "name": "§cPaper 1.21.4" },
            "players": { "online": 12, "max": 100 },
            "description": { "text": "§aWelcome ", "extra": [{ "text": "home" }, "!"] },
            "favicon": "data:image/png;base64,AAAA",
        });
        let status = status_from(&reply, 34);
        assert!(status.online);
        assert_eq!(status.players, Some(12));
        assert_eq!(status.capacity, Some(100));
        assert_eq!(status.motd, "Welcome home!");
        assert_eq!(status.version.as_deref(), Some("Paper 1.21.4"));
        assert_eq!(status.latency_ms, Some(34));
        assert!(status.icon.is_some());

        let hostile = status_from(&json!({ "favicon": "https://evil.example/x.png" }), 1);
        assert_eq!(hostile.icon, None);
    }

    #[test]
    fn numbers_are_written_the_way_the_protocol_expects() {
        let mut out = Vec::new();
        write_varint(&mut out, 300);
        assert_eq!(out, [0xac, 0x02]);
        let mut out = Vec::new();
        write_varint(&mut out, -1);
        assert_eq!(out, [0xff, 0xff, 0xff, 0xff, 0x0f]);
        let frame = packet(0, &[9, 9]);
        assert_eq!(frame, [3, 0, 9, 9]);
    }

    #[tokio::test]
    async fn pings_a_real_socket_speaking_the_protocol() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 256];
            let _ = socket.read(&mut buffer).await;
            let body = json!({ "players": { "online": 3, "max": 20 }, "description": "Hello" })
                .to_string();
            let mut payload = Vec::new();
            write_varint(&mut payload, body.len() as i32);
            payload.extend_from_slice(body.as_bytes());
            socket.write_all(&packet(0, &payload)).await.unwrap();
        });
        let status = ping("127.0.0.1", port).await.unwrap();
        assert_eq!(status.players, Some(3));
        assert_eq!(status.motd, "Hello");
    }

    #[tokio::test]
    async fn a_silent_server_is_just_offline() {
        let (_dir, core, _id) = setup();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let status = core.server_status(&format!("127.0.0.1:{port}")).await;
        assert!(!status.online);
    }

    #[test]
    fn a_backup_is_a_zip_of_the_world() {
        let (_dir, core, id) = setup();
        add_world(&core, &id, "Home", "Home");
        let world = core.paths.game(&id).join("saves/Home");
        fs::create_dir_all(world.join("region")).unwrap();
        fs::write(world.join("region/r.0.0.mca"), b"chunks").unwrap();
        fs::write(world.join("session.lock"), b"held").unwrap();

        let folder = core.back_up_world(&id, "Home").unwrap();

        let zip_path = fs::read_dir(folder)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut archive = zip::ZipArchive::new(fs::File::open(zip_path).unwrap()).unwrap();
        let names: Vec<String> = archive
            .file_names()
            .filter_map(|name| Some(name.ok()?.into_owned()))
            .collect();
        assert!(names.contains(&"Home/region/r.0.0.mca".to_string()));
        assert!(!names.iter().any(|name| name.contains("session.lock")));
        let mut text = String::new();
        archive
            .by_name("Home/region/r.0.0.mca")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "chunks");
    }

    #[test]
    fn pictures_cannot_be_read_from_outside_their_folders() {
        let (dir, core, id) = setup();
        add_world(&core, &id, "Home", "Home");
        fs::write(core.paths.game(&id).join("saves/Home/icon.png"), b"png").unwrap();
        fs::write(dir.path().join("secret.png"), b"secret").unwrap();
        fs::write(core.paths.game(&id).join("saves/Home/notes.txt"), b"text").unwrap();

        assert_eq!(
            core.local_image(&format!("world/{id}/Home")).as_deref(),
            Some(&b"png"[..])
        );
        for attempt in [
            format!("world/{id}/.."),
            format!("world/../{id}"),
            format!("screenshot/{id}/../../../secret.png"),
            "skin/../secret.png".to_string(),
            format!("screenshot/{id}/C:\\Windows\\x.png"),
            format!("screenshot/{id}/..\\..\\secret.png"),
            "other/thing".to_string(),
            String::new(),
        ] {
            assert_eq!(core.local_image(&attempt), None, "{attempt}");
        }
    }

    #[test]
    fn recent_worlds_skip_versions_that_cannot_open_them() {
        let (_dir, core, id) = setup();
        add_world(&core, &id, "Home", "Home");
        let old = core
            .register_instance("Old", "1.16.1", Loader::default(), "1.16.1".into(), None)
            .unwrap();
        add_world(&core, &old.id, "Speedrun", "Speedrun");
        let recent = core.recent_worlds().unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].instance, id);
    }

    #[test]
    fn a_reader_never_needs_more_than_the_length_it_was_given() {
        let mut cursor = std::io::Cursor::new(vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        let result = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(read_varint(&mut cursor));
        assert!(result.is_err(), "a number can't take more than five bytes");
    }
}

//! Discord rich presence: the instance being played, shown on the player's Discord
//! profile. Waymark tells the Discord app on the same computer through its local
//! IPC pipe, so nothing goes over the network from here. It's off until the
//! player turns it on, and nothing happens at all while Discord isn't running.
//!
//! Only names a player would expect to share go out: the instance, its Minecraft
//! version and loader, and a server's name when it came from the server list.
//! A private server's address never does.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Value, json};

use crate::Core;
use crate::brand::{DISCORD_CLIENT_ID, PRODUCT_NAME, VERSION};
use crate::instance::Instance;
use crate::lock::LoaderKind;
use crate::worlds::Join;

const HANDSHAKE: u32 = 0;
const FRAME: u32 = 1;
/// How long to wait before trying Discord again after it wasn't there.
const RETRY: Duration = Duration::from_secs(15);
const SITE: &str = "https://waymark-launcher.vercel.app";
const LOGO: &str = "https://waymark-launcher.vercel.app/assets/logo-512.png";
/// Discord rejects text shorter than 2 characters or longer than 128.
const MAX_TEXT: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub details: String,
    pub state: String,
    /// When the game started, in seconds since 1970, which Discord counts up from.
    pub started: i64,
    pub image: String,
    pub image_text: String,
    pub badge: Option<String>,
}

fn loader_name(kind: LoaderKind) -> &'static str {
    match kind {
        LoaderKind::Vanilla => "Minecraft",
        LoaderKind::Fabric => "Fabric",
        LoaderKind::Quilt => "Quilt",
        LoaderKind::Forge => "Forge",
        LoaderKind::NeoForge => "NeoForge",
        LoaderKind::LegacyFabric => "Legacy Fabric",
        LoaderKind::Babric => "Babric",
        LoaderKind::Ornithe => "Ornithe",
        LoaderKind::LiteLoader => "LiteLoader",
        LoaderKind::Rift => "Rift",
        LoaderKind::Cleanroom => "Cleanroom",
    }
}

fn fit(text: &str) -> String {
    let text: String = text.trim().chars().take(MAX_TEXT).collect();
    if text.chars().count() < 2 {
        format!("Playing {text}").trim().to_string()
    } else {
        text
    }
}

/// What to show while an instance plays.
pub fn activity_for(instance: &Instance, join: Option<&Join>, started: i64) -> Activity {
    let game = match instance.loader.kind {
        LoaderKind::Vanilla => format!("Minecraft {}", instance.minecraft),
        kind => format!("{} {}", loader_name(kind), instance.minecraft),
    };
    let listed = instance.server.as_ref().filter(|link| match join {
        Some(Join::Server { address }) => address.eq_ignore_ascii_case(&link.address),
        _ => false,
    });
    let state = match (listed, join) {
        (Some(link), _) => format!("On {}", link.name),
        (None, Some(Join::Server { .. })) => format!("{game}, multiplayer"),
        (None, Some(Join::World { .. })) => format!("{game}, singleplayer"),
        (None, None) => game,
    };
    let icon = listed
        .and_then(|link| link.icon.clone())
        .filter(|icon| icon.starts_with("https://"));
    Activity {
        details: fit(&instance.name),
        state: fit(&state),
        started,
        image: icon.clone().unwrap_or_else(|| LOGO.to_string()),
        image_text: fit(&match listed {
            Some(link) => link.name.clone(),
            None => format!("{PRODUCT_NAME} {VERSION}"),
        }),
        badge: icon.map(|_| LOGO.to_string()),
    }
}

/// The SET_ACTIVITY command Discord expects, or a clear when there's nothing.
fn command(activity: Option<&Activity>, pid: u32, nonce: u64) -> Value {
    let activity = activity.map(|activity| {
        let mut assets = json!({
            "large_image": activity.image,
            "large_text": activity.image_text,
        });
        if let Some(badge) = &activity.badge {
            assets["small_image"] = json!(badge);
            assets["small_text"] = json!(PRODUCT_NAME);
        }
        json!({
            "type": 0,
            "details": activity.details,
            "state": activity.state,
            "timestamps": { "start": activity.started },
            "assets": assets,
            "buttons": [{ "label": format!("Get {PRODUCT_NAME}"), "url": SITE }],
        })
    });
    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce.to_string(),
    })
}

/// A frame is its opcode and length, both little-endian, then the JSON.
fn write_frame(pipe: &mut impl Write, op: u32, body: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(body)?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    pipe.write_all(&frame)?;
    pipe.flush()
}

fn read_frame(pipe: &mut impl Read) -> io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    pipe.read_exact(&mut head)?;
    let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len > 1 << 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    pipe.read_exact(&mut body)?;
    let value = serde_json::from_slice(&body)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok((op, value))
}

pub(crate) trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

/// Discord's pipe, if the app is running.
fn connect() -> Option<Box<dyn Pipe>> {
    for number in 0..10 {
        #[cfg(windows)]
        {
            let name = format!(r"\\.\pipe\discord-ipc-{number}");
            if let Ok(pipe) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(name)
            {
                return Some(Box::new(pipe));
            }
        }
        #[cfg(unix)]
        {
            let mut roots: Vec<std::path::PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
                .iter()
                .filter_map(|name| std::env::var_os(name).map(Into::into))
                .collect();
            roots.push("/tmp".into());
            for root in roots {
                // Flatpak and Snap builds of Discord put the pipe one folder down.
                for folder in ["", "app/com.discordapp.Discord", "snap.discord"] {
                    let path = root.join(folder).join(format!("discord-ipc-{number}"));
                    if let Ok(pipe) = std::os::unix::net::UnixStream::connect(path) {
                        return Some(Box::new(pipe));
                    }
                }
            }
        }
    }
    None
}

struct Client {
    pipe: Box<dyn Pipe>,
    nonce: u64,
}

impl Client {
    fn open(client_id: &str, pipe: Box<dyn Pipe>) -> io::Result<Self> {
        let mut client = Client { pipe, nonce: 0 };
        write_frame(
            &mut client.pipe,
            HANDSHAKE,
            &json!({ "v": 1, "client_id": client_id }),
        )?;
        let (op, reply) = read_frame(&mut client.pipe)?;
        if op != FRAME || reply.get("evt").and_then(Value::as_str) != Some("READY") {
            return Err(io::Error::other(format!("Discord said no: {reply}")));
        }
        Ok(client)
    }

    fn set(&mut self, activity: Option<&Activity>) -> io::Result<()> {
        self.nonce += 1;
        write_frame(
            &mut self.pipe,
            FRAME,
            &command(activity, std::process::id(), self.nonce),
        )?;
        let (_, reply) = read_frame(&mut self.pipe)?;
        if reply.get("evt").and_then(Value::as_str) == Some("ERROR") {
            return Err(io::Error::other(format!(
                "Discord refused the activity: {reply}"
            )));
        }
        Ok(())
    }
}

/// Keeps the newest wanted activity and gets it to Discord, connecting when there
/// is something to show and trying again later if Discord wasn't there.
fn run(
    client_id: &str,
    wanted: mpsc::Receiver<Option<Activity>>,
    connect: impl Fn() -> Option<Box<dyn Pipe>>,
) {
    let mut showing: Option<Activity> = None;
    let mut client: Option<Client> = None;
    let mut due = false;
    loop {
        match wanted.recv_timeout(RETRY) {
            Ok(next) => {
                showing = next;
                due = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(next) = wanted.try_recv() {
            showing = next;
            due = true;
        }
        if !due {
            continue;
        }
        if client.is_none() {
            // Closing the pipe already clears it, so there's nothing to do.
            if showing.is_none() {
                due = false;
                continue;
            }
            client = connect().and_then(|pipe| Client::open(client_id, pipe).ok());
            if client.is_none() {
                continue;
            }
        }
        let Some(open) = client.as_mut() else {
            continue;
        };
        match open.set(showing.as_ref()) {
            Ok(()) => {
                due = false;
                if showing.is_none() {
                    client = None;
                }
            }
            Err(error) => {
                tracing::debug!(%error, "Discord presence failed, trying again later");
                client = None;
            }
        }
    }
}

pub(crate) struct Presence {
    wanted: mpsc::Sender<Option<Activity>>,
}

impl Presence {
    fn start() -> Self {
        let (wanted, receive) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("discord presence".into())
            .spawn(move || run(DISCORD_CLIENT_ID, receive, connect));
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start Discord presence");
        }
        Presence { wanted }
    }
}

impl Core {
    /// Whether this build can talk to Discord at all.
    pub fn discord_available(&self) -> bool {
        !DISCORD_CLIENT_ID.is_empty()
    }

    /// A game started: show it, if the player wants that.
    pub(crate) fn show_playing(&self, instance: &Instance, join: Option<&Join>) {
        let started = time::OffsetDateTime::now_utc().unix_timestamp();
        *self.playing.lock().expect("presence poisoned") =
            Some((instance.id.clone(), activity_for(instance, join, started)));
        self.update_presence();
    }

    /// A game closed: stop showing it.
    pub(crate) fn stop_playing(&self, id: &str) {
        let mut playing = self.playing.lock().expect("presence poisoned");
        if playing.as_ref().is_some_and(|(shown, _)| shown == id) {
            *playing = None;
        }
        drop(playing);
        self.update_presence();
    }

    /// Sends Discord whatever should show now, under the player's setting.
    pub(crate) fn update_presence(&self) {
        if cfg!(test) || !self.discord_available() {
            return;
        }
        let on = self
            .settings()
            .is_ok_and(|settings| settings.discord_presence);
        let wanted = on
            .then(|| {
                self.playing
                    .lock()
                    .expect("presence poisoned")
                    .as_ref()
                    .map(|(_, activity)| activity.clone())
            })
            .flatten();
        if wanted.is_none() && self.presence.get().is_none() {
            return;
        }
        let _ = self
            .presence
            .get_or_init(Presence::start)
            .wanted
            .send(wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::ServerLink;
    use crate::lock::Loader;
    use std::sync::{Arc, Mutex};

    fn instance(kind: LoaderKind) -> Instance {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        core.register_instance(
            "Emberfall",
            "1.21.4",
            Loader {
                kind,
                version: None,
            },
            "x".into(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn a_plain_launch_shows_the_instance_and_its_game() {
        let activity = activity_for(&instance(LoaderKind::Fabric), None, 1_700_000_000);
        assert_eq!(activity.details, "Emberfall");
        assert_eq!(activity.state, "Fabric 1.21.4");
        assert_eq!(activity.image, LOGO);
        assert!(activity.badge.is_none());
        let vanilla = activity_for(&instance(LoaderKind::Vanilla), None, 0);
        assert_eq!(vanilla.state, "Minecraft 1.21.4");
    }

    #[test]
    fn a_listed_server_is_named_but_a_private_address_never_is() {
        let mut listed = instance(LoaderKind::Fabric);
        listed.server = Some(ServerLink {
            project: "abc".into(),
            name: "Aero SMP".into(),
            address: "play.aerosmp.com".into(),
            icon: Some("https://cdn.modrinth.com/icon.webp".into()),
        });
        let join = Join::Server {
            address: "PLAY.aerosmp.com".into(),
        };
        let on = activity_for(&listed, Some(&join), 0);
        assert_eq!(on.state, "On Aero SMP");
        assert_eq!(on.image, "https://cdn.modrinth.com/icon.webp");
        assert_eq!(on.badge.as_deref(), Some(LOGO));

        let private = Join::Server {
            address: "192.168.1.20:25565".into(),
        };
        let elsewhere = activity_for(&instance(LoaderKind::Fabric), Some(&private), 0);
        assert_eq!(elsewhere.state, "Fabric 1.21.4, multiplayer");
        let all = serde_json::to_string(&command(Some(&elsewhere), 1, 1)).unwrap();
        assert!(!all.contains("192.168"), "{all}");

        let world = Join::World {
            folder: "My World".into(),
        };
        assert_eq!(
            activity_for(&instance(LoaderKind::Fabric), Some(&world), 0).state,
            "Fabric 1.21.4, singleplayer"
        );
    }

    #[test]
    fn text_fits_what_discord_accepts() {
        assert_eq!(fit("X"), "Playing X");
        assert_eq!(fit(&"a".repeat(300)).chars().count(), MAX_TEXT);
        assert_eq!(fit("  Emberfall  "), "Emberfall");
    }

    #[test]
    fn frames_round_trip() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, FRAME, &json!({ "cmd": "PING" })).unwrap();
        assert_eq!(&buffer[..4], &1u32.to_le_bytes());
        let (op, value) = read_frame(&mut buffer.as_slice()).unwrap();
        assert_eq!((op, value), (FRAME, json!({ "cmd": "PING" })));
        let mut huge = Vec::new();
        huge.extend_from_slice(&1u32.to_le_bytes());
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut huge.as_slice()).is_err());
    }

    /// Stands in for the Discord app: answers the handshake and each command, and
    /// keeps every frame Waymark sent.
    #[derive(Clone)]
    struct FakeDiscord {
        sent: Arc<Mutex<Vec<(u32, Value)>>>,
        replies: Arc<Mutex<Vec<u8>>>,
        refuse: bool,
    }

    impl Read for FakeDiscord {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let mut replies = self.replies.lock().unwrap();
            let n = out.len().min(replies.len());
            out[..n].copy_from_slice(&replies[..n]);
            replies.drain(..n);
            Ok(n)
        }
    }

    impl Write for FakeDiscord {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let (op, value) = read_frame(&mut &bytes[..]).unwrap();
            self.sent.lock().unwrap().push((op, value));
            let reply = if op == HANDSHAKE {
                json!({ "cmd": "DISPATCH", "evt": if self.refuse { "ERROR" } else { "READY" } })
            } else {
                json!({ "cmd": "SET_ACTIVITY", "evt": null })
            };
            write_frame(&mut *self.replies.lock().unwrap(), FRAME, &reply).unwrap();
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn fake(refuse: bool) -> FakeDiscord {
        FakeDiscord {
            sent: Arc::default(),
            replies: Arc::default(),
            refuse,
        }
    }

    #[test]
    fn the_newest_activity_reaches_discord_and_closing_clears_it() {
        let discord = fake(false);
        let (wanted, receive) = mpsc::channel();
        let pipe = discord.clone();
        let worker = std::thread::spawn(move || {
            run("1234", receive, move || {
                Some(Box::new(pipe.clone()) as Box<dyn Pipe>)
            })
        });
        let activity = activity_for(&instance(LoaderKind::Fabric), None, 42);
        wanted.send(Some(activity)).unwrap();
        let shown = || discord.sent.lock().unwrap().len();
        for _ in 0..200 {
            if shown() >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        wanted.send(None).unwrap();
        drop(wanted);
        worker.join().unwrap();

        let sent = discord.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 3, "{sent:?}");
        assert_eq!(sent[0].0, HANDSHAKE);
        assert_eq!(sent[0].1["client_id"], "1234");
        assert_eq!(sent[1].1["args"]["activity"]["details"], "Emberfall");
        assert!(sent[2].1["args"]["activity"].is_null());
    }

    #[test]
    fn nothing_connects_when_there_is_nothing_to_show() {
        let (wanted, receive) = mpsc::channel();
        let tried = Arc::new(Mutex::new(0));
        let count = tried.clone();
        let worker = std::thread::spawn(move || {
            run("1234", receive, move || {
                *count.lock().unwrap() += 1;
                None
            })
        });
        wanted.send(None).unwrap();
        drop(wanted);
        worker.join().unwrap();
        assert_eq!(*tried.lock().unwrap(), 0);
    }

    #[test]
    fn an_activity_is_sent_with_its_details() {
        let discord = fake(false);
        let mut client = Client::open("1234", Box::new(discord.clone())).unwrap();
        let activity = activity_for(&instance(LoaderKind::NeoForge), None, 42);
        client.set(Some(&activity)).unwrap();
        client.set(None).unwrap();
        let sent = discord.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 3);
        let shown = &sent[1].1;
        assert_eq!(shown["cmd"], "SET_ACTIVITY");
        assert_eq!(shown["args"]["activity"]["details"], "Emberfall");
        assert_eq!(shown["args"]["activity"]["state"], "NeoForge 1.21.4");
        assert_eq!(shown["args"]["activity"]["timestamps"]["start"], 42);
        assert_eq!(shown["args"]["activity"]["buttons"][0]["url"], SITE);
        assert!(sent[2].1["args"]["activity"].is_null());
        assert_ne!(sent[1].1["nonce"], sent[2].1["nonce"]);
    }

    /// Checks Waymark's application against the Discord app on this computer. It
    /// only shakes hands and clears, so nothing ever shows on the profile, and it
    /// prints whether Discord answered, never who is signed in.
    #[test]
    #[ignore = "talks to the Discord app on this computer"]
    fn live_discord_accepts_waymark_s_application() {
        let Some(pipe) = connect() else {
            println!("Discord isn't running on this computer");
            return;
        };
        let mut client =
            Client::open(DISCORD_CLIENT_ID, pipe).expect("Discord refused the application");
        client.set(None).expect("Discord refused a clear");
        println!("Discord accepted the application and a clear");
    }

    #[test]
    fn a_refusal_from_discord_is_an_error_not_a_hang() {
        assert!(Client::open("1234", Box::new(fake(true))).is_err());
    }
}

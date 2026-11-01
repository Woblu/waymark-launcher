//! Running an instance as a server on the player's own computer. It uses the
//! same folder the server export builds, and it never agrees to Minecraft's
//! EULA by itself: that takes the player's own tick, each time a server is
//! set up, after being shown where to read it.

use std::fs;
use std::net::{IpAddr, UdpSocket};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Notify, mpsc};

use crate::Core;
use crate::error::{Error, Result};
use crate::event::{Event, Task};

const EULA_FILE: &str = "eula.txt";
const EULA_ACCEPTED: &str = "eula=true\n";
const PROPERTIES_FILE: &str = "server.properties";
const DEFAULT_PORT: u16 = 25565;
/// How long a server gets to save and close before it's stopped by force.
const STOP_TIMEOUT: Duration = Duration::from_secs(45);

/// How the running server is reached.
#[derive(Clone)]
pub(crate) struct Host {
    input: mpsc::UnboundedSender<String>,
    stop: Arc<Notify>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    /// Whether the server folder has been built.
    pub ready: bool,
    pub eula_accepted: bool,
    pub running: bool,
    /// The address for people on the same network.
    pub address: Option<String>,
    pub port: u16,
}

/// The computer's address on its own network. This sends nothing: connecting
/// a UDP socket only asks the system which route it would use.
fn local_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    socket.local_addr().ok().map(|address| address.ip())
}

fn port_in(properties: &str) -> Option<u16> {
    properties.lines().find_map(|line| {
        line.trim()
            .strip_prefix("server-port=")?
            .trim()
            .parse()
            .ok()
    })
}

fn has_accepted(eula: &str) -> bool {
    eula.lines().any(|line| line.trim() == "eula=true")
}

impl Core {
    fn host_folder(&self, id: &str) -> PathBuf {
        self.paths
            .root()
            .join("servers")
            .join(format!("{id}-server"))
    }

    pub fn host_path(&self, id: &str) -> PathBuf {
        self.host_folder(id)
    }

    pub fn host_running(&self, id: &str) -> bool {
        self.hosts
            .lock()
            .expect("host list poisoned")
            .contains_key(id)
    }

    pub fn any_host_running(&self) -> bool {
        !self.hosts.lock().expect("host list poisoned").is_empty()
    }

    pub fn host_status(&self, id: &str) -> Result<HostStatus> {
        self.instance(id)?;
        let folder = self.host_folder(id);
        let eula = fs::read_to_string(folder.join(EULA_FILE)).unwrap_or_default();
        let properties = fs::read_to_string(folder.join(PROPERTIES_FILE)).unwrap_or_default();
        let port = port_in(&properties).unwrap_or(DEFAULT_PORT);
        Ok(HostStatus {
            ready: folder.join("start.bat").is_file(),
            eula_accepted: has_accepted(&eula),
            running: self.host_running(id),
            address: local_address().map(|ip| format!("{ip}:{port}")),
            port,
        })
    }

    /// Builds the server folder, or brings its mods up to date. Worlds and
    /// server settings in an existing folder are left alone.
    pub async fn prepare_host(&self, id: &str) -> Result<HostStatus> {
        if self.host_running(id) {
            return Err(Error::Other("stop the server first".into()));
        }
        let instance = self.instance(id)?;
        let task = Task::start(
            &self.events,
            format!("Setting up a server for {}", instance.name),
        );
        let folder = self.host_folder(id);
        let outcome = if folder.join("start.bat").is_file() {
            self.refresh_host(id, &folder)
        } else {
            let _ = fs::remove_dir_all(&folder);
            let parent = folder.parent().map(PathBuf::from).unwrap_or_default();
            match fs::create_dir_all(&parent) {
                Ok(()) => self
                    .build_server(&instance, &parent, &task)
                    .await
                    .map(|_| ()),
                Err(error) => Err(error.into()),
            }
        };
        match outcome {
            Ok(()) => {
                task.finish();
                self.host_status(id)
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    fn refresh_host(&self, id: &str, folder: &std::path::Path) -> Result<()> {
        let lock = self.lockfile(id)?;
        let mods = folder.join("mods");
        let _ = fs::remove_dir_all(&mods);
        let (kept, _) = crate::export::split_mods(&lock);
        for entry in kept {
            self.store.place(
                &entry.hash,
                &mods.join(&entry.file),
                crate::store::Placement::Copy,
            )?;
        }
        Ok(())
    }

    /// Records that the player has accepted the EULA. Only ever called from
    /// their own action.
    pub fn accept_eula(&self, id: &str) -> Result<()> {
        let folder = self.host_folder(id);
        if !folder.is_dir() {
            return Err(Error::NotFound("this instance's server".into()));
        }
        fs::write(folder.join(EULA_FILE), EULA_ACCEPTED)?;
        Ok(())
    }

    pub fn start_host(&self, id: &str) -> Result<()> {
        if self.host_running(id) {
            return Err(Error::Other("this server is already running".into()));
        }
        let status = self.host_status(id)?;
        if !status.ready {
            return Err(Error::Other("set up the server first".into()));
        }
        if !status.eula_accepted {
            return Err(Error::Other(
                "the Minecraft EULA has to be accepted first".into(),
            ));
        }
        let folder = self.host_folder(id);
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.arg("/C").arg("start.bat");
            command
        } else {
            let mut command = Command::new("sh");
            command.arg("start.sh");
            command
        };
        command
            .current_dir(&folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::loaders::hide_console(&mut command);
        let mut child = command.spawn()?;

        let log_key = format!("server:{id}");
        for (stream, error) in [
            (child.stdout.take().map(boxed), false),
            (child.stderr.take().map(boxed), true),
        ] {
            let Some(stream) = stream else { continue };
            let (events, key) = (self.events.clone(), log_key.clone());
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    events(Event::GameLog {
                        instance: key.clone(),
                        line,
                        error,
                    });
                }
            });
        }

        let (input, mut commands) = mpsc::unbounded_channel::<String>();
        let stop = Arc::new(Notify::new());
        self.hosts.lock().expect("host list poisoned").insert(
            id.to_string(),
            Host {
                input,
                stop: stop.clone(),
            },
        );

        let core = self.clone();
        let id = id.to_string();
        let mut stdin = child.stdin.take();
        tokio::spawn(async move {
            let finished = async {
                loop {
                    tokio::select! {
                        status = child.wait() => return status.ok(),
                        Some(line) = commands.recv() => {
                            if let Some(pipe) = stdin.as_mut() {
                                let _ = pipe.write_all(format!("{line}\n").as_bytes()).await;
                                let _ = pipe.flush().await;
                            }
                        }
                        () = stop.notified() => {
                            if let Some(pipe) = stdin.as_mut() {
                                let _ = pipe.write_all(b"stop\n").await;
                                let _ = pipe.flush().await;
                            }
                            match tokio::time::timeout(STOP_TIMEOUT, child.wait()).await {
                                Ok(status) => return status.ok(),
                                Err(_) => {
                                    let _ = child.kill().await;
                                    return None;
                                }
                            }
                        }
                    }
                }
            };
            finished.await;
            core.hosts.lock().expect("host list poisoned").remove(&id);
            (core.events)(Event::ServerStopped { instance: id });
        });
        Ok(())
    }

    /// Asks the server to save and close.
    pub fn stop_host(&self, id: &str) -> Result<()> {
        let hosts = self.hosts.lock().expect("host list poisoned");
        let host = hosts
            .get(id)
            .ok_or_else(|| Error::Other("this server isn't running".into()))?;
        host.stop.notify_one();
        Ok(())
    }

    pub fn host_command(&self, id: &str, line: &str) -> Result<()> {
        let hosts = self.hosts.lock().expect("host list poisoned");
        let host = hosts
            .get(id)
            .ok_or_else(|| Error::Other("this server isn't running".into()))?;
        host.input
            .send(line.to_string())
            .map_err(|_| Error::Other("this server isn't running".into()))
    }
}

fn boxed<R: tokio::io::AsyncRead + Send + Unpin + 'static>(
    stream: R,
) -> Box<dyn tokio::io::AsyncRead + Send + Unpin> {
    Box::new(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;

    fn setup() -> (tempfile::TempDir, Core, String) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Test", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        (dir, core, instance.id)
    }

    #[test]
    fn reads_the_port_from_server_properties() {
        assert_eq!(port_in("motd=hi\nserver-port=25570\n"), Some(25570));
        assert_eq!(port_in("motd=hi\n"), None);
        assert_eq!(port_in("server-port=nonsense"), None);
    }

    #[test]
    fn the_eula_only_counts_when_it_says_true() {
        assert!(has_accepted("#comment\neula=true\n"));
        assert!(!has_accepted("#eula=true\neula=false\n"));
        assert!(!has_accepted(""));
    }

    #[tokio::test]
    async fn a_server_never_starts_without_the_eula() {
        let (_dir, core, id) = setup();
        assert!(core.start_host(&id).is_err(), "nothing is set up yet");
        let folder = core.host_folder(&id);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("start.bat"), "exit\n").unwrap();
        fs::write(folder.join("eula.txt"), "eula=false\n").unwrap();

        let refused = core.start_host(&id).unwrap_err().to_string();
        assert!(refused.contains("EULA"), "{refused}");
        assert!(!core.host_status(&id).unwrap().eula_accepted);

        core.accept_eula(&id).unwrap();
        assert!(core.host_status(&id).unwrap().eula_accepted);
    }

    #[tokio::test]
    async fn a_running_server_takes_commands_and_stops() {
        let (_dir, core, id) = setup();
        let folder = core.host_folder(&id);
        fs::create_dir_all(&folder).unwrap();
        // Stands in for a server: prints a line, then waits for its console to close.
        fs::write(
            folder.join("start.bat"),
            "@echo ready\r\n@set /p line=\r\n@echo got %line%\r\n",
        )
        .unwrap();
        fs::write(
            folder.join("start.sh"),
            "echo ready\nread line\necho got $line\n",
        )
        .unwrap();
        fs::write(folder.join("eula.txt"), EULA_ACCEPTED).unwrap();

        core.start_host(&id).unwrap();
        assert!(core.host_running(&id));
        assert!(core.start_host(&id).is_err(), "only one at a time");
        core.stop_host(&id).unwrap();

        for _ in 0..200 {
            if !core.host_running(&id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(!core.host_running(&id));
    }
}

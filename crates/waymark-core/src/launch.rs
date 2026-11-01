//! Turns a prepared version into a running game.
//!
//! Launching takes a `Session`, and outside debug builds the only way to get
//! one is a signed-in account with verified ownership (see `auth`). There is
//! no username-only path to play.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Instant, SystemTime};

use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::Core;
use crate::auth::Session;
use crate::brand::{PRODUCT_NAME, VERSION};
use crate::crash;
use crate::error::{Error, Result};
use crate::event::{Event, Task};
use crate::install::Prepared;
use crate::instance::{Instance, LaunchOptions};
use crate::mojang::{Argument, Features, OneOrMany, allowed};
use crate::paths::Paths;
use crate::waymarks::WaymarkKind;
use crate::worlds::{Join, join_arguments};

const CLASSPATH_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

/// The same collector settings Mojang's launcher defaults to.
const GC_ARGS: [&str; 6] = [
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+UseG1GC",
    "-XX:G1NewSizePercent=20",
    "-XX:G1ReservePercent=20",
    "-XX:MaxGCPauseMillis=50",
    "-XX:G1HeapRegionSize=32M",
];

fn classpath(paths: &Paths, prepared: &Prepared) -> String {
    let libraries = prepared
        .meta
        .libraries
        .iter()
        .filter(|l| l.is_allowed(Features::default()))
        .filter_map(|l| l.artifact())
        .map(|a| paths.libraries().join(a.path));
    libraries
        .chain(std::iter::once(prepared.client_jar.clone()))
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(CLASSPATH_SEPARATOR)
}

fn expand(argument: &str, values: &HashMap<&str, String>) -> String {
    let mut text = argument.to_string();
    for (key, value) in values {
        let marker = format!("${{{key}}}");
        if text.contains(&marker) {
            text = text.replace(&marker, value);
        }
    }
    text
}

fn collect(arguments: &[Argument], values: &HashMap<&str, String>, out: &mut Vec<String>) {
    for argument in arguments {
        match argument {
            Argument::Plain(text) => out.push(expand(text, values)),
            Argument::Ruled { rules, value } if allowed(rules, Features::default()) => {
                match value {
                    OneOrMany::One(text) => out.push(expand(text, values)),
                    OneOrMany::Many(texts) => out.extend(texts.iter().map(|t| expand(t, values))),
                }
            }
            Argument::Ruled { .. } => {}
        }
    }
}

/// Splits a command the way a shell would, keeping quoted parts together.
fn split_command(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    for character in text.chars() {
        match (quote, character) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(character);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
                started = false;
            }
            (None, c) => current.push(c),
        }
    }
    if started || !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn window_arguments(options: &LaunchOptions) -> Vec<String> {
    let mut args = Vec::new();
    if options.fullscreen {
        args.push("--fullscreen".to_string());
    } else if let (Some(width), Some(height)) = (options.width, options.height) {
        args.extend([
            "--width".to_string(),
            width.to_string(),
            "--height".to_string(),
            height.to_string(),
        ]);
    }
    args
}

/// The same names other launchers give hook commands, so scripts carry over.
fn hook_environment(paths: &Paths, instance: &Instance) -> [(&'static str, String); 5] {
    [
        ("INST_ID", instance.id.clone()),
        ("INST_NAME", instance.name.clone()),
        (
            "INST_DIR",
            paths.instance(&instance.id).to_string_lossy().into_owned(),
        ),
        (
            "INST_MC_DIR",
            paths.game(&instance.id).to_string_lossy().into_owned(),
        ),
        ("INST_MC_VER", instance.minecraft.clone()),
    ]
}

/// cmd has its own quoting, so a script has to reach it exactly as typed.
#[cfg(windows)]
fn shell(script: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("cmd");
    command.as_std_mut().raw_arg("/C").raw_arg(script);
    command
}

#[cfg(not(windows))]
fn shell(script: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(script);
    command
}

/// Builds the full argument list after the Java binary.
pub fn build_arguments(
    paths: &Paths,
    instance: &Instance,
    prepared: &Prepared,
    session: &Session,
    memory_mb: u32,
) -> Result<Vec<String>> {
    let meta = &prepared.meta;
    let game_dir = paths.game(&instance.id);
    let classpath = classpath(paths, prepared);
    let asset_index = meta
        .asset_index
        .as_ref()
        .map(|a| a.id.clone())
        .or_else(|| meta.assets.clone())
        .unwrap_or_else(|| "legacy".to_string());

    let values: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", session.name.clone()),
        ("version_name", instance.version_id.clone()),
        ("game_directory", game_dir.to_string_lossy().into_owned()),
        ("assets_root", paths.assets().to_string_lossy().into_owned()),
        (
            "game_assets",
            prepared.game_assets.to_string_lossy().into_owned(),
        ),
        ("assets_index_name", asset_index),
        ("auth_uuid", session.uuid.clone()),
        ("auth_access_token", session.access_token.clone()),
        ("auth_session", session.access_token.clone()),
        ("auth_xuid", session.xuid.clone()),
        ("clientid", crate::brand::MSA_CLIENT_ID.to_string()),
        ("user_type", "msa".to_string()),
        ("user_properties", "{}".to_string()),
        (
            "version_type",
            meta.kind.clone().unwrap_or_else(|| "release".to_string()),
        ),
        (
            "natives_directory",
            prepared.natives.to_string_lossy().into_owned(),
        ),
        ("launcher_name", PRODUCT_NAME.to_string()),
        ("launcher_version", VERSION.to_string()),
        (
            "library_directory",
            paths.libraries().to_string_lossy().into_owned(),
        ),
        ("classpath_separator", CLASSPATH_SEPARATOR.to_string()),
        ("classpath", classpath.clone()),
    ]);

    let mut args = vec![
        format!("-Xms{}M", memory_mb.min(1024)),
        format!("-Xmx{memory_mb}M"),
    ];
    args.extend(GC_ARGS.iter().map(ToString::to_string));
    args.extend(instance.jvm_args.iter().cloned());

    let declared = args.len();
    if let Some(arguments) = &meta.arguments {
        collect(&arguments.jvm, &values, &mut args);
    }
    // A loader's profile over an old version adds flags but still leaves the classpath to us.
    if !args[declared..]
        .iter()
        .any(|arg| matches!(arg.as_str(), "-cp" | "-classpath"))
    {
        args.push(format!(
            "-Djava.library.path={}",
            prepared.natives.display()
        ));
        args.push("-cp".to_string());
        args.push(classpath);
    }

    if let (Some(config), Some(logging)) = (
        &prepared.log_config,
        meta.logging.as_ref().and_then(|l| l.client.as_ref()),
    ) {
        args.push(
            logging
                .argument
                .replace("${path}", &config.to_string_lossy()),
        );
    }

    args.push(
        meta.main_class
            .clone()
            .ok_or_else(|| Error::Other("this version has no main class".into()))?,
    );

    if let Some(arguments) = meta.arguments.as_ref().filter(|a| !a.game.is_empty()) {
        collect(&arguments.game, &values, &mut args);
    } else if let Some(legacy) = &meta.minecraft_arguments {
        args.extend(legacy.split_whitespace().map(|part| expand(part, &values)));
    }
    Ok(args)
}

impl Core {
    pub fn any_game_running(&self) -> bool {
        !self.games.lock().expect("game list poisoned").is_empty()
    }

    pub fn game_running(&self, id: &str) -> bool {
        self.games
            .lock()
            .expect("game list poisoned")
            .contains_key(id)
    }

    pub fn stop_game(&self, id: &str) -> Result<()> {
        let stop = self
            .games
            .lock()
            .expect("game list poisoned")
            .get(id)
            .cloned();
        match stop {
            Some(signal) => {
                signal.notify_one();
                Ok(())
            }
            None => Err(Error::Other("this instance isn't running".into())),
        }
    }

    /// Prepares and starts the game with the active account.
    pub async fn launch(&self, id: &str) -> Result<()> {
        let session = self.session().await?;
        self.launch_with(id, session, None).await
    }

    /// Starts the game, and goes straight to a world or server if given one.
    pub async fn launch_with(&self, id: &str, session: Session, join: Option<Join>) -> Result<()> {
        if self.game_running(id) {
            return Err(Error::Other("this instance is already running".into()));
        }
        let instance = self.instance(id)?;
        let task = Task::start(&self.events, format!("Launching {}", instance.name));
        let started = self
            .start_game(&instance, &session, join.as_ref(), &task)
            .await;
        match started {
            Ok(()) => {
                task.finish();
                Ok(())
            }
            Err(error) => {
                task.fail(error.to_string());
                Err(error)
            }
        }
    }

    async fn start_game(
        &self,
        instance: &Instance,
        session: &Session,
        join: Option<&Join>,
        task: &Task,
    ) -> Result<()> {
        let extra = join.map_or(Ok(Vec::new()), |join| {
            join_arguments(join, &instance.minecraft)
        })?;
        let prepared = self.prepare_version(&instance.version_id, task).await?;
        self.add_java_fix(instance).await?;

        task.progress(0, 1, "Setting a waymark");
        let trail = self.trail(&instance.id);
        tokio::task::spawn_blocking(move || trail.mark("Before playing", WaymarkKind::Launch))
            .await
            .map_err(|e| Error::Other(e.to_string()))??;

        let memory = instance.memory_mb.unwrap_or(self.settings()?.memory_mb);
        let mut args = build_arguments(&self.paths, instance, &prepared, session, memory)?;
        args.extend(window_arguments(&instance.options));
        args.extend(extra);
        let java: std::path::PathBuf = instance
            .java_path
            .clone()
            .map_or(prepared.java.clone(), Into::into);
        let hooks = hook_environment(&self.paths, instance);

        if let Some(script) = instance
            .options
            .pre_launch
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            task.progress(0, 1, "Running the pre-launch command");
            let mut hook = shell(script);
            hook.current_dir(self.paths.game(&instance.id))
                .envs(hooks.clone());
            crate::loaders::hide_console(&mut hook);
            let status = hook.status().await?;
            if !status.success() {
                return Err(Error::Other(format!(
                    "the pre-launch command failed with code {}",
                    status.code().unwrap_or(-1)
                )));
            }
        }

        // A wrapper like mangohud runs the game for us, so it goes first.
        let wrapper = instance
            .options
            .wrapper
            .as_deref()
            .map(split_command)
            .unwrap_or_default();
        let mut command = match wrapper.split_first() {
            Some((program, rest)) => {
                let mut command = Command::new(program);
                command.args(rest).arg(&java);
                command
            }
            None => Command::new(&java),
        };
        command
            .args(&args)
            .envs(instance.options.env.iter().map(|(key, value)| (key, value)))
            .envs(hooks)
            .current_dir(self.paths.game(&instance.id))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::loaders::hide_console(&mut command);
        let child = command.spawn()?;
        self.watch(instance.clone(), child);
        Ok(())
    }

    /// Follows a running game until it exits, then records the session and
    /// explains the crash if there was one.
    fn watch(&self, instance: Instance, mut child: Child) {
        let id = instance.id.clone();
        let events = self.events.clone();
        let core = self.clone();
        let stop = std::sync::Arc::new(tokio::sync::Notify::new());
        self.games
            .lock()
            .expect("game list poisoned")
            .insert(id.clone(), stop.clone());
        events(Event::GameStarted {
            instance: id.clone(),
            pid: child.id().unwrap_or(0),
        });

        for (stream, error) in [
            (child.stdout.take().map(boxed), false),
            (child.stderr.take().map(boxed), true),
        ] {
            let Some(stream) = stream else { continue };
            let (events, id) = (events.clone(), id.clone());
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                let mut reader = crate::gamelog::LogReader::default();
                while let Ok(Some(line)) = lines.next_line().await {
                    for line in reader.feed(&line) {
                        let error = error || line.is_error;
                        events(Event::GameLog {
                            instance: id.clone(),
                            line: line.text,
                            error,
                        });
                    }
                }
            });
        }

        tokio::spawn(async move {
            let began = Instant::now();
            let began_at = SystemTime::now();
            let status = tokio::select! {
                status = child.wait() => status.ok(),
                () = stop.notified() => {
                    let _ = child.kill().await;
                    None
                }
            };
            core.games.lock().expect("game list poisoned").remove(&id);

            let played_secs = began.elapsed().as_secs();
            let code = status.and_then(|s| s.code());
            let crashed = status.is_some_and(|s| !s.success());
            let crash = crashed.then(|| crash::inspect(&core.paths.game(&id), began_at));

            if let Some(script) = instance
                .options
                .post_exit
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                let mut hook = shell(script);
                hook.current_dir(core.paths.game(&id))
                    .envs(hook_environment(&core.paths, &instance));
                crate::loaders::hide_console(&mut hook);
                let _ = hook.status().await;
            }

            if let Ok(mut latest) = core.instance(&id) {
                latest.last_played = Some(OffsetDateTime::now_utc());
                latest.playtime_secs += played_secs;
                let _ = core.save_instance(&latest);
            }
            events(Event::GameExited {
                instance: id,
                code,
                played_secs,
                crash,
            });
        });
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
    use crate::mojang::{Arguments, VersionMeta};

    fn fixture(meta: VersionMeta) -> (Paths, Instance, Prepared) {
        let paths = Paths::new("/data");
        let instance = Instance {
            id: "pack".into(),
            name: "Pack".into(),
            minecraft: "1.21.4".into(),
            loader: Loader::default(),
            version_id: "1.21.4".into(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            last_played: None,
            playtime_secs: 0,
            memory_mb: None,
            jvm_args: vec!["-Dcustom=1".into()],
            java_path: None,
            icon: None,
            pack: None,
            shared: Vec::new(),
            group: None,
            options: LaunchOptions::default(),
        };
        let prepared = Prepared {
            meta,
            client_jar: "/data/versions/1.21.4/1.21.4.jar".into(),
            java: "/java".into(),
            natives: "/data/natives/1.21.4".into(),
            log_config: None,
            game_assets: "/data/assets".into(),
        };
        (paths, instance, prepared)
    }

    fn session() -> Session {
        Session {
            name: "Steve".into(),
            uuid: "abc".into(),
            access_token: "secret".into(),
            xuid: "1".into(),
        }
    }

    #[test]
    fn commands_split_like_a_shell() {
        assert_eq!(split_command("mangohud --dlsym"), ["mangohud", "--dlsym"]);
        assert_eq!(
            split_command("\"C:\\Program Files\\a.exe\" -x 'b c'"),
            ["C:\\Program Files\\a.exe", "-x", "b c"]
        );
        assert_eq!(split_command("run \"\" end"), ["run", "", "end"]);
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn the_game_window_can_be_sized_or_fullscreen() {
        let mut options = LaunchOptions {
            width: Some(1280),
            height: Some(720),
            ..LaunchOptions::default()
        };
        assert_eq!(
            window_arguments(&options),
            ["--width", "1280", "--height", "720"]
        );
        options.fullscreen = true;
        assert_eq!(window_arguments(&options), ["--fullscreen"]);
        assert!(
            window_arguments(&LaunchOptions {
                width: Some(1280),
                ..LaunchOptions::default()
            })
            .is_empty()
        );
    }

    #[tokio::test]
    async fn hook_commands_can_read_where_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.txt");
        let script = if cfg!(windows) {
            format!("echo %INST_NAME%> \"{}\"", out.display())
        } else {
            format!("echo $INST_NAME > '{}'", out.display())
        };
        let (paths, instance, _) = fixture(VersionMeta::default());
        let status = shell(&script)
            .envs(hook_environment(&paths, &instance))
            .status()
            .await
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read_to_string(out).unwrap().trim(), "Pack");
    }

    #[test]
    fn modern_versions_use_their_declared_arguments() {
        let meta = VersionMeta {
            main_class: Some("net.minecraft.client.main.Main".into()),
            arguments: Some(Arguments {
                game: vec![
                    Argument::Plain("--username".into()),
                    Argument::Plain("${auth_player_name}".into()),
                ],
                jvm: vec![
                    Argument::Plain("-cp".into()),
                    Argument::Plain("${classpath}".into()),
                ],
            }),
            ..VersionMeta::default()
        };
        let (paths, instance, prepared) = fixture(meta);

        let args = build_arguments(&paths, &instance, &prepared, &session(), 4096).unwrap();

        assert!(args.contains(&"-Xmx4096M".to_string()));
        assert!(args.contains(&"-Dcustom=1".to_string()));
        let main = args
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .unwrap();
        assert_eq!(args[main + 1..], ["--username", "Steve"]);
        let cp = args.iter().position(|a| a == "-cp").unwrap();
        assert!(args[cp + 1].ends_with("1.21.4.jar"));
    }

    #[test]
    fn a_loader_that_only_adds_flags_still_gets_a_classpath() {
        let meta = VersionMeta {
            main_class: Some("net.fabricmc.loader.impl.launch.knot.KnotClient".into()),
            arguments: Some(Arguments {
                game: vec![],
                jvm: vec![Argument::Plain("-Dfabric.gameVersion=1.8.9".into())],
            }),
            minecraft_arguments: Some("--username ${auth_player_name}".into()),
            ..VersionMeta::default()
        };
        let (paths, instance, prepared) = fixture(meta);

        let args = build_arguments(&paths, &instance, &prepared, &session(), 2048).unwrap();

        assert!(args.iter().any(|a| a == "-Dfabric.gameVersion=1.8.9"));
        assert!(args.iter().any(|a| a == "-cp"));
        assert!(args.ends_with(&["--username".to_string(), "Steve".to_string()]));
    }

    #[test]
    fn old_versions_get_a_classpath_and_split_their_argument_string() {
        let meta = VersionMeta {
            main_class: Some("net.minecraft.client.Minecraft".into()),
            minecraft_arguments: Some(
                "--username ${auth_player_name} --session ${auth_session}".into(),
            ),
            ..VersionMeta::default()
        };
        let (paths, instance, prepared) = fixture(meta);

        let args = build_arguments(&paths, &instance, &prepared, &session(), 2048).unwrap();

        assert!(args.iter().any(|a| a == "-cp"));
        assert!(args.iter().any(|a| a.starts_with("-Djava.library.path=")));
        let main = args
            .iter()
            .position(|a| a == "net.minecraft.client.Minecraft")
            .unwrap();
        assert_eq!(
            args[main + 1..],
            ["--username", "Steve", "--session", "secret"]
        );
    }
}

//! Turns a prepared version into a running game.
//!
//! Launching takes a `Session`, and outside debug builds the only way to get
//! one is a signed-in account with verified ownership (see `auth`). There is
//! no username-only path to play.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Instant, SystemTime};

use time::OffsetDateTime;
use tokio::io::BufReader;
use tokio::process::{Child, Command};

use crate::Core;
use crate::auth::Session;
use crate::brand::{PRODUCT_NAME, VERSION};
use crate::crash;
use crate::error::{Error, Result};
use crate::gamelog::next_line_lossy;
use crate::event::{Event, Task};
use crate::install::Prepared;
use crate::instance::{Instance, LaunchOptions};
use crate::mojang::{Argument, Features, OneOrMany, allowed};
use crate::paths::Paths;
use crate::waymarks::WaymarkKind;
use crate::worlds::{Join, join_arguments};

const CLASSPATH_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };
/// Minecraft's own default window, for versions that have to be told a size.
const DEFAULT_WIDTH: u32 = 854;
const DEFAULT_HEIGHT: u32 = 480;

/// The same collector settings Mojang's launcher defaults to.
const GC_ARGS: [&str; 6] = [
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+UseG1GC",
    "-XX:G1NewSizePercent=20",
    "-XX:G1ReservePercent=20",
    "-XX:MaxGCPauseMillis=50",
    "-XX:G1HeapRegionSize=32M",
];

/// How Toolscreen is told the Minecraft version when `--version` names a loader.
const TOOLSCREEN_VERSION: &str = "-Dtoolscreen.minecraftVersion=";

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
/// Toolscreen's setup, for one, runs itself with `$INST_JAVA`.
fn hook_environment(
    paths: &Paths,
    instance: &Instance,
    java: &std::path::Path,
) -> [(&'static str, String); 6] {
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
        ("INST_JAVA", java.to_string_lossy().into_owned()),
    ]
}

const HOOK_NAMES: [&str; 6] = [
    "INST_ID",
    "INST_NAME",
    "INST_DIR",
    "INST_MC_DIR",
    "INST_MC_VER",
    "INST_JAVA",
];

/// Prism and MultiMC hooks are written as `$INST_JAVA` or `${INST_JAVA}` on every
/// system, and cmd only reads `%INST_JAVA%`, so those names are turned into its
/// form. Anything else with a dollar sign is left as typed.
#[cfg_attr(not(windows), allow(dead_code))]
fn cmd_names(script: &str) -> String {
    let mut out = String::with_capacity(script.len());
    let mut rest = script;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let braced = after.strip_prefix('{');
        let found = HOOK_NAMES.iter().find_map(|name| match braced {
            Some(inner) => inner
                .strip_prefix(name)
                .and_then(|tail| tail.strip_prefix('}'))
                .map(|tail| (name, tail)),
            None => after
                .strip_prefix(name)
                .filter(|tail| {
                    !tail
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                })
                .map(|tail| (name, tail)),
        });
        match found {
            Some((name, tail)) => {
                out.push('%');
                out.push_str(name);
                out.push('%');
                rest = tail;
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// cmd has its own quoting, so a script has to reach it exactly as typed. Without
/// /S, a script that starts with a quote and has more than two, like
/// `"C:\My Tools\run.cmd" "a b"`, loses its first and last quote. With /S, cmd
/// removes exactly the pair put around it here. /D skips AutoRun scripts.
#[cfg(windows)]
fn shell(script: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("cmd");
    command
        .as_std_mut()
        .raw_arg("/D /S /C")
        .raw_arg(format!("\"{}\"", cmd_names(script)));
    command
}

#[cfg(not(windows))]
fn shell(script: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(script);
    command
}

/// The command that starts a game.
pub(crate) struct Launch {
    pub java: std::path::PathBuf,
    pub args: Vec<String>,
    /// Where `APPDATA` has to point, for versions that would otherwise use the real one.
    pub appdata: Option<std::path::PathBuf>,
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
        ("profile_name", instance.name.clone()),
        ("instance_icon", String::new()),
        (
            "resolution_width",
            instance.options.width.unwrap_or(DEFAULT_WIDTH).to_string(),
        ),
        (
            "resolution_height",
            instance
                .options
                .height
                .unwrap_or(DEFAULT_HEIGHT)
                .to_string(),
        ),
    ]);

    let mut args = vec![
        format!("-Xms{}M", memory_mb.min(1024)),
        format!("-Xmx{memory_mb}M"),
    ];
    args.extend(GC_ARGS.iter().map(ToString::to_string));
    // Toolscreen reads the game version from `--version`, which for a loader is its
    // profile id, like fabric-loader-0.16.14-1.16.1, and turns off resizing when it
    // can't read one. It also reads this property, and finds it first.
    if !instance
        .jvm_args
        .iter()
        .any(|arg| arg.starts_with(TOOLSCREEN_VERSION))
    {
        args.push(format!("{TOOLSCREEN_VERSION}{}", instance.minecraft));
    }
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
        self.cancel_check(id).await;
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

    /// Gets everything a launch needs onto disk and works out its command.
    pub(crate) async fn prepare_launch(
        &self,
        instance: &Instance,
        session: &Session,
        join: Option<&Join>,
        task: &Task,
    ) -> Result<Launch> {
        let extra = join.map_or(Ok(Vec::new()), |join| {
            join_arguments(join, &instance.minecraft)
        })?;
        let prepared = self.prepare_version(&instance.version_id, task).await?;
        self.add_java_fix(instance).await?;
        self.add_fml_libraries(instance, task).await?;
        self.skip_sound_restart(instance)?;
        self.link_resources(instance, &prepared).await?;

        let memory = match instance.memory_mb {
            Some(memory) => memory,
            None => self.settings()?.memory_mb,
        };
        let mut args = build_arguments(&self.paths, instance, &prepared, session, memory)?;
        args.extend(window_arguments(&instance.options));
        args.extend(extra);
        // Before 1.6 the game opens a log in the player's real `.minecraft` before
        // anything can move it. That folder is found from these, so they're pointed
        // at the instance. Windows reads the variable, and other systems the property.
        let appdata = prepared
            .resources
            .is_some()
            .then(|| self.paths.instance(&instance.id));
        if let Some(folder) = &appdata {
            args.insert(0, format!("-Duser.home={}", folder.display()));
        }
        let java: std::path::PathBuf = instance
            .java_path
            .clone()
            .map_or(prepared.java.clone(), Into::into);
        Ok(Launch {
            java,
            args,
            appdata,
        })
    }

    async fn start_game(
        &self,
        instance: &Instance,
        session: &Session,
        join: Option<&Join>,
        task: &Task,
    ) -> Result<()> {
        let Launch {
            java,
            args,
            appdata,
        } = self.prepare_launch(instance, session, join, task).await?;

        // Before the waymark, so it records exactly what the game starts with.
        if let Err(error) = self.share_settings_in(instance) {
            tracing::warn!(%error, "could not share game settings with this instance");
        }

        task.progress(0, 1, "Setting a waymark");
        let trail = self.trail(&instance.id);
        tokio::task::spawn_blocking(move || trail.mark("Before playing", WaymarkKind::Launch))
            .await
            .map_err(|e| Error::Other(e.to_string()))??;

        let hooks = hook_environment(&self.paths, instance, &java);

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
        if let Some(folder) = &appdata {
            command.env("APPDATA", folder);
        }
        command
            .args(&args)
            .envs(instance.options.env.iter().map(|(key, value)| (key, value)))
            .envs(hooks.clone())
            .current_dir(self.paths.game(&instance.id))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::loaders::hide_console(&mut command);
        let child = command.spawn()?;
        self.show_playing(instance, join);
        self.watch(instance.clone(), child, hooks);
        Ok(())
    }

    /// Follows a running game until it exits, then records the session and
    /// explains the crash if there was one.
    fn watch(&self, instance: Instance, mut child: Child, hooks: [(&'static str, String); 6]) {
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
                let mut lines = BufReader::new(stream);
                let mut reader = crate::gamelog::LogReader::default();
                while let Some(line) = next_line_lossy(&mut lines).await {
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
            core.stop_playing(&id);
            if let Err(error) = core.share_settings_out(&instance) {
                tracing::warn!(%error, "could not take back this instance's game settings");
            }

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
                hook.current_dir(core.paths.game(&id)).envs(hooks);
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
            keeps_own_settings: false,
            server: None,
            options: LaunchOptions::default(),
        };
        let prepared = Prepared {
            meta,
            client_jar: "/data/versions/1.21.4/1.21.4.jar".into(),
            java: "/java".into(),
            natives: "/data/natives/1.21.4".into(),
            log_config: None,
            game_assets: "/data/assets".into(),
            resources: None,
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
            .envs(hook_environment(
                &paths,
                &instance,
                std::path::Path::new("java"),
            ))
            .status()
            .await
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read_to_string(out).unwrap().trim(), "Pack");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_hook_can_start_with_a_quoted_path_that_has_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("My Tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(tools.join("say.cmd"), "@echo %~1> \"%~2\"\r\n").unwrap();
        let out = dir.path().join("said here.txt");
        let script = format!(
            "\"{}\" \"hello there\" \"{}\"",
            tools.join("say.cmd").display(),
            out.display()
        );
        let status = shell(&script).status().await.unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read_to_string(out).unwrap().trim(), "hello there");
    }

    /// The command Toolscreen's installer writes into a Waymark instance, run the way
    /// Waymark runs it, with a stand-in for java that reports what it was given.
    #[cfg(windows)]
    #[tokio::test]
    async fn toolscreen_s_command_reaches_java_intact() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("Java Runtime");
        std::fs::create_dir(&runtime).unwrap();
        let out = dir.path().join("args.txt");
        std::fs::write(
            runtime.join("java.cmd"),
            format!("@echo %1^|%~2^|%3> \"{}\"\r\n", out.display()),
        )
        .unwrap();
        let (paths, instance, _) = fixture(VersionMeta::default());
        let script = r#""$INST_JAVA" -jar "$INST_MC_DIR/Toolscreen.jar" --prelaunch"#;
        let status = shell(script)
            .envs(hook_environment(
                &paths,
                &instance,
                &runtime.join("java.cmd"),
            ))
            .status()
            .await
            .unwrap();
        assert!(status.success());
        let game = paths.game(&instance.id);
        assert_eq!(
            std::fs::read_to_string(out).unwrap().trim(),
            format!("-jar|{}/Toolscreen.jar|--prelaunch", game.display())
        );
    }

    #[test]
    fn prism_style_names_become_cmd_names() {
        assert_eq!(
            cmd_names(r#""$INST_JAVA" -jar "${INST_MC_DIR}/Toolscreen.jar""#),
            r#""%INST_JAVA%" -jar "%INST_MC_DIR%/Toolscreen.jar""#
        );
        assert_eq!(
            cmd_names(r"$INST_DIR\x $INST_ID."),
            r"%INST_DIR%\x %INST_ID%."
        );
        // Only the names a launcher sets, and only whole ones.
        assert_eq!(
            cmd_names("$INST_DIRECTORY $HOME ${INST_JAVA"),
            "$INST_DIRECTORY $HOME ${INST_JAVA"
        );
        assert_eq!(cmd_names("cost $5 and $"), "cost $5 and $");
        assert_eq!(cmd_names("%INST_NAME% stays"), "%INST_NAME% stays");
    }

    #[tokio::test]
    async fn a_prism_style_hook_reads_the_java_it_will_run_with() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("java.txt");
        let script = if cfg!(windows) {
            format!("echo $INST_JAVA> \"{}\"", out.display())
        } else {
            format!("echo $INST_JAVA > '{}'", out.display())
        };
        let (paths, instance, _) = fixture(VersionMeta::default());
        let java = dir.path().join("runtime").join("javaw.exe");
        let status = shell(&script)
            .envs(hook_environment(&paths, &instance, &java))
            .status()
            .await
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(out).unwrap().trim(),
            java.to_string_lossy()
        );
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

    /// Toolscreen can't read a version out of a loader's id, and without one it
    /// turns off resizing, so it's told the Minecraft version before `--version`.
    #[test]
    fn toolscreen_is_told_the_minecraft_version_of_a_loader_instance() {
        let meta = VersionMeta {
            main_class: Some("net.fabricmc.loader.impl.launch.knot.KnotClient".into()),
            minecraft_arguments: Some("--version ${version_name}".into()),
            ..VersionMeta::default()
        };
        let (paths, mut instance, prepared) = fixture(meta);
        instance.minecraft = "1.16.1".into();
        instance.version_id = "fabric-loader-0.16.14-1.16.1".into();

        let args = build_arguments(&paths, &instance, &prepared, &session(), 2048).unwrap();
        let told = args
            .iter()
            .position(|a| a == "-Dtoolscreen.minecraftVersion=1.16.1")
            .expect("the property is passed");
        let version = args.iter().position(|a| a == "--version").unwrap();
        assert!(told < version);
        assert_eq!(args[version + 1], "fabric-loader-0.16.14-1.16.1");

        // A version the player set themselves is the only one passed.
        instance.jvm_args = vec!["-Dtoolscreen.minecraftVersion=1.16.5".into()];
        let args = build_arguments(&paths, &instance, &prepared, &session(), 2048).unwrap();
        let passed: Vec<_> = args
            .iter()
            .filter(|a| a.starts_with("-Dtoolscreen.minecraftVersion="))
            .collect();
        assert_eq!(passed, ["-Dtoolscreen.minecraftVersion=1.16.5"]);
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

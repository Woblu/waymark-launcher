//! After an update, starts the game out of sight to see whether it still loads, and
//! goes back to the last waymark by itself if it doesn't. The player can keep doing
//! whatever they're doing, because the check runs on a desktop nobody is looking at.
//!
//! It proves the game gets through loading. It can't prove a world plays well, and
//! a check that runs out of time keeps the update, because a slow pack isn't a broken one.

use std::time::Duration;

use serde::Serialize;

use crate::Core;
use crate::error::Result;

const CANCEL_TICK: Duration = Duration::from_millis(50);

/// What the player gets told.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum StartCheck {
    Passed,
    /// The update stopped the game loading, so it's been undone.
    Undone {
        reason: String,
    },
    /// Still loading when time ran out, or the game closed without saying
    /// why, so the update stayed.
    Unclear,
    /// The update stopped the game loading, but the instance changed while
    /// the check ran, so nothing was undone automatically.
    Failed {
        reason: String,
    },
    /// Not checked: turned off, cancelled, not signed in, or not possible on this system.
    Skipped,
}

#[cfg(windows)]
mod check {
    use std::ffi::OsString;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime};

    use tokio::sync::Notify;

    use super::StartCheck;
    use crate::Core;
    use crate::auth::Session;
    use crate::crash;
    use crate::error::Result;
    use crate::event::Task;
    use crate::hidden_game::HiddenGame;
    use crate::instance::Instance;

    const LOG: &str = ".waymarks/start-check.log";
    /// Lines a game only reaches once its mods have loaded.
    const LOADED: [&str; 3] = [
        "Sound engine started",
        "OpenAL initialized",
        "Forge Mod Loader has successfully loaded",
    ];
    /// Lines that mean it's over. Loaders often show these in an error window and
    /// wait for a click, which nobody could give on this desktop, so waiting for
    /// the game to exit isn't enough.
    const BROKEN: [&str; 8] = [
        "Some of your mods are incompatible",
        "Incompatible mods found",
        "Could not execute entrypoint",
        "Failed to create mod instance",
        "LoadingFailedException",
        "Missing or unsupported mandatory dependencies",
        "---- Minecraft Crash Report ----",
        "Crash report saved to",
    ];
    /// How long it has to stay up after that, because some mods fail just afterwards.
    const SETTLE: Duration = Duration::from_secs(15);
    /// Big packs can take minutes, more so at low priority.
    const LIMIT: Duration = Duration::from_secs(420);
    const TICK: Duration = Duration::from_secs(1);
    /// Enough of the last read to catch a line split across two.
    const OVERLAP: usize = 80;

    enum Outcome {
        Loaded,
        Failed(String),
        /// Closed with no crash report or loader error. On a hidden desktop that
        /// is as likely the graphics or sound driver as the update.
        Unexplained,
        OutOfTime,
        Cancelled,
    }

    /// What has been written since the last look, with a little of what came before.
    struct LogTail {
        seen: u64,
        kept: String,
    }

    impl LogTail {
        fn read(&mut self, path: &Path) -> Option<String> {
            let mut file = std::fs::File::open(path).ok()?;
            file.seek(SeekFrom::Start(self.seen)).ok()?;
            let mut bytes = Vec::new();
            self.seen += file.read_to_end(&mut bytes).ok()? as u64;
            let fresh = format!("{}{}", self.kept, String::from_utf8_lossy(&bytes));
            let tail = fresh.len().saturating_sub(OVERLAP);
            let tail = (tail..=fresh.len())
                .find(|at| fresh.is_char_boundary(*at))
                .unwrap_or(fresh.len());
            self.kept = fresh[tail..].to_string();
            Some(fresh)
        }
    }

    impl Core {
        /// Starts the game unseen and reports whether it got through loading. The update
        /// is the newest waymark, so the one before it is what a failure goes back to.
        pub async fn check_start_with(&self, id: &str, session: Session) -> Result<StartCheck> {
            let instance = self.instance(id)?;
            let trail = self.trail(id);
            let marks = trail.list()?;
            let Some(before) = marks.iter().rev().nth(1).map(|mark| mark.id.clone()) else {
                return Ok(StartCheck::Skipped);
            };
            let update = marks.last().map(|mark| mark.id.clone());
            let task = Task::start(
                &self.events,
                format!("Checking that {} still starts", instance.name),
            );
            let stop = Arc::new(Notify::new());
            self.checks
                .lock()
                .expect("check list poisoned")
                .insert(id.to_string(), stop.clone());
            let outcome = self.watch_hidden(&instance, &session, &stop, &task).await;
            // The check stays listed until any undo is done, so pressing Play in
            // the meantime waits for it instead of starting mid-swap.
            let result = self.settle_check(&trail, outcome, &before, update.as_deref(), task);
            self.checks.lock().expect("check list poisoned").remove(id);
            result
        }

        fn settle_check(
            &self,
            trail: &crate::waymarks::Trail,
            outcome: Result<Outcome>,
            before: &str,
            update: Option<&str>,
            task: Task,
        ) -> Result<StartCheck> {
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    task.fail(error.to_string());
                    return Err(error);
                }
            };
            task.finish();
            Ok(match outcome {
                Outcome::Loaded => StartCheck::Passed,
                Outcome::OutOfTime | Outcome::Unexplained => StartCheck::Unclear,
                Outcome::Cancelled => StartCheck::Skipped,
                Outcome::Failed(reason) => {
                    // Anything the player did during the check is newer than the
                    // update, and undoing it behind their back would be worse.
                    let newest = trail.list()?.last().map(|mark| mark.id.clone());
                    if newest.as_deref() == update {
                        trail.restore(before)?;
                        StartCheck::Undone { reason }
                    } else {
                        StartCheck::Failed { reason }
                    }
                }
            })
        }

        async fn watch_hidden(
            &self,
            instance: &Instance,
            session: &Session,
            stop: &Notify,
            task: &Task,
        ) -> Result<Outcome> {
            let launch = self.prepare_launch(instance, session, None, task).await?;
            let game_dir = self.paths.game(&instance.id);
            let log = self.paths.instance(&instance.id).join(LOG);
            if let Some(folder) = log.parent() {
                std::fs::create_dir_all(folder)?;
            }
            let mut environ: Vec<(OsString, OsString)> = instance
                .options
                .env
                .iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect();
            if let Some(folder) = &launch.appdata {
                environ.push(("APPDATA".into(), folder.into()));
            }
            task.progress(0, 1, "Checking that the game still starts");
            let began = SystemTime::now();
            let started = Instant::now();
            let game = HiddenGame::start(&launch.java, &launch.args, &environ, &game_dir, &log)?;
            let mut tail = LogTail {
                seen: 0,
                kept: String::new(),
            };
            let mut loaded: Option<Instant> = None;
            loop {
                tokio::select! {
                    () = stop.notified() => {
                        game.stop();
                        return Ok(Outcome::Cancelled);
                    }
                    () = tokio::time::sleep(TICK) => {}
                }
                let text = tail.read(&log).unwrap_or_default();
                if let Some(line) = broken_line(&text) {
                    game.stop();
                    return Ok(Outcome::Failed(line));
                }
                if loaded.is_none() && LOADED.iter().any(|line| text.contains(line)) {
                    loaded = Some(Instant::now());
                }
                if game.exited() {
                    return Ok(match reason_for(&game_dir, began) {
                        Some(reason) => Outcome::Failed(reason),
                        None => Outcome::Unexplained,
                    });
                }
                if loaded.is_some_and(|at| at.elapsed() >= SETTLE) {
                    game.stop();
                    return Ok(Outcome::Loaded);
                }
                if started.elapsed() >= LIMIT {
                    game.stop();
                    return Ok(Outcome::OutOfTime);
                }
            }
        }
    }

    /// The game's own words for what went wrong. Newer versions log as XML, so the
    /// line is taken from the first of the tags around it that holds any text.
    fn broken_line(text: &str) -> Option<String> {
        let line = text
            .lines()
            .find(|line| BROKEN.iter().any(|mark| line.contains(mark)))?;
        let line = line.rsplit("<![CDATA[").next().unwrap_or(line);
        let line = line.split("]]>").next().unwrap_or(line);
        let line = line
            .split_once("Exception: ")
            .map_or(line, |(_, rest)| rest);
        Some(line.trim().to_string())
    }

    fn reason_for(game_dir: &Path, began: SystemTime) -> Option<String> {
        crash::inspect(game_dir, began)
            .findings
            .first()
            .map(|finding| finding.title.clone())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_loader_s_error_is_found_in_plain_and_xml_logs() {
            let xml = "<log4j:Event>\n<log4j:Throwable><![CDATA[net.fabricmc.loader.impl.FormattedException: Some of your mods are incompatible with the game or each other!\n";
            assert_eq!(
                broken_line(xml).unwrap(),
                "Some of your mods are incompatible with the game or each other!"
            );
            let plain = "[main/INFO]: fine\n---- Minecraft Crash Report ----\n";
            assert_eq!(
                broken_line(plain).unwrap(),
                "---- Minecraft Crash Report ----"
            );
        }

        #[test]
        fn a_healthy_log_is_left_alone() {
            let log = "[Render thread/INFO]: Sound engine started\n[main/WARN]: Missing sound for event\n";
            assert_eq!(broken_line(log), None);
        }

        #[test]
        fn a_line_split_across_two_reads_is_still_seen() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("log");
            let mut tail = LogTail {
                seen: 0,
                kept: String::new(),
            };
            std::fs::write(&path, "first line\nSound eng").unwrap();
            assert!(!tail.read(&path).unwrap().contains("Sound engine started"));
            std::fs::write(&path, "first line\nSound engine started\n").unwrap();
            assert!(tail.read(&path).unwrap().contains("Sound engine started"));
        }
    }
}

impl Core {
    /// Checks the update just made to an instance, if this computer and the player's
    /// settings allow it. Signing in is needed like any launch, so there's no way
    /// around owning the game here.
    pub async fn check_start(&self, id: &str) -> Result<StartCheck> {
        let busy = self.game_running(id)
            || self
                .checks
                .lock()
                .expect("check list poisoned")
                .contains_key(id);
        if busy || !self.settings()?.check_updates {
            return Ok(StartCheck::Skipped);
        }
        #[cfg(windows)]
        {
            let Ok(session) = self.session().await else {
                return Ok(StartCheck::Skipped);
            };
            self.check_start_with(id, session).await
        }
        #[cfg(not(windows))]
        Ok(StartCheck::Skipped)
    }

    /// Playing comes first, so pressing Play ends a check rather than waiting for it.
    pub(crate) async fn cancel_check(&self, id: &str) {
        let running = |core: &Core| {
            core.checks
                .lock()
                .expect("check list poisoned")
                .get(id)
                .cloned()
        };
        let Some(stop) = running(self) else {
            return;
        };
        stop.notify_one();
        while running(self).is_some() {
            tokio::time::sleep(CANCEL_TICK).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Notify;

    use crate::Core;

    #[tokio::test]
    async fn play_ends_a_check_and_waits_for_it_to_finish() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let stop = Arc::new(Notify::new());
        core.checks
            .lock()
            .unwrap()
            .insert("pack".to_string(), stop.clone());
        let check = core.clone();
        tokio::spawn(async move {
            stop.notified().await;
            check.checks.lock().unwrap().remove("pack");
        });
        core.cancel_check("pack").await;
        assert!(core.checks.lock().unwrap().is_empty());
        core.cancel_check("no such check").await;
    }
}

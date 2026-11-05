//! The launcher's own log and crash notes, so a report of "it just closed"
//! comes with something to read. Everything stays in the data folder; nothing
//! is sent anywhere unless the player copies it out themselves.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::brand::{PRODUCT_NAME, VERSION};
use crate::logs::redact;

const LOG_FILE: &str = "launcher.log";
const OLD_LOG_FILE: &str = "launcher.old.log";
const CRASH_FILE: &str = "last-crash.txt";
const SEEN_CRASH_FILE: &str = "last-crash.seen.txt";
/// Two files of this size are kept, so the log can never grow past about 10 MB.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
/// Enough to show what led up to a problem without pasting a novel.
const REPORT_LINES: usize = 300;

pub fn folder(root: &Path) -> PathBuf {
    root.join("logs")
}

/// Starts writing the log and installs the crash note. Call once, early. A
/// failure here is reported but never stops the launcher from opening.
pub fn start(root: &Path) -> io::Result<()> {
    let folder = folder(root);
    fs::create_dir_all(&folder)?;
    let writer = Rotating::open(folder.join(LOG_FILE), folder.join(OLD_LOG_FILE))?;
    let level = match std::env::var("WAYMARK_LOG").as_deref() {
        Ok("debug") => tracing::Level::DEBUG,
        Ok("trace") => tracing::Level::TRACE,
        _ => tracing::Level::INFO,
    };
    let installed = tracing_subscriber::fmt()
        .with_writer(Mutex::new(writer))
        .with_ansi(false)
        .with_target(true)
        .with_max_level(level)
        .try_init()
        .is_ok();
    install_panic_hook(folder);
    if installed {
        tracing::info!(
            version = VERSION,
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            "{PRODUCT_NAME} started"
        );
    }
    Ok(())
}

/// The note from a crash the player hasn't seen yet, which is then marked seen
/// so it's shown once.
pub fn take_last_crash(root: &Path) -> Option<String> {
    let folder = folder(root);
    let note = fs::read_to_string(folder.join(CRASH_FILE)).ok()?;
    let _ = fs::rename(folder.join(CRASH_FILE), folder.join(SEEN_CRASH_FILE));
    Some(note)
}

/// Plain text to paste into a support email or an issue: versions, the last
/// crash if any, and the end of the log, with folder names and tokens removed.
pub fn report(root: &Path) -> String {
    let folder = folder(root);
    let mut out = format!(
        "{PRODUCT_NAME} {VERSION}\nSystem: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let crash = fs::read_to_string(folder.join(CRASH_FILE))
        .or_else(|_| fs::read_to_string(folder.join(SEEN_CRASH_FILE)));
    if let Ok(crash) = crash {
        out.push_str("\nLast crash:\n");
        out.push_str(crash.trim_end());
        out.push('\n');
    }
    let log = fs::read_to_string(folder.join(LOG_FILE)).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.len().saturating_sub(REPORT_LINES);
    out.push_str(&format!("\nLog (last {} lines):\n", lines.len() - start));
    for line in &lines[start..] {
        out.push_str(line);
        out.push('\n');
    }
    scrub(&out)
}

/// Takes out the player's home folder (which holds their name) and tokens.
fn scrub(text: &str) -> String {
    let home = directories::BaseDirs::new().map(|base| base.home_dir().to_path_buf());
    redact(text, home.as_deref())
}

fn install_panic_hook(folder: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Release builds abort right after this, so write it down now and
        // keep it simple: no locks the panicking code might already hold.
        let when = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        let thread = std::thread::current();
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown".into());
        let note = format!(
            "{PRODUCT_NAME} {VERSION} stopped unexpectedly at {when}\n\
             System: {} {}\nThread: {}\nWhere: {location}\nWhat: {message}\n\n{}\n",
            std::env::consts::OS,
            std::env::consts::ARCH,
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture(),
        );
        let _ = fs::write(folder.join(CRASH_FILE), scrub(&note));
        if let Ok(mut log) = OpenOptions::new()
            .append(true)
            .create(true)
            .open(folder.join(LOG_FILE))
        {
            let _ = writeln!(log, "{when} PANIC {location}: {}", scrub(&message));
        }
        previous(info);
    }));
}

/// A log file that starts over in a second file once it's big enough.
struct Rotating {
    path: PathBuf,
    old: PathBuf,
    file: File,
    written: u64,
}

impl Rotating {
    fn open(path: PathBuf, old: PathBuf) -> io::Result<Self> {
        let file = OpenOptions::new().append(true).create(true).open(&path)?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Rotating {
            path,
            old,
            file,
            written,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let _ = fs::remove_file(&self.old);
        fs::rename(&self.path, &self.old)?;
        self.file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        self.written = 0;
        Ok(())
    }
}

impl Write for Rotating {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.written + bytes.len() as u64 > MAX_LOG_BYTES {
            // A file another program holds open can't be renamed on Windows;
            // carry on in the same file rather than lose the line.
            let _ = self.rotate();
        }
        let n = self.file.write(bytes)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_starts_over_instead_of_growing_forever() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = Rotating::open(dir.path().join(LOG_FILE), dir.path().join(OLD_LOG_FILE))
            .unwrap();
        let line = vec![b'x'; 1024 * 1024];
        for _ in 0..12 {
            log.write_all(&line).unwrap();
        }
        let size = |name| fs::metadata(dir.path().join(name)).unwrap().len();
        assert!(size(LOG_FILE) <= MAX_LOG_BYTES);
        assert!(size(OLD_LOG_FILE) <= MAX_LOG_BYTES);
    }

    #[test]
    fn a_crash_note_is_shown_once_and_stays_in_the_report() {
        let dir = tempfile::tempdir().unwrap();
        let logs = folder(dir.path());
        fs::create_dir_all(&logs).unwrap();
        fs::write(logs.join(CRASH_FILE), "What: boom\n").unwrap();
        fs::write(logs.join(LOG_FILE), "line one\nline two\n").unwrap();

        assert_eq!(take_last_crash(dir.path()).as_deref(), Some("What: boom\n"));
        assert_eq!(take_last_crash(dir.path()), None);
        let report = report(dir.path());
        assert!(report.contains("What: boom"));
        assert!(report.contains("line two"));
        assert!(report.starts_with(PRODUCT_NAME));
    }
}

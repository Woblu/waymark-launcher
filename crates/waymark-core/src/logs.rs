//! Log files from earlier sessions, and sending one to mclo.gs for help. Logs
//! can carry a player's folder names and tokens, so they're cleaned before
//! anything leaves the machine, and only ever after the player asks.

use std::fs;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::Core;
use crate::error::{Error, Result};
use crate::install::safe_relative;

const UPLOAD: &str = "https://api.mclo.gs/1/log";
/// The service's own limit.
const MAX_UPLOAD_BYTES: usize = 10 << 20;
/// Enough for any real log, without letting a runaway one freeze the window.
const MAX_READ_BYTES: u64 = 32 << 20;
const FOLDERS: [&str; 2] = ["logs", "crash-reports"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    /// `folder/file`, the name to ask for it by.
    pub path: String,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: u64,
}

#[derive(Deserialize)]
struct UploadReply {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Takes out what shouldn't leave the machine. mclo.gs filters logs too, but
/// that happens after they've arrived.
pub fn redact(text: &str, home: Option<&Path>) -> String {
    let mut out = String::with_capacity(text.len());
    let homes: Vec<String> = home.map_or_else(Vec::new, |home| {
        let path = home.to_string_lossy().into_owned();
        vec![
            path.clone(),
            path.replace('\\', "/"),
            path.replace('\\', "\\\\"),
        ]
    });
    for line in text.lines() {
        let mut line = line.to_string();
        for path in &homes {
            line = line.replace(path.as_str(), "~");
        }
        out.push_str(&hide_after(
            &line,
            &[
                "--accessToken",
                "accessToken:",
                "Session ID is token:",
                "--uuid",
                "--xuid",
            ],
        ));
        out.push('\n');
    }
    out
}

fn hide_after(line: &str, markers: &[&str]) -> String {
    let mut line = line.to_string();
    for marker in markers {
        let Some(start) = line.find(marker) else {
            continue;
        };
        let value = start + marker.len();
        let rest = &line[value..];
        let skipped = rest.len() - rest.trim_start().len();
        let length = rest
            .trim_start()
            .find([' ', ',', ']'])
            .unwrap_or(rest.trim_start().len());
        line.replace_range(value + skipped..value + skipped + length, "[hidden]");
    }
    line
}

impl Core {
    /// The instance's saved logs and crash reports, newest first.
    pub fn log_files(&self, id: &str) -> Result<Vec<LogFile>> {
        let game = self.paths.game(id);
        let mut files = Vec::new();
        for folder in FOLDERS {
            let Ok(entries) = fs::read_dir(game.join(folder)) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let useful =
                    name.ends_with(".log") || name.ends_with(".log.gz") || name.ends_with(".txt");
                let Ok(meta) = entry.metadata() else { continue };
                if !useful || !meta.is_file() {
                    continue;
                }
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |at| at.as_secs());
                files.push(LogFile {
                    path: format!("{folder}/{name}"),
                    size: meta.len(),
                    modified,
                });
            }
        }
        files.sort_by_key(|file| std::cmp::Reverse(file.modified));
        Ok(files)
    }

    pub fn read_log(&self, id: &str, path: &str) -> Result<String> {
        let relative = safe_relative(path)?;
        let parts: Vec<_> = relative
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect();
        if parts.len() != 2 || !FOLDERS.contains(&parts[0].as_str()) {
            return Err(Error::NotFound(path.to_string()));
        }
        let full = self.paths.game(id).join(relative);
        let file = fs::File::open(&full)?;
        let mut bytes = Vec::new();
        if path.ends_with(".gz") {
            GzDecoder::new(file)
                .take(MAX_READ_BYTES)
                .read_to_end(&mut bytes)?;
        } else {
            file.take(MAX_READ_BYTES).read_to_end(&mut bytes)?;
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Sends a log to mclo.gs and returns its address.
    pub async fn upload_log(&self, text: &str) -> Result<String> {
        let home = directories::BaseDirs::new().map(|base| base.home_dir().to_path_buf());
        let mut cleaned = redact(text, home.as_deref());
        if cleaned.len() > MAX_UPLOAD_BYTES {
            // The end of a log is where the problem is.
            let mut start = cleaned.len() - MAX_UPLOAD_BYTES;
            while !cleaned.is_char_boundary(start) {
                start += 1;
            }
            cleaned = cleaned.split_off(start);
        }
        let reply: UploadReply = self
            .net
            .client()
            .post(UPLOAD)
            .form(&[("content", cleaned)])
            .send()
            .await?
            .json()
            .await?;
        match reply {
            UploadReply {
                success: true,
                url: Some(url),
                ..
            } => Ok(url),
            UploadReply { error, .. } => Err(Error::Other(
                error.unwrap_or_else(|| "mclo.gs didn't accept the log".into()),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::Loader;
    use std::io::Write;

    #[test]
    fn folder_names_and_tokens_are_taken_out() {
        let log = "[main] Loading C:\\Users\\boaz\\AppData\\mods\nC:/Users/boaz/x\n--username Steve --accessToken abc.def --uuid 1234 --version 1\nSession ID is token:secret:hash, more\n";
        let cleaned = redact(log, Some(Path::new("C:\\Users\\boaz")));
        assert!(!cleaned.contains("boaz"), "{cleaned}");
        assert!(!cleaned.contains("abc.def"), "{cleaned}");
        assert!(!cleaned.contains("1234"), "{cleaned}");
        assert!(!cleaned.contains("secret"), "{cleaned}");
        assert!(cleaned.contains("--version 1"));
        assert!(cleaned.contains("~\\AppData\\mods"));
    }

    #[test]
    fn a_line_ending_in_a_marker_is_left_alone() {
        assert_eq!(
            hide_after("--accessToken", &["--accessToken"]),
            "--accessToken[hidden]"
        );
    }

    #[test]
    fn reads_plain_and_gzipped_logs_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path(), crate::event::silent()).unwrap();
        let instance = core
            .register_instance("Test", "1.21.4", Loader::default(), "x".into(), None)
            .unwrap();
        let game = core.paths.game(&instance.id);
        fs::create_dir_all(game.join("logs")).unwrap();
        fs::write(game.join("logs/latest.log"), "hello").unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(b"older").unwrap();
        fs::write(game.join("logs/2026-01-01-1.log.gz"), gz.finish().unwrap()).unwrap();
        fs::write(game.join("logs/secret.json"), "{}").unwrap();

        assert_eq!(core.log_files(&instance.id).unwrap().len(), 2);
        assert_eq!(
            core.read_log(&instance.id, "logs/latest.log").unwrap(),
            "hello"
        );
        assert_eq!(
            core.read_log(&instance.id, "logs/2026-01-01-1.log.gz")
                .unwrap(),
            "older"
        );
        for path in [
            "../instance.toml",
            "logs/../../instance.toml",
            "mods/a.jar",
            "config/x/y.log",
        ] {
            assert!(core.read_log(&instance.id, path).is_err(), "{path}");
        }
    }
}

//! Keeps Waymark current. New versions download in the background and install
//! when the app closes, or sooner if the player asks. An update never cuts a
//! game off, because the installer has to stop this process to replace it, and
//! the watching of a game's log and crashes happens here.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Update, UpdaterExt};
use waymark_core::Error;
use waymark_core::brand::RELEASES_API;

use crate::Launcher;

/// Long enough that the check doesn't compete with the app starting.
const FIRST_CHECK: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const READY_EVENT: &str = "waymark://update";

struct Ready {
    update: Update,
    bytes: Vec<u8>,
}

/// A verified update waiting for a moment it can install.
#[derive(Default)]
pub struct Updates {
    ready: Mutex<Option<Ready>>,
}

impl Updates {
    fn version(&self) -> Option<String> {
        let ready = self.ready.lock().expect("update slot poisoned");
        ready.as_ref().map(|ready| ready.update.version.clone())
    }

    fn take(&self) -> Option<Ready> {
        self.ready.lock().expect("update slot poisoned").take()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    current: String,
    ready: Option<String>,
}

fn busy(app: &AppHandle) -> bool {
    let core = &app.state::<Launcher>().core;
    core.any_game_running() || core.any_host_running()
}

/// Whether something is running that closing the app would disturb.
pub fn exit_would_interrupt(app: &AppHandle) -> bool {
    busy(app)
}

/// Downloads a newer version if there is one. The plugin checks its signature
/// before this returns, and again before it installs.
async fn fetch(app: &AppHandle) -> tauri_plugin_updater::Result<Option<String>> {
    let updater = match beta_endpoint(app).await {
        Some(endpoint) => app.updater_builder().endpoints(vec![endpoint])?.build()?,
        None => app.updater()?,
    };
    let Some(update) = updater.check().await? else {
        return Ok(None);
    };
    let version = update.version.clone();
    if app.state::<Updates>().version().as_deref() == Some(version.as_str()) {
        return Ok(Some(version));
    }
    let bytes = update.download(|_, _| {}, || {}).await?;
    *app.state::<Updates>()
        .ready
        .lock()
        .expect("update slot poisoned") = Some(Ready { update, bytes });
    let _ = app.emit(READY_EVENT, &version);
    Ok(Some(version))
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// For players who turned on beta updates: the manifest of the newest published
/// release, pre-releases included. Anyone else, or any trouble reaching GitHub,
/// falls back to the usual address. Either way the plugin checks the signature.
async fn beta_endpoint(app: &AppHandle) -> Option<Url> {
    let core = &app.state::<Launcher>().core;
    if !core.settings().is_ok_and(|settings| settings.beta_updates) {
        return None;
    }
    let releases: Vec<Release> = core.net.json(RELEASES_API).await.ok()?;
    newest_manifest(releases)?.parse().ok()
}

fn newest_manifest(releases: Vec<Release>) -> Option<String> {
    releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let version = semver::Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
            let manifest = release
                .assets
                .into_iter()
                .find(|asset| asset.name == "latest.json")?;
            Some((version, manifest.browser_download_url))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, url)| url)
}

fn wanted(app: &AppHandle) -> bool {
    let core = &app.state::<Launcher>().core;
    core.settings().is_ok_and(|settings| settings.auto_update)
}

/// Checks on a timer for as long as the app runs. A failed check says nothing,
/// because being offline isn't a problem.
pub fn watch(app: AppHandle) {
    if tauri::is_dev() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if wanted(&app) {
                let _ = fetch(&app).await;
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Installs what's waiting as the app closes, without opening it again.
pub fn install_on_exit(app: &AppHandle) {
    if busy(app) {
        return;
    }
    if let Some(Ready { update, bytes }) = app.state::<Updates>().take() {
        let _ = update.restart_after_install(false).install(&bytes);
    }
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    UpdateStatus {
        current: app.package_info().version.to_string(),
        ready: app.state::<Updates>().version(),
    }
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<String>, Error> {
    fetch(&app)
        .await
        .map_err(|error| Error::Other(error.to_string()))
}

#[tauri::command]
pub fn install_update(app: AppHandle) -> Result<(), Error> {
    if busy(&app) {
        return Err(Error::Other(
            "Close the game and any hosted server first".into(),
        ));
    }
    let Some(Ready { update, bytes }) = app.state::<Updates>().take() else {
        return Err(Error::Other("There's no update ready to install".into()));
    };
    update
        .install(&bytes)
        .map_err(|error| Error::Other(error.to_string()))?;
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, manifest: bool) -> Release {
        let mut assets = vec![Asset {
            name: "Waymark.Launcher_x64-setup.exe".into(),
            browser_download_url: format!("https://example.com/{tag}/setup.exe"),
        }];
        if manifest {
            assets.push(Asset {
                name: "latest.json".into(),
                browser_download_url: format!("https://example.com/{tag}/latest.json"),
            });
        }
        Release {
            tag_name: tag.into(),
            draft,
            assets,
        }
    }

    #[test]
    fn beta_players_get_the_newest_published_version_with_a_manifest() {
        let releases = vec![
            release("v1.1.0-beta.1", true, true),
            release("v1.0.0-beta.2", false, true),
            release("v1.0.0", false, true),
            release("v1.0.1-beta.1", false, false),
            release("not-a-version", false, true),
        ];
        assert_eq!(
            newest_manifest(releases).as_deref(),
            Some("https://example.com/v1.0.0/latest.json")
        );
        let ahead = vec![release("v1.0.0", false, true), release("v1.1.0-beta.1", false, true)];
        assert_eq!(
            newest_manifest(ahead).as_deref(),
            Some("https://example.com/v1.1.0-beta.1/latest.json")
        );
    }
}

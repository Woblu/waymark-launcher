//! Keeps Waymark current. New versions download in the background and install
//! when the app closes, or sooner if the player asks. An update never cuts a
//! game off, because the installer has to stop this process to replace it, and
//! the watching of a game's log and crashes happens here.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use waymark_core::Error;

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
    let Some(update) = app.updater()?.check().await? else {
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

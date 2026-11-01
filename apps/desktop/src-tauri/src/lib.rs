//! The bridge between the UI and `waymark-core`. Commands stay thin, because
//! anything worth testing lives in the core.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::http::{Response, StatusCode, header};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::mpsc;
use waymark_core::auth::{Account, DeviceLogin, Session};
use waymark_core::bisect::Verdict;
use waymark_core::content::{
    ContentKind, ContentPage, ContentSource, ModUpdate, SearchCategory, SearchQuery, UpdatePick,
};
use waymark_core::crash::CrashReport;
use waymark_core::event::Event;
use waymark_core::export::ServerExport;
use waymark_core::external::ExternalInstance;
use waymark_core::hosting::HostStatus;
use waymark_core::instance::{Instance, NewInstance};
use waymark_core::library::{BisectView, InstancePatch, LibraryEntry};
use waymark_core::loaders::LoaderVersion;
use waymark_core::lock::{LoaderKind, LockedMod};
use waymark_core::logs::LogFile;
use waymark_core::pack_export::{ListFormat, PackExport, PackFormat};
use waymark_core::packs::{ManualFile, PackResult, PackUpdate, PackUpdateReport};
use waymark_core::paths::Paths;
use waymark_core::projects::{Fit, InstalledPage, ProjectPage, VersionFilter, VersionList};
use waymark_core::screenshots::Screenshot;
use waymark_core::share::{ShareCode, SharedResult};
use waymark_core::shortcut::LAUNCH_FLAG;
use waymark_core::skins::{SavedSkin, SkinLibrary, SkinProfile, SkinVariant, Wear};
use waymark_core::social::{FriendOverview, friends_available};
use waymark_core::trash::Trashed;
use waymark_core::waymarks::{Pending, WaymarkKind, WaymarkSummary};
use waymark_core::worlds::{Join, RecentWorld, SavedServer, ServerStatus, Worlds};
use waymark_core::{Core, Error, Settings, curseforge};

mod updates;

type Reply<T> = Result<T, Error>;

const LOG_FLUSH: Duration = Duration::from_millis(80);
const MAX_KEPT_LOG_LINES: usize = 8000;
const MAIN_WINDOW: &str = "main";
const FRIEND_BEAT: Duration = Duration::from_secs(60);

struct Launcher {
    core: Core,
    pending_login: Mutex<Option<DeviceLogin>>,
    /// Game output, kept here so it survives the window closing during play.
    logs: Mutex<HashMap<String, VecDeque<LogEntry>>>,
    /// A crash that happened while the window was closed.
    pending_crash: Mutex<Option<CrashNotice>>,
    /// The instance a desktop shortcut asked to start.
    pending_launch: Mutex<Option<String>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrashNotice {
    instance: String,
    report: CrashReport,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogBatch {
    instance: String,
    lines: Vec<LogEntry>,
}

#[derive(Clone, Serialize)]
struct LogEntry {
    text: String,
    error: bool,
}

fn open_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    if let Some(config) = app.config().app.windows.first() {
        // The page load handler shows it once there's something to see.
        let _ = WebviewWindowBuilder::from_config(app, config).and_then(|builder| builder.build());
    }
}

/// Destroying the window ends its web view, which is what gives the memory
/// back. Hiding it wouldn't.
fn close_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.destroy();
    }
}

fn keep_log_line(app: &AppHandle, instance: &str, entry: LogEntry) {
    let launcher = app.state::<Launcher>();
    let mut logs = launcher.logs.lock().expect("log buffer poisoned");
    let lines = logs.entry(instance.to_string()).or_default();
    if lines.len() == MAX_KEPT_LOG_LINES {
        lines.pop_front();
    }
    lines.push_back(entry);
}

fn on_game_started(app: &AppHandle, instance: &str) {
    let launcher = app.state::<Launcher>();
    launcher
        .logs
        .lock()
        .expect("log buffer poisoned")
        .remove(instance);
    if launcher
        .core
        .settings()
        .is_ok_and(|settings| settings.close_while_playing)
    {
        close_window(app);
    }
}

fn on_game_exited(app: &AppHandle, instance: &str, crash: Option<CrashReport>) {
    let launcher = app.state::<Launcher>();
    if app.get_webview_window(MAIN_WINDOW).is_some() {
        return;
    }
    if let Some(report) = crash {
        *launcher.pending_crash.lock().expect("crash slot poisoned") = Some(CrashNotice {
            instance: instance.to_string(),
            report,
        });
    }
    if !launcher.core.any_game_running() {
        open_window(app);
    }
}

/// Forwards core events to the UI. Log lines are batched, because a modpack
/// loading can print thousands a second and one IPC message for each would
/// freeze the interface.
fn forward_events(app: AppHandle, mut events: mpsc::UnboundedReceiver<Event>) {
    tauri::async_runtime::spawn(async move {
        let mut batches: Vec<LogBatch> = Vec::new();
        let mut flush = tokio::time::interval(LOG_FLUSH);
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(Event::GameLog { instance, line, error }) => {
                        let entry = LogEntry { text: line, error };
                        keep_log_line(&app, &instance, entry.clone());
                        match batches.iter_mut().find(|b| b.instance == instance) {
                            Some(batch) => batch.lines.push(entry),
                            None => batches.push(LogBatch { instance, lines: vec![entry] }),
                        }
                    }
                    Some(event) => {
                        for batch in batches.drain(..) {
                            let _ = app.emit("waymark://log", batch);
                        }
                        let _ = app.emit("waymark://event", event.clone());
                        match event {
                            Event::GameStarted { instance, .. } => on_game_started(&app, &instance),
                            Event::GameExited { instance, crash, .. } => on_game_exited(&app, &instance, crash),
                            _ => {}
                        }
                    }
                    None => break,
                },
                _ = flush.tick() => {
                    for batch in batches.drain(..) {
                        let _ = app.emit("waymark://log", batch);
                    }
                }
            }
        }
    });
}

/// Filesystem work runs off the async threads so the UI never waits on a disk.
async fn off_thread<T: Send + 'static>(
    launcher: &State<'_, Launcher>,
    work: impl FnOnce(&Core) -> Reply<T> + Send + 'static,
) -> Reply<T> {
    let core = launcher.core.clone();
    tauri::async_runtime::spawn_blocking(move || work(&core))
        .await
        .map_err(|error| Error::Other(error.to_string()))?
}

#[tauri::command]
async fn library(launcher: State<'_, Launcher>) -> Reply<Vec<LibraryEntry>> {
    off_thread(&launcher, |core| core.library()).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameVersion {
    id: String,
    kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameVersions {
    latest: String,
    versions: Vec<GameVersion>,
}

#[tauri::command]
async fn minecraft_versions(launcher: State<'_, Launcher>) -> Reply<GameVersions> {
    let manifest = launcher.core.version_manifest().await?;
    Ok(GameVersions {
        latest: manifest.latest.release,
        versions: manifest
            .versions
            .into_iter()
            .map(|v| GameVersion {
                id: v.id,
                kind: v.kind,
            })
            .collect(),
    })
}

#[tauri::command]
async fn loader_versions(
    launcher: State<'_, Launcher>,
    kind: LoaderKind,
    minecraft: String,
) -> Reply<Vec<LoaderVersion>> {
    launcher.core.loader_versions(kind, &minecraft).await
}

#[tauri::command]
async fn create_instance(launcher: State<'_, Launcher>, request: NewInstance) -> Reply<Instance> {
    launcher.core.create_instance(request).await
}

#[tauri::command]
async fn update_instance(
    launcher: State<'_, Launcher>,
    id: String,
    patch: InstancePatch,
) -> Reply<Instance> {
    off_thread(&launcher, move |core| core.update_instance(&id, patch)).await
}

#[tauri::command]
async fn delete_instance(launcher: State<'_, Launcher>, id: String) -> Reply<Trashed> {
    off_thread(&launcher, move |core| core.delete_instance(&id)).await
}

#[tauri::command]
async fn game_directory(launcher: State<'_, Launcher>, id: String) -> Reply<PathBuf> {
    Ok(launcher.core.paths.game(&id))
}

#[tauri::command]
async fn instance_content(
    launcher: State<'_, Launcher>,
    id: String,
    kind: ContentKind,
) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| core.instance_content(&id, kind)).await
}

#[tauri::command]
async fn installed_projects(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<String>> {
    off_thread(&launcher, move |core| core.installed_projects(&id)).await
}

#[tauri::command]
async fn set_content_enabled(
    launcher: State<'_, Launcher>,
    id: String,
    kind: ContentKind,
    hash: String,
    world: Option<String>,
    enabled: bool,
) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| {
        core.set_content_enabled(&id, kind, &hash, world.as_deref(), enabled)
    })
    .await
}

#[tauri::command]
async fn remove_content(
    launcher: State<'_, Launcher>,
    id: String,
    kind: ContentKind,
    hashes: Vec<String>,
    world: Option<String>,
) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| {
        core.remove_content(&id, kind, &hashes, world.as_deref())
    })
    .await
}

#[tauri::command]
async fn set_mods_enabled(
    launcher: State<'_, Launcher>,
    id: String,
    hashes: Vec<String>,
    enabled: bool,
) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| {
        core.set_mods_enabled(&id, &hashes, enabled)
    })
    .await
}

#[tauri::command]
async fn set_mods_group(
    launcher: State<'_, Launcher>,
    id: String,
    hashes: Vec<String>,
    group: Option<String>,
) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| {
        core.set_mods_group(&id, &hashes, group)
    })
    .await
}

#[tauri::command]
async fn toggle_mods_tag(
    launcher: State<'_, Launcher>,
    id: String,
    hashes: Vec<String>,
    tag: String,
) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| {
        core.toggle_mods_tag(&id, &hashes, &tag)
    })
    .await
}

/// Looks up what we know nothing about, and returns the new list if it changed.
#[tauri::command]
async fn identify_content(
    launcher: State<'_, Launcher>,
    id: String,
    kind: ContentKind,
) -> Reply<Option<Vec<LockedMod>>> {
    if !launcher.core.identify_mods(&id).await? {
        return Ok(None);
    }
    Ok(Some(launcher.core.lockfile(&id)?.shelf(kind).to_vec()))
}

#[tauri::command]
async fn check_pack_update(launcher: State<'_, Launcher>, id: String) -> Reply<Option<PackUpdate>> {
    launcher.core.check_pack_update(&id).await
}

#[tauri::command]
async fn update_pack(launcher: State<'_, Launcher>, id: String) -> Reply<PackUpdateReport> {
    launcher.core.update_pack(&id).await
}

#[tauri::command]
async fn set_folder_shared(
    launcher: State<'_, Launcher>,
    id: String,
    folder: String,
    shared: bool,
) -> Reply<Instance> {
    off_thread(&launcher, move |core| {
        core.set_folder_shared(&id, &folder, shared)
    })
    .await
}

#[tauri::command]
async fn copy_settings(
    launcher: State<'_, Launcher>,
    from: String,
    to: String,
) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| core.copy_settings(&from, &to)).await
}

#[tauri::command]
async fn export_server(
    launcher: State<'_, Launcher>,
    id: String,
    folder: PathBuf,
) -> Reply<ServerExport> {
    launcher.core.export_server(&id, &folder).await
}

#[tauri::command]
async fn recent_logs(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<LogEntry>> {
    let logs = launcher.logs.lock().expect("log buffer poisoned");
    Ok(logs
        .get(&id)
        .map(|lines| lines.iter().cloned().collect())
        .unwrap_or_default())
}

#[tauri::command]
async fn take_pending_crash(launcher: State<'_, Launcher>) -> Reply<Option<CrashNotice>> {
    Ok(launcher
        .pending_crash
        .lock()
        .expect("crash slot poisoned")
        .take())
}

#[tauri::command]
async fn trail(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<WaymarkSummary>> {
    off_thread(&launcher, move |core| core.trail(&id).list()).await
}

#[tauri::command]
async fn pending_changes(launcher: State<'_, Launcher>, id: String) -> Reply<Pending> {
    off_thread(&launcher, move |core| core.trail(&id).pending()).await
}

#[tauri::command]
async fn set_waymark(
    launcher: State<'_, Launcher>,
    id: String,
    label: String,
) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| {
        core.trail(&id).mark(&label, WaymarkKind::Manual)
    })
    .await
}

#[tauri::command]
async fn return_to_waymark(
    launcher: State<'_, Launcher>,
    id: String,
    waymark: String,
) -> Reply<WaymarkSummary> {
    off_thread(&launcher, move |core| core.trail(&id).restore(&waymark)).await
}

#[tauri::command]
async fn search_content(
    launcher: State<'_, Launcher>,
    source: ContentSource,
    query: SearchQuery,
) -> Reply<ContentPage> {
    launcher.core.search_content(source, &query).await
}

#[tauri::command]
async fn search_categories(
    launcher: State<'_, Launcher>,
    source: ContentSource,
    kind: ContentKind,
) -> Reply<Vec<SearchCategory>> {
    launcher.core.search_categories(source, kind).await
}

#[tauri::command]
fn curseforge_available() -> bool {
    curseforge::available()
}

/// Only opens folders, so a page can't use this to run a program.
#[tauri::command]
fn open_folder(app: AppHandle, path: PathBuf) -> Reply<()> {
    if !path.is_dir() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| Error::Other(error.to_string()))
}

#[tauri::command]
async fn project_page(
    launcher: State<'_, Launcher>,
    source: ContentSource,
    project: String,
) -> Reply<ProjectPage> {
    launcher.core.project_page(source, &project).await
}

#[tauri::command]
async fn project_versions(
    launcher: State<'_, Launcher>,
    source: ContentSource,
    project: String,
    filter: VersionFilter,
) -> Reply<VersionList> {
    launcher
        .core
        .project_versions(source, &project, &filter)
        .await
}

#[tauri::command]
async fn version_changelog(
    launcher: State<'_, Launcher>,
    source: ContentSource,
    project: String,
    version: String,
) -> Reply<String> {
    launcher
        .core
        .version_changelog(source, &project, &version)
        .await
}

#[tauri::command]
async fn project_fit(
    launcher: State<'_, Launcher>,
    id: String,
    source: ContentSource,
    project: String,
    kind: ContentKind,
) -> Reply<Option<Fit>> {
    launcher.core.project_fit(&id, source, &project, kind).await
}

#[tauri::command]
async fn installed_mod_page(
    launcher: State<'_, Launcher>,
    id: String,
    hash: String,
) -> Reply<InstalledPage> {
    launcher.core.installed_mod_page(&id, &hash).await
}

#[tauri::command]
async fn install_content(
    launcher: State<'_, Launcher>,
    id: String,
    source: ContentSource,
    project: String,
    kind: ContentKind,
    version: Option<String>,
    world: Option<String>,
) -> Reply<Option<WaymarkSummary>> {
    launcher
        .core
        .install_content(
            &id,
            source,
            &project,
            kind,
            version.as_deref(),
            world.as_deref(),
        )
        .await
}

#[tauri::command]
async fn install_pack(
    launcher: State<'_, Launcher>,
    source: ContentSource,
    project: String,
) -> Reply<PackResult> {
    launcher.core.install_pack(source, &project, None).await
}

#[tauri::command]
async fn install_pack_file(launcher: State<'_, Launcher>, path: PathBuf) -> Reply<PackResult> {
    launcher.core.install_pack_file(&path).await
}

#[tauri::command]
async fn check_updates(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<ModUpdate>> {
    launcher.core.check_updates(&id).await
}

#[tauri::command]
async fn apply_updates(
    launcher: State<'_, Launcher>,
    id: String,
    picks: Vec<UpdatePick>,
) -> Reply<Option<WaymarkSummary>> {
    launcher.core.apply_updates(&id, &picks).await
}

#[tauri::command]
async fn accounts(launcher: State<'_, Launcher>) -> Reply<Vec<Account>> {
    launcher.core.accounts()
}

#[tauri::command]
async fn begin_sign_in(launcher: State<'_, Launcher>) -> Reply<DeviceLogin> {
    let login = launcher.core.begin_sign_in().await?;
    *launcher.pending_login.lock().expect("login slot poisoned") = Some(login.clone());
    Ok(login)
}

#[tauri::command]
async fn finish_sign_in(launcher: State<'_, Launcher>) -> Reply<Account> {
    let login = launcher
        .pending_login
        .lock()
        .expect("login slot poisoned")
        .take();
    let login = login.ok_or_else(|| Error::Auth("Start signing in first.".into()))?;
    launcher.core.finish_sign_in(&login).await
}

#[tauri::command]
async fn set_active_account(launcher: State<'_, Launcher>, uuid: String) -> Reply<Vec<Account>> {
    launcher.core.set_active_account(&uuid)
}

#[tauri::command]
async fn remove_account(launcher: State<'_, Launcher>, uuid: String) -> Reply<Vec<Account>> {
    launcher.core.remove_account(&uuid)
}

/// Debug builds can launch with no account, which is how the launcher gets
/// tested before sign-in is available. Release builds have no such path.
#[cfg(debug_assertions)]
async fn launch_session(core: &Core) -> Reply<Session> {
    match core.session().await {
        Err(Error::SignInRequired) => Ok(Session::development()),
        other => other,
    }
}

#[cfg(not(debug_assertions))]
async fn launch_session(core: &Core) -> Reply<Session> {
    core.session().await
}

#[tauri::command]
async fn launch(launcher: State<'_, Launcher>, id: String, join: Option<Join>) -> Reply<()> {
    let session = launch_session(&launcher.core).await?;
    launcher.core.launch_with(&id, session, join).await
}

#[tauri::command]
async fn stop_game(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    launcher.core.stop_game(&id)
}

#[tauri::command]
async fn external_instances(launcher: State<'_, Launcher>) -> Reply<Vec<ExternalInstance>> {
    off_thread(&launcher, |core| Ok(core.find_external_instances())).await
}

#[tauri::command]
async fn import_external(
    launcher: State<'_, Launcher>,
    instance: ExternalInstance,
) -> Reply<Instance> {
    launcher.core.import_external(instance).await
}

#[tauri::command]
async fn start_bisect(launcher: State<'_, Launcher>, id: String) -> Reply<BisectView> {
    off_thread(&launcher, move |core| core.start_bisect(&id)).await
}

#[tauri::command]
async fn report_bisect(
    launcher: State<'_, Launcher>,
    id: String,
    verdict: Verdict,
) -> Reply<BisectView> {
    off_thread(&launcher, move |core| core.report_bisect(&id, verdict)).await
}

#[tauri::command]
async fn cancel_bisect(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    off_thread(&launcher, move |core| core.cancel_bisect(&id)).await
}

#[tauri::command]
async fn bisect_status(launcher: State<'_, Launcher>, id: String) -> Reply<Option<BisectView>> {
    off_thread(&launcher, move |core| core.bisect_status(&id)).await
}

#[tauri::command]
async fn settings(launcher: State<'_, Launcher>) -> Reply<Settings> {
    launcher.core.settings()
}

#[tauri::command]
async fn save_settings(launcher: State<'_, Launcher>, settings: Settings) -> Reply<()> {
    launcher.core.save_settings(&settings)
}

#[tauri::command]
async fn worlds(launcher: State<'_, Launcher>, id: String) -> Reply<Worlds> {
    off_thread(&launcher, move |core| core.worlds(&id)).await
}

#[tauri::command]
async fn recent_worlds(launcher: State<'_, Launcher>) -> Reply<Vec<RecentWorld>> {
    off_thread(&launcher, |core| core.recent_worlds()).await
}

#[tauri::command]
async fn delete_world(launcher: State<'_, Launcher>, id: String, folder: String) -> Reply<Trashed> {
    off_thread(&launcher, move |core| core.delete_world(&id, &folder)).await
}

#[tauri::command]
async fn back_up_world(
    launcher: State<'_, Launcher>,
    id: String,
    folder: String,
) -> Reply<PathBuf> {
    off_thread(&launcher, move |core| core.back_up_world(&id, &folder)).await
}

#[tauri::command]
async fn data_packs(
    launcher: State<'_, Launcher>,
    id: String,
    world: String,
) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| core.data_packs(&id, &world)).await
}

#[tauri::command]
async fn servers(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<SavedServer>> {
    off_thread(&launcher, move |core| core.servers(&id)).await
}

#[tauri::command]
async fn add_server(
    launcher: State<'_, Launcher>,
    id: String,
    name: String,
    address: String,
) -> Reply<Vec<SavedServer>> {
    off_thread(&launcher, move |core| core.add_server(&id, &name, &address)).await
}

#[tauri::command]
async fn edit_server(
    launcher: State<'_, Launcher>,
    id: String,
    index: usize,
    name: String,
    address: String,
) -> Reply<Vec<SavedServer>> {
    off_thread(&launcher, move |core| {
        core.edit_server(&id, index, &name, &address)
    })
    .await
}

#[tauri::command]
async fn remove_server(
    launcher: State<'_, Launcher>,
    id: String,
    index: usize,
) -> Reply<Vec<SavedServer>> {
    off_thread(&launcher, move |core| core.remove_server(&id, index)).await
}

#[tauri::command]
async fn server_status(launcher: State<'_, Launcher>, address: String) -> Reply<ServerStatus> {
    Ok(launcher.core.server_status(&address).await)
}

#[tauri::command]
async fn copy_instance(
    launcher: State<'_, Launcher>,
    id: String,
    name: String,
    worlds: bool,
) -> Reply<Instance> {
    off_thread(&launcher, move |core| {
        core.copy_instance(&id, &name, worlds)
    })
    .await
}

#[tauri::command]
async fn set_instance_group(
    launcher: State<'_, Launcher>,
    id: String,
    group: Option<String>,
) -> Reply<Instance> {
    off_thread(&launcher, move |core| {
        core.set_instance_group(&id, group.as_deref())
    })
    .await
}

#[tauri::command]
async fn set_instance_icon(
    launcher: State<'_, Launcher>,
    id: String,
    picture: Option<PathBuf>,
) -> Reply<Instance> {
    off_thread(&launcher, move |core| {
        core.set_instance_icon(&id, picture.as_deref())
    })
    .await
}

#[tauri::command]
async fn create_shortcut(launcher: State<'_, Launcher>, id: String) -> Reply<PathBuf> {
    let exe = std::env::current_exe().map_err(|error| Error::Other(error.to_string()))?;
    off_thread(&launcher, move |core| core.create_shortcut(&id, &exe)).await
}

#[tauri::command]
async fn export_pack(
    launcher: State<'_, Launcher>,
    id: String,
    target: PathBuf,
    format: PackFormat,
    name: String,
    version: String,
) -> Reply<PackExport> {
    launcher
        .core
        .export_pack(&id, &target, format, &name, &version)
        .await
}

#[tauri::command]
async fn mod_list(launcher: State<'_, Launcher>, id: String, format: ListFormat) -> Reply<String> {
    off_thread(&launcher, move |core| core.mod_list(&id, format)).await
}

#[tauri::command]
async fn add_local_files(
    launcher: State<'_, Launcher>,
    id: String,
    kind: ContentKind,
    paths: Vec<PathBuf>,
) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| {
        core.add_local_files(&id, kind, &paths)
    })
    .await
}

#[tauri::command]
async fn log_files(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<LogFile>> {
    off_thread(&launcher, move |core| core.log_files(&id)).await
}

#[tauri::command]
async fn read_log(launcher: State<'_, Launcher>, id: String, path: String) -> Reply<String> {
    off_thread(&launcher, move |core| core.read_log(&id, &path)).await
}

#[tauri::command]
async fn upload_log(launcher: State<'_, Launcher>, text: String) -> Reply<String> {
    launcher.core.upload_log(&text).await
}

#[tauri::command]
async fn manual_files(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<ManualFile>> {
    off_thread(&launcher, move |core| core.manual_files(&id)).await
}

/// Looks in the player's downloads for what a pack is still waiting on.
#[tauri::command]
async fn collect_manual_files(
    app: AppHandle,
    launcher: State<'_, Launcher>,
    id: String,
) -> Reply<Vec<ManualFile>> {
    let downloads = app
        .path()
        .download_dir()
        .map_err(|error| Error::Other(error.to_string()))?;
    off_thread(&launcher, move |core| {
        core.collect_manual_files(&id, &downloads)
    })
    .await
}

/// A shortcut's instance, once the window is ready to act on it.
#[tauri::command]
async fn take_pending_launch(launcher: State<'_, Launcher>) -> Reply<Option<String>> {
    Ok(launcher
        .pending_launch
        .lock()
        .expect("launch slot poisoned")
        .take())
}

#[tauri::command]
fn friends_open() -> bool {
    friends_available()
}

#[tauri::command]
async fn friend_account(launcher: State<'_, Launcher>) -> Reply<Option<String>> {
    launcher.core.friend_account().await
}

#[tauri::command]
async fn friend_sign_in(
    launcher: State<'_, Launcher>,
    username: String,
    password: String,
) -> Reply<String> {
    launcher.core.friend_sign_in(&username, &password).await
}

#[tauri::command]
async fn friend_sign_up(
    launcher: State<'_, Launcher>,
    username: String,
    password: String,
) -> Reply<String> {
    launcher.core.friend_sign_up(&username, &password).await
}

#[tauri::command]
async fn friend_sign_out(launcher: State<'_, Launcher>) -> Reply<()> {
    launcher.core.friend_sign_out().await
}

#[tauri::command]
async fn friend_delete_account(launcher: State<'_, Launcher>) -> Reply<()> {
    launcher.core.friend_delete_account().await
}

#[tauri::command]
async fn friend_overview(launcher: State<'_, Launcher>) -> Reply<FriendOverview> {
    launcher.core.friend_overview().await
}

#[tauri::command]
async fn friend_request(launcher: State<'_, Launcher>, username: String) -> Reply<()> {
    launcher.core.friend_request(&username).await
}

#[tauri::command]
async fn friend_respond(launcher: State<'_, Launcher>, request: String, accept: bool) -> Reply<()> {
    launcher.core.friend_respond(&request, accept).await
}

#[tauri::command]
async fn friend_remove(launcher: State<'_, Launcher>, other: String) -> Reply<()> {
    launcher.core.friend_remove(&other).await
}

#[tauri::command]
async fn friend_block(launcher: State<'_, Launcher>, other: String) -> Reply<()> {
    launcher.core.friend_block(&other).await
}

#[tauri::command]
async fn friend_unblock(launcher: State<'_, Launcher>, other: String) -> Reply<()> {
    launcher.core.friend_unblock(&other).await
}

#[tauri::command]
async fn friend_show_status(launcher: State<'_, Launcher>, shown: bool) -> Reply<()> {
    launcher.core.friend_show_status(shown).await
}

#[tauri::command]
async fn friend_send_instance(
    launcher: State<'_, Launcher>,
    friend: String,
    instance: String,
) -> Reply<Vec<String>> {
    launcher.core.friend_send_instance(&friend, &instance).await
}

#[tauri::command]
async fn friend_install_share(launcher: State<'_, Launcher>, share: String) -> Reply<SharedResult> {
    launcher.core.friend_install_share(&share).await
}

#[tauri::command]
async fn friend_drop_share(launcher: State<'_, Launcher>, share: String) -> Reply<()> {
    launcher.core.friend_drop_share(&share).await
}

/// Tells friends this player is around for as long as the app runs.
fn watch_friends(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut beat = tokio::time::interval(FRIEND_BEAT);
        loop {
            beat.tick().await;
            let _ = app.state::<Launcher>().core.friend_heartbeat().await;
        }
    });
}

#[tauri::command]
async fn share_code(launcher: State<'_, Launcher>, id: String) -> Reply<ShareCode> {
    off_thread(&launcher, move |core| core.share_code(&id)).await
}

#[tauri::command]
async fn install_share(launcher: State<'_, Launcher>, code: String) -> Reply<SharedResult> {
    launcher.core.install_share(&code).await
}

#[tauri::command]
async fn host_status(launcher: State<'_, Launcher>, id: String) -> Reply<HostStatus> {
    off_thread(&launcher, move |core| core.host_status(&id)).await
}

#[tauri::command]
async fn prepare_host(launcher: State<'_, Launcher>, id: String) -> Reply<HostStatus> {
    launcher.core.prepare_host(&id).await
}

/// Only the player's own tick in the Server tab reaches this.
#[tauri::command]
async fn accept_eula(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    off_thread(&launcher, move |core| core.accept_eula(&id)).await
}

#[tauri::command]
async fn start_host(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    launcher.core.start_host(&id)
}

#[tauri::command]
async fn stop_host(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    launcher.core.stop_host(&id)
}

#[tauri::command]
async fn host_command(launcher: State<'_, Launcher>, id: String, line: String) -> Reply<()> {
    launcher.core.host_command(&id, &line)
}

#[tauri::command]
async fn host_folder(launcher: State<'_, Launcher>, id: String) -> Reply<PathBuf> {
    Ok(launcher.core.host_path(&id))
}

#[tauri::command]
async fn screenshots(launcher: State<'_, Launcher>) -> Reply<Vec<Screenshot>> {
    off_thread(&launcher, |core| core.screenshots()).await
}

#[tauri::command]
async fn delete_screenshot(
    launcher: State<'_, Launcher>,
    id: String,
    file: String,
) -> Reply<Trashed> {
    off_thread(&launcher, move |core| core.delete_screenshot(&id, &file)).await
}

#[tauri::command]
async fn skin_library(launcher: State<'_, Launcher>) -> Reply<SkinLibrary> {
    off_thread(&launcher, |core| core.skin_library()).await
}

#[tauri::command]
async fn add_skin(launcher: State<'_, Launcher>, path: PathBuf) -> Reply<SavedSkin> {
    off_thread(&launcher, move |core| core.add_skin(&path)).await
}

#[tauri::command]
async fn update_skin(
    launcher: State<'_, Launcher>,
    id: String,
    name: String,
    variant: SkinVariant,
) -> Reply<Vec<SavedSkin>> {
    off_thread(&launcher, move |core| core.update_skin(&id, &name, variant)).await
}

#[tauri::command]
async fn remove_skin(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<SavedSkin>> {
    off_thread(&launcher, move |core| core.remove_skin(&id)).await
}

#[tauri::command]
async fn skin_profile(launcher: State<'_, Launcher>) -> Reply<SkinProfile> {
    launcher.core.skin_profile().await
}

#[tauri::command]
async fn wear_skin(launcher: State<'_, Launcher>, wear: Wear) -> Reply<SkinProfile> {
    launcher.core.wear_skin(wear).await
}

#[tauri::command]
async fn wear_cape(launcher: State<'_, Launcher>, cape: Option<String>) -> Reply<SkinProfile> {
    launcher.core.wear_cape(cape.as_deref()).await
}

#[tauri::command]
async fn trashed(launcher: State<'_, Launcher>) -> Reply<Vec<Trashed>> {
    off_thread(&launcher, |core| core.trashed()).await
}

#[tauri::command]
async fn restore_trashed(launcher: State<'_, Launcher>, id: String) -> Reply<Trashed> {
    off_thread(&launcher, move |core| core.restore_trashed(&id)).await
}

#[tauri::command]
async fn erase_trashed(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    off_thread(&launcher, move |core| core.erase_trashed(&id)).await
}

#[tauri::command]
async fn reclaim_space(launcher: State<'_, Launcher>) -> Reply<u64> {
    off_thread(&launcher, |core| core.reclaim_space()).await
}

/// The instance named by a desktop shortcut, if this start came from one.
fn launch_request(mut args: impl Iterator<Item = String>) -> Option<String> {
    args.find(|arg| arg == LAUNCH_FLAG)?;
    args.next()
}

/// A second start hands over to the window that's already open, and a
/// shortcut's instance starts there.
fn on_second_start(app: &AppHandle, args: Vec<String>) {
    open_window(app);
    if let Some(id) = launch_request(args.into_iter()) {
        *app.state::<Launcher>()
            .pending_launch
            .lock()
            .expect("launch slot poisoned") = Some(id);
        let _ = app.emit("waymark://launch", ());
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        let hex = bytes
            .get(position + 1..position + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok());
        match (
            bytes[position],
            hex.and_then(|pair| u8::from_str_radix(pair, 16).ok()),
        ) {
            (b'%', Some(value)) => {
                out.push(value);
                position += 3;
            }
            (byte, _) => {
                out.push(byte);
                position += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    // A separate data folder is a separate launcher, which is how tests run
    // beside a copy someone is using.
    if std::env::var_os("WAYMARK_HOME").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _| {
            on_second_start(app, args)
        }));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Pictures from the data folder, which the core checks before reading.
        .register_uri_scheme_protocol("waymark-icon", |context, request| {
            let path = percent_decode(request.uri().path().trim_start_matches('/'));
            let launcher = context.app_handle().state::<Launcher>();
            match launcher.core.local_image(&path) {
                // The 3D view can only read pictures that allow it, even from this app.
                Some(bytes) => Response::builder()
                    .header(header::CONTENT_TYPE, "image/png")
                    .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                    .body(bytes),
                None => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Vec::new()),
            }
            .expect("a response built from constants is valid")
        })
        // The window stays hidden until there's something to show, so the
        // first thing a player sees is the app rather than a blank frame.
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = webview.window().show();
            }
        })
        .setup(|app| {
            let (sender, receiver) = mpsc::unbounded_channel();
            let emitter = Arc::new(move |event: Event| {
                let _ = sender.send(event);
            });
            let root = match std::env::var_os("WAYMARK_HOME") {
                Some(path) => PathBuf::from(path),
                None => Paths::default_root()?,
            };
            let core = Core::new(root, emitter)?;
            let tidying = core.clone();
            tauri::async_runtime::spawn_blocking(move || tidying.purge_trash());
            app.manage(Launcher {
                core,
                pending_login: Mutex::new(None),
                logs: Mutex::new(HashMap::new()),
                pending_crash: Mutex::new(None),
                pending_launch: Mutex::new(launch_request(std::env::args())),
            });
            forward_events(app.handle().clone(), receiver);
            app.manage(updates::Updates::default());
            updates::watch(app.handle().clone());
            watch_friends(app.handle().clone());

            let show = MenuItem::with_id(app, "show", "Show Waymark", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Waymark", true, None::<&str>)?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Waymark")
                .menu(&Menu::with_items(app, &[&show, &quit])?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => open_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        open_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library,
            minecraft_versions,
            loader_versions,
            create_instance,
            update_instance,
            delete_instance,
            game_directory,
            instance_content,
            installed_projects,
            set_content_enabled,
            remove_content,
            set_mods_enabled,
            set_mods_group,
            toggle_mods_tag,
            identify_content,
            check_pack_update,
            update_pack,
            set_folder_shared,
            copy_settings,
            export_server,
            recent_logs,
            take_pending_crash,
            trail,
            pending_changes,
            set_waymark,
            return_to_waymark,
            search_content,
            search_categories,
            curseforge_available,
            open_folder,
            project_page,
            project_versions,
            version_changelog,
            project_fit,
            installed_mod_page,
            install_content,
            install_pack,
            install_pack_file,
            check_updates,
            updates::update_status,
            updates::check_for_update,
            updates::install_update,
            apply_updates,
            accounts,
            begin_sign_in,
            finish_sign_in,
            set_active_account,
            remove_account,
            launch,
            stop_game,
            external_instances,
            import_external,
            start_bisect,
            report_bisect,
            cancel_bisect,
            bisect_status,
            settings,
            save_settings,
            worlds,
            recent_worlds,
            delete_world,
            back_up_world,
            data_packs,
            servers,
            add_server,
            edit_server,
            remove_server,
            server_status,
            copy_instance,
            set_instance_group,
            set_instance_icon,
            create_shortcut,
            export_pack,
            mod_list,
            add_local_files,
            log_files,
            read_log,
            upload_log,
            manual_files,
            collect_manual_files,
            take_pending_launch,
            share_code,
            install_share,
            friends_open,
            friend_account,
            friend_sign_in,
            friend_sign_up,
            friend_sign_out,
            friend_delete_account,
            friend_overview,
            friend_request,
            friend_respond,
            friend_remove,
            friend_block,
            friend_unblock,
            friend_show_status,
            friend_send_instance,
            friend_install_share,
            friend_drop_share,
            host_status,
            prepare_host,
            accept_eula,
            start_host,
            stop_host,
            host_command,
            host_folder,
            screenshots,
            delete_screenshot,
            skin_library,
            add_skin,
            update_skin,
            remove_skin,
            skin_profile,
            wear_skin,
            wear_cape,
            trashed,
            restore_trashed,
            erase_trashed,
            reclaim_space,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Waymark");

    app.run(|app, event| {
        match event {
            // Closing the window mustn't end the app while a game is being watched.
            RunEvent::ExitRequested {
                api, code: None, ..
            } if updates::exit_would_interrupt(app) => api.prevent_exit(),
            RunEvent::Exit => updates::install_on_exit(app),
            _ => {}
        }
    });
}

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
use tokio::sync::mpsc;
use waymark_core::auth::{Account, DeviceLogin, Session};
use waymark_core::bisect::Verdict;
use waymark_core::content::{ContentKind, ContentPage, ContentSource, ModUpdate, SearchQuery};
use waymark_core::crash::CrashReport;
use waymark_core::event::Event;
use waymark_core::export::ServerExport;
use waymark_core::external::ExternalInstance;
use waymark_core::instance::{Instance, NewInstance};
use waymark_core::library::{BisectView, InstancePatch, LibraryEntry};
use waymark_core::loaders::LoaderVersion;
use waymark_core::lock::{LoaderKind, LockedMod, Lockfile};
use waymark_core::packs::{PackResult, PackUpdate, PackUpdateReport};
use waymark_core::paths::Paths;
use waymark_core::waymarks::{Pending, WaymarkKind, WaymarkSummary};
use waymark_core::{Core, Error, Settings, curseforge};

type Reply<T> = Result<T, Error>;

const LOG_FLUSH: Duration = Duration::from_millis(80);
const MAX_KEPT_LOG_LINES: usize = 8000;
const MAIN_WINDOW: &str = "main";

struct Launcher {
    core: Core,
    pending_login: Mutex<Option<DeviceLogin>>,
    /// Game output, kept here so it survives the window closing during play.
    logs: Mutex<HashMap<String, VecDeque<LogEntry>>>,
    /// A crash that happened while the window was closed.
    pending_crash: Mutex<Option<CrashNotice>>,
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
    launcher.logs.lock().expect("log buffer poisoned").remove(instance);
    if launcher.core.settings().is_ok_and(|settings| settings.close_while_playing) {
        close_window(app);
    }
}

fn on_game_exited(app: &AppHandle, instance: &str, crash: Option<CrashReport>) {
    let launcher = app.state::<Launcher>();
    if app.get_webview_window(MAIN_WINDOW).is_some() {
        return;
    }
    if let Some(report) = crash {
        *launcher.pending_crash.lock().expect("crash slot poisoned") = Some(CrashNotice { instance: instance.to_string(), report });
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
            .map(|v| GameVersion { id: v.id, kind: v.kind })
            .collect(),
    })
}

#[tauri::command]
async fn loader_versions(launcher: State<'_, Launcher>, kind: LoaderKind, minecraft: String) -> Reply<Vec<LoaderVersion>> {
    launcher.core.loader_versions(kind, &minecraft).await
}

#[tauri::command]
async fn create_instance(launcher: State<'_, Launcher>, request: NewInstance) -> Reply<Instance> {
    launcher.core.create_instance(request).await
}

#[tauri::command]
async fn update_instance(launcher: State<'_, Launcher>, id: String, patch: InstancePatch) -> Reply<Instance> {
    off_thread(&launcher, move |core| core.update_instance(&id, patch)).await
}

#[tauri::command]
async fn delete_instance(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    off_thread(&launcher, move |core| core.delete_instance(&id)).await
}

#[tauri::command]
async fn game_directory(launcher: State<'_, Launcher>, id: String) -> Reply<PathBuf> {
    Ok(launcher.core.paths.game(&id))
}

#[tauri::command]
async fn instance_mods(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| {
        let mut lock: Lockfile = core.lockfile(&id)?;
        if lock.reconcile(&core.paths.game(&id), &core.store)? {
            lock.save(&core.paths.instance(&id))?;
        }
        Ok(lock.mods)
    })
    .await
}

#[tauri::command]
async fn set_mod_enabled(launcher: State<'_, Launcher>, id: String, hash: String, enabled: bool) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| Ok(core.set_mod_enabled(&id, &hash, enabled)?.mods)).await
}

#[tauri::command]
async fn remove_mod(launcher: State<'_, Launcher>, id: String, hash: String) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| core.remove_mod(&id, &hash)).await
}

#[tauri::command]
async fn set_mods_enabled(launcher: State<'_, Launcher>, id: String, hashes: Vec<String>, enabled: bool) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| core.set_mods_enabled(&id, &hashes, enabled)).await
}

#[tauri::command]
async fn set_mods_group(launcher: State<'_, Launcher>, id: String, hashes: Vec<String>, group: Option<String>) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| core.set_mods_group(&id, &hashes, group)).await
}

#[tauri::command]
async fn toggle_mods_tag(launcher: State<'_, Launcher>, id: String, hashes: Vec<String>, tag: String) -> Reply<Vec<LockedMod>> {
    off_thread(&launcher, move |core| core.toggle_mods_tag(&id, &hashes, &tag)).await
}

#[tauri::command]
async fn remove_mods(launcher: State<'_, Launcher>, id: String, hashes: Vec<String>) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| core.remove_mods(&id, &hashes)).await
}

/// Looks up mods we know nothing about, and returns the new list if it changed.
#[tauri::command]
async fn identify_mods(launcher: State<'_, Launcher>, id: String) -> Reply<Option<Vec<LockedMod>>> {
    if !launcher.core.identify_mods(&id).await? {
        return Ok(None);
    }
    Ok(Some(launcher.core.lockfile(&id)?.mods))
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
async fn set_folder_shared(launcher: State<'_, Launcher>, id: String, folder: String, shared: bool) -> Reply<Instance> {
    off_thread(&launcher, move |core| core.set_folder_shared(&id, &folder, shared)).await
}

#[tauri::command]
async fn copy_settings(launcher: State<'_, Launcher>, from: String, to: String) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| core.copy_settings(&from, &to)).await
}

#[tauri::command]
async fn export_server(launcher: State<'_, Launcher>, id: String, folder: PathBuf) -> Reply<ServerExport> {
    launcher.core.export_server(&id, &folder).await
}

#[tauri::command]
async fn recent_logs(launcher: State<'_, Launcher>, id: String) -> Reply<Vec<LogEntry>> {
    let logs = launcher.logs.lock().expect("log buffer poisoned");
    Ok(logs.get(&id).map(|lines| lines.iter().cloned().collect()).unwrap_or_default())
}

#[tauri::command]
async fn take_pending_crash(launcher: State<'_, Launcher>) -> Reply<Option<CrashNotice>> {
    Ok(launcher.pending_crash.lock().expect("crash slot poisoned").take())
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
async fn set_waymark(launcher: State<'_, Launcher>, id: String, label: String) -> Reply<Option<WaymarkSummary>> {
    off_thread(&launcher, move |core| core.trail(&id).mark(&label, WaymarkKind::Manual)).await
}

#[tauri::command]
async fn return_to_waymark(launcher: State<'_, Launcher>, id: String, waymark: String) -> Reply<WaymarkSummary> {
    off_thread(&launcher, move |core| core.trail(&id).restore(&waymark)).await
}

#[tauri::command]
async fn search_content(launcher: State<'_, Launcher>, source: ContentSource, query: SearchQuery) -> Reply<ContentPage> {
    launcher.core.search_content(source, &query).await
}

#[tauri::command]
fn curseforge_available() -> bool {
    curseforge::available()
}

#[tauri::command]
async fn install_content(
    launcher: State<'_, Launcher>,
    id: String,
    source: ContentSource,
    project: String,
    kind: ContentKind,
) -> Reply<Option<WaymarkSummary>> {
    launcher.core.install_content(&id, source, &project, kind).await
}

#[tauri::command]
async fn install_pack(launcher: State<'_, Launcher>, source: ContentSource, project: String) -> Reply<PackResult> {
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
async fn apply_updates(launcher: State<'_, Launcher>, id: String, hashes: Vec<String>) -> Reply<Option<WaymarkSummary>> {
    launcher.core.apply_updates(&id, &hashes).await
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
    let login = launcher.pending_login.lock().expect("login slot poisoned").take();
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
async fn launch(launcher: State<'_, Launcher>, id: String) -> Reply<()> {
    let session = launch_session(&launcher.core).await?;
    launcher.core.launch_with(&id, session).await
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
async fn import_external(launcher: State<'_, Launcher>, instance: ExternalInstance) -> Reply<Instance> {
    launcher.core.import_external(instance).await
}

#[tauri::command]
async fn start_bisect(launcher: State<'_, Launcher>, id: String) -> Reply<BisectView> {
    off_thread(&launcher, move |core| core.start_bisect(&id)).await
}

#[tauri::command]
async fn report_bisect(launcher: State<'_, Launcher>, id: String, verdict: Verdict) -> Reply<BisectView> {
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
async fn reclaim_space(launcher: State<'_, Launcher>) -> Reply<u64> {
    off_thread(&launcher, |core| core.reclaim_space()).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Icons taken from inside jars, served by hash from the cache.
        .register_uri_scheme_protocol("waymark-icon", |context, request| {
            let hash = request.uri().path().trim_start_matches('/').to_string();
            let launcher = context.app_handle().state::<Launcher>();
            match launcher.core.cached_icon(&hash) {
                Some(bytes) => Response::builder().header(header::CONTENT_TYPE, "image/png").body(bytes),
                None => Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
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
            app.manage(Launcher {
                core,
                pending_login: Mutex::new(None),
                logs: Mutex::new(HashMap::new()),
                pending_crash: Mutex::new(None),
            });
            forward_events(app.handle().clone(), receiver);

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
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
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
            instance_mods,
            set_mod_enabled,
            remove_mod,
            set_mods_enabled,
            set_mods_group,
            toggle_mods_tag,
            remove_mods,
            identify_mods,
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
            curseforge_available,
            install_content,
            install_pack,
            install_pack_file,
            check_updates,
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
            reclaim_space,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting Waymark");

    app.run(|app, event| {
        // Closing the window mustn't end the app while a game is being watched.
        if let RunEvent::ExitRequested { api, code: None, .. } = event
            && app.state::<Launcher>().core.any_game_running()
        {
            api.prevent_exit();
        }
    });
}

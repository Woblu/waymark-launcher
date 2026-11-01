// Typed access to the Rust side. Outside Tauri (a plain browser during design
// work) calls go to sample data instead, so the UI can be built and
// screenshotted without a backend.

import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type LoaderKind = 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | 'legacyfabric' | 'babric' | 'ornithe';
export type ContentSource = 'modrinth' | 'curseforge' | 'ftb' | 'atlauncher';
export type ContentKind = 'mod' | 'modpack' | 'resourcepack' | 'shader' | 'datapack';
export type Verdict = 'crashes' | 'works';

export interface Loader { kind: LoaderKind; version?: string }

export interface LibraryEntry {
  id: string;
  name: string;
  minecraft: string;
  loader: Loader;
  versionId: string;
  createdAt: string;
  lastPlayed?: string;
  playtimeSecs: number;
  memoryMb?: number;
  jvmArgs?: string[];
  javaPath?: string;
  icon?: string;
  pack?: string;
  shared?: string[];
  group?: string;
  width?: number;
  height?: number;
  fullscreen?: boolean;
  env?: [string, string][];
  preLaunch?: string;
  wrapper?: string;
  postExit?: string;
  mods: number;
  waymarks: number;
  lastWaymark: string | null;
  running: boolean;
  bisecting: boolean;
  packProject?: string | null;
}

export type Origin = { kind: 'manual' } | { kind: 'pack'; name: string } | { kind: 'dependency'; of: string[] };

export interface LockedMod {
  name: string;
  file: string;
  hash: string;
  size: number;
  enabled: boolean;
  origin: Origin;
  side: 'both' | 'client' | 'server';
  version?: string;
  icon?: string;
  group?: string;
  tags?: string[];
  source: { kind: 'modrinth'; project: string } | { kind: 'curseforge'; project: number } | { kind: 'manual'; project?: undefined };
}

export type WaymarkKind = 'created' | 'manual' | 'install' | 'remove' | 'update' | 'launch' | 'import' | 'return' | 'automatic';

export interface Waymark {
  id: string;
  at: string;
  label: string;
  kind: WaymarkKind;
  added: string[];
  removed: string[];
  changed: string[];
  files: number;
  mods: number;
  bytes: number;
}

export interface Pending { added: string[]; removed: string[]; changed: string[] }

export interface ContentHit {
  source: ContentSource;
  id: string;
  slug: string;
  title: string;
  summary: string;
  author: string;
  downloads: number;
  icon: string | null;
  banner: string | null;
  kind: ContentKind;
  categories: string[];
}

export interface ContentPage { hits: ContentHit[]; total: number }

export type Sort = 'relevance' | 'downloads' | 'follows' | 'newest' | 'updated';

export interface SearchQuery {
  text: string;
  kind: ContentKind;
  minecraft?: string;
  loader?: LoaderKind;
  sort?: Sort;
  categories?: string[];
  offset: number;
  limit: number;
}

export interface SearchCategory { id: string; name: string; group: string }

export interface Link { label: string; url: string }
export interface Shot { url: string; full: string; title?: string | null; description?: string | null }
export interface Creator { name: string; role?: string | null; avatar?: string | null; url?: string | null }

export interface ProjectPage {
  source: ContentSource;
  id: string;
  slug: string;
  kind: ContentKind;
  title: string;
  summary: string;
  body: string;
  format: 'markdown' | 'html';
  icon?: string | null;
  downloads: number;
  followers: number;
  published?: string | null;
  updated?: string | null;
  categories: string[];
  loaders: string[];
  gameVersions: string[];
  client?: string | null;
  server?: string | null;
  license?: Link | null;
  links: Link[];
  gallery: Shot[];
  creators: Creator[];
  page: string;
}

export interface VersionRow {
  id: string;
  name: string;
  number: string;
  channel: string;
  published: string;
  downloads: number;
  loaders: string[];
  gameVersions: string[];
  file: string;
  size: number;
  available: boolean;
}

export interface VersionList { versions: VersionRow[]; total: number }
export interface VersionFilter { minecraft?: string; loader?: string; offset: number }
export interface Needed { source: ContentSource; id: string; title: string; icon?: string | null }
export interface Fit { version: string; requires: Needed[] }

export interface LocalMod {
  name: string;
  version?: string | null;
  description?: string | null;
  authors: string[];
  license?: string | null;
  links: Link[];
  file: string;
  size: number;
  icon?: string | null;
}

export type InstalledPage =
  | { kind: 'store'; source: ContentSource; project: string }
  | { kind: 'namesake'; source: ContentSource; project: string; file: string; version?: string }
  | ({ kind: 'local' } & LocalMod);

export interface ProjectPreview { title: string; icon?: string | null; summary?: string }

/** A page to open: a store project, or a mod file in an instance. */
export type ProjectTarget =
  | { source: ContentSource; id: string; kind?: ContentKind; instance?: string; preview?: ProjectPreview }
  | { hash: string; instance: string; preview?: ProjectPreview };

export type SkinVariant = 'classic' | 'slim';
export interface SavedSkin { id: string; name: string; variant: SkinVariant }
export interface DefaultSkin { name: string; variant: SkinVariant; url: string }
export interface SkinLibrary { saved: SavedSkin[]; defaults: DefaultSkin[] }
export type Wear = { kind: 'saved'; id: string } | { kind: 'default'; url: string; variant: SkinVariant };

export interface SkinProfile {
  name: string;
  skins: { url: string; variant: string; state: string }[];
  capes: { id: string; url: string; alias?: string | null; state: string }[];
}

export type Join = { kind: 'world'; folder: string } | { kind: 'server'; address: string };

export interface World {
  folder: string;
  name: string;
  lastPlayed?: number | null;
  mode: string;
  version?: string | null;
  hasIcon: boolean;
}

export interface Worlds { worlds: World[]; canJoin: boolean }
export interface RecentWorld extends World { instance: string; instanceName: string }
export interface SavedServer { index: number; name: string; address: string; icon?: string | null }

export interface ServerStatus {
  online: boolean;
  players?: number | null;
  capacity?: number | null;
  version?: string | null;
  motd: string;
  latencyMs?: number | null;
  icon?: string | null;
}

export interface Trashed { id: string; kind: 'instance' | 'world' | 'screenshot'; name: string; instance: string; folder: string; at: string }

export interface ModUpdate {
  kind: ContentKind;
  hash: string;
  name: string;
  source: ContentSource;
  project: string;
  current?: string;
  latest: string;
  held: boolean;
}

export interface UpdatePick { hash: string; version?: string }
export interface PackExport { linked: number; bundled: string[] }
export interface LogFile { path: string; size: number; modified: number }

export interface ShareCode { code: string; leftOut: string[] }
export interface HostStatus { ready: boolean; eulaAccepted: boolean; running: boolean; address?: string | null; port: number }

export interface ManualFile { name: string; page: string; file: string; size: number; sha1?: string | null; project: number; fileId: number }

export interface LaunchOptions {
  width?: number | null;
  height?: number | null;
  fullscreen?: boolean;
  env?: [string, string][];
  preLaunch?: string | null;
  wrapper?: string | null;
  postExit?: string | null;
}

export interface Account { uuid: string; name: string; ownsGame: boolean; verifiedAt: string; skinUrl?: string; active: boolean }

export interface DeviceLogin { userCode: string; verificationUri: string; expiresIn: number }

export interface ExternalInstance {
  launcher: 'prism' | 'multimc' | 'curseforge' | 'modrinth' | 'atlauncher' | 'gdlauncher';
  name: string;
  gameDir: string;
  minecraft: string;
  loader: Loader;
}

/** `system`, `dark`, `light`, or the id of a built-in or custom theme. */
export type Theme = string;

export interface CustomTheme {
  id: string;
  name: string;
  base: string;
  accent: string;
}

export interface Settings {
  memoryMb: number;
  releaseDelayHours: number;
  showSnapshots: boolean;
  closeWhilePlaying: boolean;
  theme: Theme;
  customThemes: CustomTheme[];
  language: string;
  autoUpdate: boolean;
}

export interface UpdateStatus { current: string; ready: string | null }

export interface Friend { id: string; username: string; online: boolean; lastSeen: string | null; playing: string | null }
export interface OtherPlayer { id: string; username: string }
export interface SharedByFriend { id: string; name: string; from: string; at: string }
export interface FriendOverview {
  username: string;
  showStatus: boolean;
  friends: Friend[];
  incoming: OtherPlayer[];
  outgoing: OtherPlayer[];
  blocked: OtherPlayer[];
  shares: SharedByFriend[];
}

export interface Screenshot { instance: string; instanceName: string; file: string; size: number; modified: number }

export interface PackUpdate { name: string; current: string; latest: string }

export interface PackUpdateReport {
  version: string;
  added: number;
  updated: number;
  removed: number;
  kept: string[];
  stayedRemoved: string[];
  minecraftChanged: boolean;
  manual: ManualFile[];
}

export interface ServerExport { folder: string; mods: number; skipped: string[] }

export type BisectView =
  | { status: 'testing'; round: number; estimate: number; enabled: number; total: number }
  | { status: 'found'; culprits: string[] }
  | { status: 'notAMod' };

export type Fix =
  | { kind: 'useJava'; major: number }
  | { kind: 'raiseMemory' }
  | { kind: 'updateDrivers' }
  | { kind: 'installMod'; id: string }
  | { kind: 'removeDuplicate'; id: string }
  | { kind: 'runBisect' };

export interface Finding { title: string; detail: string; suspects: string[]; fix: Fix }
export interface CrashReport { findings: Finding[]; reportFile: string | null }

export type CoreEvent =
  | { type: 'taskStarted'; id: string; title: string }
  | { type: 'taskProgress'; id: string; done: number; total: number; detail: string }
  | { type: 'taskFinished'; id: string }
  | { type: 'taskFailed'; id: string; message: string }
  | { type: 'gameStarted'; instance: string; pid: number }
  | { type: 'gameExited'; instance: string; code: number | null; playedSecs: number; crash: CrashReport | null }
  | { type: 'serverStopped'; instance: string };

export interface LogBatch { instance: string; lines: { text: string; error: boolean }[] }

export interface ApiError { code: string; message: string }

export const inTauri = '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return invoke<T>(command, args);
  const { mockCall } = await import('./mock');
  return mockCall(command, args) as T;
}

export const api = {
  library: () => call<LibraryEntry[]>('library'),
  minecraftVersions: () => call<{ latest: string; versions: { id: string; kind: string }[] }>('minecraft_versions'),
  loaderVersions: (kind: LoaderKind, minecraft: string) =>
    call<{ version: string; stable: boolean }[]>('loader_versions', { kind, minecraft }),
  createInstance: (request: { name: string; minecraft: string; loader: LoaderKind }) => call<LibraryEntry>('create_instance', { request }),
  updateInstance: (id: string, patch: Record<string, unknown>) => call<LibraryEntry>('update_instance', { id, patch }),
  deleteInstance: (id: string) => call<Trashed>('delete_instance', { id }),
  gameDirectory: (id: string) => call<string>('game_directory', { id }),
  content: (id: string, kind: ContentKind) => call<LockedMod[]>('instance_content', { id, kind }),
  installedProjects: (id: string) => call<string[]>('installed_projects', { id }),
  setContentEnabled: (id: string, kind: ContentKind, hash: string, enabled: boolean, world?: string) =>
    call<LockedMod[]>('set_content_enabled', { id, kind, hash, enabled, world }),
  removeContent: (id: string, kind: ContentKind, hashes: string[], world?: string) => call<Waymark | null>('remove_content', { id, kind, hashes, world }),
  setModsEnabled: (id: string, hashes: string[], enabled: boolean) => call<LockedMod[]>('set_mods_enabled', { id, hashes, enabled }),
  setModsGroup: (id: string, hashes: string[], group: string | null) => call<LockedMod[]>('set_mods_group', { id, hashes, group }),
  toggleModsTag: (id: string, hashes: string[], tag: string) => call<LockedMod[]>('toggle_mods_tag', { id, hashes, tag }),
  identifyContent: (id: string, kind: ContentKind) => call<LockedMod[] | null>('identify_content', { id, kind }),
  checkPackUpdate: (id: string) => call<PackUpdate | null>('check_pack_update', { id }),
  updatePack: (id: string) => call<PackUpdateReport>('update_pack', { id }),
  setFolderShared: (id: string, folder: string, shared: boolean) => call<LibraryEntry>('set_folder_shared', { id, folder, shared }),
  copySettings: (from: string, to: string) => call<Waymark | null>('copy_settings', { from, to }),
  exportServer: (id: string, folder: string) => call<ServerExport>('export_server', { id, folder }),
  recentLogs: (id: string) => call<{ text: string; error: boolean }[]>('recent_logs', { id }),
  takePendingCrash: () => call<{ instance: string; report: CrashReport } | null>('take_pending_crash'),
  trail: (id: string) => call<Waymark[]>('trail', { id }),
  pending: (id: string) => call<Pending>('pending_changes', { id }),
  setWaymark: (id: string, label: string) => call<Waymark | null>('set_waymark', { id, label }),
  returnTo: (id: string, waymark: string) => call<Waymark>('return_to_waymark', { id, waymark }),
  search: (source: ContentSource, query: SearchQuery) => call<ContentPage>('search_content', { source, query }),
  searchCategories: (source: ContentSource, kind: ContentKind) => call<SearchCategory[]>('search_categories', { source, kind }),
  curseforgeAvailable: () => call<boolean>('curseforge_available'),
  projectPage: (source: ContentSource, project: string) => call<ProjectPage>('project_page', { source, project }),
  projectVersions: (source: ContentSource, project: string, filter: VersionFilter) =>
    call<VersionList>('project_versions', { source, project, filter }),
  versionChangelog: (source: ContentSource, project: string, version: string) => call<string>('version_changelog', { source, project, version }),
  projectFit: (id: string, source: ContentSource, project: string, kind: ContentKind) => call<Fit | null>('project_fit', { id, source, project, kind }),
  installedModPage: (id: string, hash: string) => call<InstalledPage>('installed_mod_page', { id, hash }),
  installContent: (id: string, source: ContentSource, project: string, kind: ContentKind, version?: string, world?: string) =>
    call<Waymark | null>('install_content', { id, source, project, kind, version, world }),
  installPack: (source: ContentSource, project: string) => call<{ instance: LibraryEntry; manual: ManualFile[] }>('install_pack', { source, project }),
  installPackFile: (path: string) => call<{ instance: LibraryEntry; manual: ManualFile[] }>('install_pack_file', { path }),
  checkUpdates: (id: string) => call<ModUpdate[]>('check_updates', { id }),
  applyUpdates: (id: string, picks: UpdatePick[]) => call<Waymark | null>('apply_updates', { id, picks }),
  accounts: () => call<Account[]>('accounts'),
  beginSignIn: () => call<DeviceLogin>('begin_sign_in'),
  finishSignIn: () => call<Account>('finish_sign_in'),
  setActiveAccount: (uuid: string) => call<Account[]>('set_active_account', { uuid }),
  removeAccount: (uuid: string) => call<Account[]>('remove_account', { uuid }),
  launch: (id: string, join?: Join) => call<void>('launch', { id, join }),
  stopGame: (id: string) => call<void>('stop_game', { id }),
  externalInstances: () => call<ExternalInstance[]>('external_instances'),
  importExternal: (instance: ExternalInstance) => call<LibraryEntry>('import_external', { instance }),
  startBisect: (id: string) => call<BisectView>('start_bisect', { id }),
  reportBisect: (id: string, verdict: Verdict) => call<BisectView>('report_bisect', { id, verdict }),
  cancelBisect: (id: string) => call<void>('cancel_bisect', { id }),
  bisectStatus: (id: string) => call<BisectView | null>('bisect_status', { id }),
  settings: () => call<Settings>('settings'),
  saveSettings: (settings: Settings) => call<void>('save_settings', { settings }),
  worlds: (id: string) => call<Worlds>('worlds', { id }),
  recentWorlds: () => call<RecentWorld[]>('recent_worlds'),
  deleteWorld: (id: string, folder: string) => call<Trashed>('delete_world', { id, folder }),
  backUpWorld: (id: string, folder: string) => call<string>('back_up_world', { id, folder }),
  dataPacks: (id: string, world: string) => call<LockedMod[]>('data_packs', { id, world }),
  servers: (id: string) => call<SavedServer[]>('servers', { id }),
  addServer: (id: string, name: string, address: string) => call<SavedServer[]>('add_server', { id, name, address }),
  editServer: (id: string, index: number, name: string, address: string) => call<SavedServer[]>('edit_server', { id, index, name, address }),
  removeServer: (id: string, index: number) => call<SavedServer[]>('remove_server', { id, index }),
  serverStatus: (address: string) => call<ServerStatus>('server_status', { address }),
  copyInstance: (id: string, name: string, worlds: boolean) => call<LibraryEntry>('copy_instance', { id, name, worlds }),
  setInstanceGroup: (id: string, group: string | null) => call<LibraryEntry>('set_instance_group', { id, group }),
  setInstanceIcon: (id: string, picture: string | null) => call<LibraryEntry>('set_instance_icon', { id, picture }),
  createShortcut: (id: string) => call<string>('create_shortcut', { id }),
  exportPack: (id: string, target: string, format: 'modrinth' | 'curseforge', name: string, version: string) =>
    call<PackExport>('export_pack', { id, target, format, name, version }),
  modList: (id: string, format: 'text' | 'markdown' | 'csv') => call<string>('mod_list', { id, format }),
  addLocalFiles: (id: string, kind: ContentKind, paths: string[]) => call<Waymark | null>('add_local_files', { id, kind, paths }),
  logFiles: (id: string) => call<LogFile[]>('log_files', { id }),
  readLog: (id: string, path: string) => call<string>('read_log', { id, path }),
  uploadLog: (text: string) => call<string>('upload_log', { text }),
  manualFiles: (id: string) => call<ManualFile[]>('manual_files', { id }),
  collectManualFiles: (id: string) => call<ManualFile[]>('collect_manual_files', { id }),
  takePendingLaunch: () => call<string | null>('take_pending_launch'),
  shareCode: (id: string) => call<ShareCode>('share_code', { id }),
  installShare: (code: string) => call<{ instance: LibraryEntry; failed: string[] }>('install_share', { code }),
  hostStatus: (id: string) => call<HostStatus>('host_status', { id }),
  prepareHost: (id: string) => call<HostStatus>('prepare_host', { id }),
  acceptEula: (id: string) => call<void>('accept_eula', { id }),
  startHost: (id: string) => call<void>('start_host', { id }),
  stopHost: (id: string) => call<void>('stop_host', { id }),
  hostCommand: (id: string, line: string) => call<void>('host_command', { id, line }),
  hostFolder: (id: string) => call<string>('host_folder', { id }),
  screenshots: () => call<Screenshot[]>('screenshots'),
  deleteScreenshot: (id: string, file: string) => call<Trashed>('delete_screenshot', { id, file }),
  skinLibrary: () => call<SkinLibrary>('skin_library'),
  addSkin: (path: string) => call<SavedSkin>('add_skin', { path }),
  updateSkin: (id: string, name: string, variant: SkinVariant) => call<SavedSkin[]>('update_skin', { id, name, variant }),
  removeSkin: (id: string) => call<SavedSkin[]>('remove_skin', { id }),
  skinProfile: () => call<SkinProfile>('skin_profile'),
  wearSkin: (wear: Wear) => call<SkinProfile>('wear_skin', { wear }),
  wearCape: (cape: string | null) => call<SkinProfile>('wear_cape', { cape }),
  trashed: () => call<Trashed[]>('trashed'),
  restoreTrashed: (id: string) => call<Trashed>('restore_trashed', { id }),
  eraseTrashed: (id: string) => call<void>('erase_trashed', { id }),
  reclaimSpace: () => call<number>('reclaim_space'),
  friendsOpen: () => call<boolean>('friends_open'),
  friendAccount: () => call<string | null>('friend_account'),
  friendSignIn: (username: string, password: string) => call<string>('friend_sign_in', { username, password }),
  friendSignUp: (username: string, password: string) => call<string>('friend_sign_up', { username, password }),
  friendSignOut: () => call<void>('friend_sign_out'),
  friendDeleteAccount: () => call<void>('friend_delete_account'),
  friendOverview: () => call<FriendOverview>('friend_overview'),
  friendRequest: (username: string) => call<void>('friend_request', { username }),
  friendRespond: (request: string, accept: boolean) => call<void>('friend_respond', { request, accept }),
  friendRemove: (other: string) => call<void>('friend_remove', { other }),
  friendBlock: (other: string) => call<void>('friend_block', { other }),
  friendUnblock: (other: string) => call<void>('friend_unblock', { other }),
  friendShowStatus: (shown: boolean) => call<void>('friend_show_status', { shown }),
  friendSendInstance: (friend: string, instance: string) => call<string[]>('friend_send_instance', { friend, instance }),
  friendInstallShare: (share: string) => call<{ instance: LibraryEntry; failed: string[] }>('friend_install_share', { share }),
  friendDropShare: (share: string) => call<void>('friend_drop_share', { share }),
  updateStatus: () => call<UpdateStatus>('update_status'),
  checkForUpdate: () => call<string | null>('check_for_update'),
  installUpdate: () => call<void>('install_update'),
};

export function onCoreEvent(handler: (event: CoreEvent) => void) {
  if (inTauri) listen<CoreEvent>('waymark://event', (e) => handler(e.payload));
}

export function onGameLog(handler: (batch: LogBatch) => void) {
  if (inTauri) listen<LogBatch>('waymark://log', (e) => handler(e.payload));
}

/** A new version finished downloading. */
export function onUpdateReady(handler: (version: string) => void) {
  if (inTauri) listen<string>('waymark://update', (e) => handler(e.payload));
}

/** A desktop shortcut was used while the launcher was already open. */
export function onLaunchRequest(handler: () => void) {
  if (inTauri) listen('waymark://launch', handler);
}

export type DropEvent = { kind: 'over' } | { kind: 'leave' } | { kind: 'drop'; paths: string[] };

export async function onFileDrop(handler: (event: DropEvent) => void) {
  if (!inTauri) return;
  const { getCurrentWebview } = await import('@tauri-apps/api/webview');
  await getCurrentWebview().onDragDropEvent(({ payload }) => {
    if (payload.type === 'drop') handler({ kind: 'drop', paths: payload.paths });
    else if (payload.type === 'leave') handler({ kind: 'leave' });
    else handler({ kind: 'over' });
  });
}

export async function openExternal(url: string) {
  if (inTauri) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  } else {
    window.open(url, '_blank');
  }
}

export async function openFolder(path: string) {
  if (inTauri) await invoke('open_folder', { path });
}

/** Icons from inside jars are served by the app, and the rest are ordinary URLs. */
export function iconUrl(icon: string | undefined | null): string | null {
  if (!icon) return null;
  if (!icon.startsWith('local:')) return icon;
  return inTauri ? convertFileSrc(icon.slice('local:'.length), 'waymark-icon') : null;
}

/** A picture from the data folder, like `world/<instance>/<folder>`. */
export function localImage(path: string): string | undefined {
  return inTauri ? convertFileSrc(path, 'waymark-icon') : undefined;
}

/** An instance's picture, or undefined to use its generated art. */
export function instanceIcon(entry: LibraryEntry, tick = 0): string | undefined {
  if (entry.icon !== 'custom') return entry.icon;
  const url = localImage(`instance/${entry.id}`);
  return url && `${url}?v=${tick}`;
}

export async function pickSavePath(defaultPath: string, name: string, extension: string): Promise<string | null> {
  if (!inTauri) return null;
  const { save } = await import('@tauri-apps/plugin-dialog');
  return save({ defaultPath, filters: [{ name, extensions: [extension] }] });
}

export async function pickImage(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg'] }] });
  return typeof picked === 'string' ? picked : null;
}

export async function pickFolder(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ directory: true });
  return typeof picked === 'string' ? picked : null;
}

export async function pickSkinFile(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ filters: [{ name: 'Skin', extensions: ['png'] }] });
  return typeof picked === 'string' ? picked : null;
}

export async function pickPackFile(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ filters: [{ name: 'Modpack', extensions: ['mrpack', 'zip'] }] });
  return typeof picked === 'string' ? picked : null;
}

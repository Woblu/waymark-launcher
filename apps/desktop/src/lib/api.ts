// Typed access to the Rust side. Outside Tauri (a plain browser during design
// work) calls go to sample data instead, so the UI can be built and
// screenshotted without a backend.

import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type LoaderKind = 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge';
export type ContentSource = 'modrinth' | 'curseforge';
export type ContentKind = 'mod' | 'modpack' | 'resourcepack' | 'shader';
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
  mods: number;
  waymarks: number;
  lastWaymark: string | null;
  running: boolean;
  bisecting: boolean;
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
  source: { kind: 'modrinth' | 'curseforge' | 'manual' };
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

export interface SearchQuery {
  text: string;
  kind: ContentKind;
  minecraft?: string;
  loader?: LoaderKind;
  offset: number;
  limit: number;
}

export interface ModUpdate { hash: string; name: string; current?: string; latest: string; held: boolean }

export interface Account { uuid: string; name: string; ownsGame: boolean; verifiedAt: string; skinUrl?: string; active: boolean }

export interface DeviceLogin { userCode: string; verificationUri: string; expiresIn: number }

export interface ExternalInstance {
  launcher: 'prism' | 'multimc' | 'curseforge' | 'modrinth';
  name: string;
  gameDir: string;
  minecraft: string;
  loader: Loader;
}

export interface Settings { memoryMb: number; releaseDelayHours: number; showSnapshots: boolean; closeWhilePlaying: boolean }

export interface PackUpdate { name: string; current: string; latest: string }

export interface PackUpdateReport {
  version: string;
  added: number;
  updated: number;
  removed: number;
  kept: string[];
  stayedRemoved: string[];
  minecraftChanged: boolean;
  manual: { name: string; page: string }[];
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
  | { type: 'gameExited'; instance: string; code: number | null; playedSecs: number; crash: CrashReport | null };

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
  deleteInstance: (id: string) => call<void>('delete_instance', { id }),
  gameDirectory: (id: string) => call<string>('game_directory', { id }),
  mods: (id: string) => call<LockedMod[]>('instance_mods', { id }),
  setModEnabled: (id: string, hash: string, enabled: boolean) => call<LockedMod[]>('set_mod_enabled', { id, hash, enabled }),
  removeMod: (id: string, hash: string) => call<Waymark | null>('remove_mod', { id, hash }),
  setModsEnabled: (id: string, hashes: string[], enabled: boolean) => call<LockedMod[]>('set_mods_enabled', { id, hashes, enabled }),
  setModsGroup: (id: string, hashes: string[], group: string | null) => call<LockedMod[]>('set_mods_group', { id, hashes, group }),
  toggleModsTag: (id: string, hashes: string[], tag: string) => call<LockedMod[]>('toggle_mods_tag', { id, hashes, tag }),
  removeMods: (id: string, hashes: string[]) => call<Waymark | null>('remove_mods', { id, hashes }),
  identifyMods: (id: string) => call<LockedMod[] | null>('identify_mods', { id }),
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
  curseforgeAvailable: () => call<boolean>('curseforge_available'),
  installContent: (id: string, source: ContentSource, project: string, kind: ContentKind) =>
    call<Waymark | null>('install_content', { id, source, project, kind }),
  installPack: (source: ContentSource, project: string) => call<{ instance: LibraryEntry; manual: { name: string; page: string }[] }>('install_pack', { source, project }),
  installPackFile: (path: string) => call<{ instance: LibraryEntry; manual: { name: string; page: string }[] }>('install_pack_file', { path }),
  checkUpdates: (id: string) => call<ModUpdate[]>('check_updates', { id }),
  applyUpdates: (id: string, hashes: string[]) => call<Waymark | null>('apply_updates', { id, hashes }),
  accounts: () => call<Account[]>('accounts'),
  beginSignIn: () => call<DeviceLogin>('begin_sign_in'),
  finishSignIn: () => call<Account>('finish_sign_in'),
  setActiveAccount: (uuid: string) => call<Account[]>('set_active_account', { uuid }),
  removeAccount: (uuid: string) => call<Account[]>('remove_account', { uuid }),
  launch: (id: string) => call<void>('launch', { id }),
  stopGame: (id: string) => call<void>('stop_game', { id }),
  externalInstances: () => call<ExternalInstance[]>('external_instances'),
  importExternal: (instance: ExternalInstance) => call<LibraryEntry>('import_external', { instance }),
  startBisect: (id: string) => call<BisectView>('start_bisect', { id }),
  reportBisect: (id: string, verdict: Verdict) => call<BisectView>('report_bisect', { id, verdict }),
  cancelBisect: (id: string) => call<void>('cancel_bisect', { id }),
  bisectStatus: (id: string) => call<BisectView | null>('bisect_status', { id }),
  settings: () => call<Settings>('settings'),
  saveSettings: (settings: Settings) => call<void>('save_settings', { settings }),
  reclaimSpace: () => call<number>('reclaim_space'),
};

export function onCoreEvent(handler: (event: CoreEvent) => void) {
  if (inTauri) listen<CoreEvent>('waymark://event', (e) => handler(e.payload));
}

export function onGameLog(handler: (batch: LogBatch) => void) {
  if (inTauri) listen<LogBatch>('waymark://log', (e) => handler(e.payload));
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
  if (inTauri) {
    const { openPath } = await import('@tauri-apps/plugin-opener');
    await openPath(path);
  }
}

/** Icons from inside jars are served by the app, and the rest are ordinary URLs. */
export function iconUrl(icon: string | undefined | null): string | null {
  if (!icon) return null;
  if (!icon.startsWith('local:')) return icon;
  return inTauri ? convertFileSrc(icon.slice('local:'.length), 'waymark-icon') : null;
}

export async function pickFolder(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ directory: true });
  return typeof picked === 'string' ? picked : null;
}

export async function pickPackFile(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import('@tauri-apps/plugin-dialog');
  const picked = await open({ filters: [{ name: 'Modpack', extensions: ['mrpack', 'zip'] }] });
  return typeof picked === 'string' ? picked : null;
}

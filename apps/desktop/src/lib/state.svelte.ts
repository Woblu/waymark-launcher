import { api, iconUrl, inTauri, onCoreEvent, onFileDrop, onGameLog, onLaunchRequest, onUpdateReady } from './api';
import type { Account, ApiError, ContentKind, ContentSource, CoreEvent, CrashReport, FriendOverview, Join, LibraryEntry, LoaderKind, LockedMod, ProjectTarget, Starter } from './api';

const FRIENDS_EVERY = 60000;

/** The list kept for every version, next to each version's own. */
export const EVERY_VERSION = '*';

/** Vanilla has no mods and nothing to run shaders with. Matches `fits` in the core. */
export function starterFits(kind: ContentKind, loader: LoaderKind): boolean {
  if (kind === 'resourcepack') return true;
  return (kind === 'mod' || kind === 'shader') && loader !== 'vanilla';
}

/** What a new instance on `minecraft` gets: its own list, then the shared one, each project once. */
export function startersFor(lists: Record<string, Starter[]> | undefined, minecraft: string): Starter[] {
  const chosen: Starter[] = [];
  for (const starter of [...(lists?.[minecraft] ?? []), ...(lists?.[EVERY_VERSION] ?? [])]) {
    if (!chosen.some((other) => other.source === starter.source && other.project === starter.project)) chosen.push(starter);
  }
  return chosen;
}

export type View = 'library' | 'discover' | 'servers' | 'skins' | 'screenshots' | 'friends' | 'settings';
export type InstanceTab = 'mods' | 'resourcepacks' | 'shaders' | 'worlds' | 'trail' | 'logs' | 'server' | 'settings';

export interface TaskState {
  id: string;
  title: string;
  done: number;
  total: number;
  detail: string;
}

export interface Toast {
  id: number;
  tone: 'info' | 'good' | 'bad';
  title: string;
  detail?: string;
  action?: ToastAction;
}

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface MenuItem {
  label: string;
  run: () => void;
  danger?: boolean;
}

export interface InstanceDialog {
  kind: 'copy' | 'group' | 'export' | 'share';
  id: string;
}

export interface LogLine {
  text: string;
  error: boolean;
}

const MAX_LOG_LINES = 8000;
const TOAST_MS = 6000;
/** Long enough to read the message and change your mind. */
const UNDO_TOAST_MS = 12000;

class AppState {
  view = $state<View>('library');
  selected = $state<string | null>(null);
  tab = $state<InstanceTab>('mods');
  discoverTarget = $state<string | undefined>(undefined);
  discoverKind = $state<ContentKind | undefined>(undefined);

  /** Open project pages, newest last, so Back retraces links between projects. */
  pages = $state<ProjectTarget[]>([]);
  /** What each instance has, because pages and search share the answer. */
  installed = $state(new Map<string, Set<string>>());

  library = $state<LibraryEntry[]>([]);
  accounts = $state<Account[]>([]);
  tasks = $state<TaskState[]>([]);
  toasts = $state<Toast[]>([]);
  crash = $state<{ instance: string; report: CrashReport } | null>(null);
  dialog = $state<'none' | 'new' | 'signin'>('none');
  instanceDialog = $state<InstanceDialog | null>(null);
  menu = $state<{ x: number; y: number; up?: boolean; items: MenuItem[] } | null>(null);
  dragging = $state(false);
  /** Bumped when an instance's picture changes, so the old one isn't kept. */
  iconTick = $state(0);
  /** Skins dropped on the window, for the Skins page to pick up. */
  droppedSkins = $state<string[]>([]);
  loaded = $state(false);
  /** A new version that has downloaded and is waiting to install. */
  update = $state<string | null>(null);
  version = $state('');
  friendsAvailable = $state(false);
  friendAccount = $state<string | null>(null);
  friends = $state<FriendOverview | null>(null);

  /** Log text stays out of reactive state; the tick says when it changed. */
  logs = new Map<string, LogLine[]>();
  logTick = $state(0);

  /** Bumped whenever an instance's contents change, so open tabs reload. */
  contentTick = $state(0);

  private toastId = 0;

  get activeAccount(): Account | undefined {
    return this.accounts.find((a) => a.active);
  }

  /** The instance on screen, if there is one. */
  get current(): LibraryEntry | undefined {
    return this.library.find((e) => e.id === this.selected);
  }

  /** What the Play bar speaks for: the instance on screen, or else the last one played. */
  get focused(): LibraryEntry | undefined {
    if (this.current) return this.current;
    const newest = (entry: LibraryEntry) => entry.lastPlayed ?? '';
    return [...this.library].sort((a, b) => Number(b.running) - Number(a.running) || newest(b).localeCompare(newest(a)))[0];
  }

  /** Where an install goes when nothing has said otherwise. */
  get target(): LibraryEntry | undefined {
    return this.current ?? this.library[0];
  }

  /** Mods and shaders need a loader. Resource packs, data packs and modpacks don't. */
  canTake(entry: LibraryEntry, kind: ContentKind): boolean {
    return entry.loader.kind !== 'vanilla' || (kind !== 'mod' && kind !== 'shader');
  }

  /** The instances this kind of content can be installed into. */
  targets(kind: ContentKind): LibraryEntry[] {
    return this.library.filter((entry) => this.canTake(entry, kind));
  }

  get canGoBack(): boolean {
    return this.pages.length > 0 || (this.view === 'library' && !!this.current) || (this.view === 'discover' && !!this.discoverTarget);
  }

  back() {
    if (this.pages.length) this.closeProject();
    else if (this.view === 'discover' && this.discoverTarget) this.open(this.discoverTarget, this.tab);
    else this.selected = null;
  }

  entry(id: string): LibraryEntry | undefined {
    return this.library.find((e) => e.id === id);
  }

  get project(): ProjectTarget | undefined {
    return this.pages.at(-1);
  }

  go(view: View, discoverTarget?: string, discoverKind?: ContentKind) {
    this.view = view;
    this.discoverTarget = discoverTarget;
    this.discoverKind = discoverKind;
    this.pages = [];
    if (view === 'library') this.selected = null;
  }

  open(id: string, tab: InstanceTab = 'mods') {
    this.selected = id;
    this.tab = tab === 'mods' && this.entry(id)?.loader.kind === 'vanilla' ? 'worlds' : tab;
    this.view = 'library';
    this.pages = [];
  }

  openProject(target: ProjectTarget) {
    this.pages.push(target);
  }

  closeProject() {
    this.pages.pop();
  }

  /** Opens a mod from an instance, wherever its page turns out to live. */
  openMod(instance: string, mod: LockedMod) {
    const preview = { title: mod.name, icon: iconUrl(mod.icon) };
    if (mod.source.kind === 'manual') this.openProject({ hash: mod.hash, instance, preview });
    else this.openProject({ source: mod.source.kind, id: String(mod.source.project), instance, preview });
  }

  async loadInstalled(instance: string) {
    const found = await api.installedProjects(instance).catch(() => []);
    this.installed = new Map(this.installed).set(instance, new Set(found));
  }

  /** Whether an instance has a project, or for a modpack, whether any instance came from it. */
  isInstalled(item: { source: ContentSource; id: string; kind: ContentKind; title?: string }, instance: string): boolean {
    const key = `${item.source}:${item.id}`;
    if (item.kind === 'modpack') return this.library.some((entry) => entry.packProject === key);
    const have = this.installed.get(instance);
    return !!have && (have.has(key) || have.has(`${item.kind}:${item.title}`.toLowerCase()));
  }

  /** Installs to an instance, or for a modpack creates one, and says if it worked. */
  async install(item: { source: ContentSource; id: string; kind: ContentKind; title: string }, instance: string, version?: string, world?: string) {
    try {
      if (item.kind === 'modpack') {
        const result = await api.installPack(item.source, item.id);
        await this.refresh();
        this.open(result.instance.id);
      } else {
        await api.installContent(instance, item.source, item.id, item.kind, version, world);
        await Promise.all([this.refresh(), this.loadInstalled(instance)]);
        this.contentTick++;
      }
    } catch (error) {
      this.fail(error, `Could not install ${item.title}`);
    }
  }

  async start() {
    onCoreEvent((event) => this.handle(event));
    // Instances can change while Waymark is in the background, for example when
    // a tool's installer adds a pre-launch command, so coming back rereads them.
    window.addEventListener('focus', () => this.refresh().catch(() => {}));
    onGameLog((batch) => {
      const lines = this.logs.get(batch.instance) ?? [];
      lines.push(...batch.lines);
      if (lines.length > MAX_LOG_LINES) lines.splice(0, lines.length - MAX_LOG_LINES);
      this.logs.set(batch.instance, lines);
      this.logTick++;
    });
    await Promise.allSettled([this.refresh(), this.refreshAccounts()]);
    this.loaded = true;
    await this.recover();
    onLaunchRequest(() => this.launchRequested());
    onFileDrop((event) => {
      this.dragging = event.kind === 'over';
      if (event.kind === 'drop') this.dropFiles(event.paths);
    });
    await this.launchRequested();
    this.watchUpdates();
    this.watchFriends();
    if (!inTauri) {
      const { applyPreview } = await import('./mock');
      applyPreview(this);
    }
  }

  private async launchRequested() {
    const id = await api.takePendingLaunch().catch(() => null);
    if (id) await this.play(id);
  }

  /** Files dropped on the window go wherever makes sense for what's on screen. */
  async dropFiles(paths: string[]) {
    const ending = (...names: string[]) => paths.filter((path) => names.some((name) => path.toLowerCase().endsWith(name)));
    const [images, jars, packs, zips] = [ending('.png'), ending('.jar'), ending('.mrpack'), ending('.zip')];
    if (images.length && this.view === 'skins') this.droppedSkins = images;
    const instance = this.current?.id;
    const vanilla = this.current?.loader.kind === 'vanilla';
    if (jars.length && !instance) this.toast('bad', 'Open an instance first', 'Mods go into the instance on screen.');
    if (jars.length && vanilla) this.toast('bad', "Vanilla instances don't use mods", 'Make an instance with a mod loader for these.');
    const zipKind = this.tab === 'shaders' ? 'shader' : this.tab === 'resourcepacks' ? 'resourcepack' : null;
    try {
      if (instance && jars.length && !vanilla) await this.addFiles(instance, 'mod', jars);
      if (instance && zipKind && zips.length && this.view === 'library' && (!vanilla || zipKind === 'resourcepack')) await this.addFiles(instance, zipKind, zips);
      for (const pack of [...packs, ...(zipKind ? [] : zips)]) {
        const result = await api.installPackFile(pack);
        await this.refresh();
        this.open(result.instance.id);
      }
    } catch (error) {
      this.fail(error, 'Could not install that file');
    }
  }

  private async addFiles(instance: string, kind: ContentKind, paths: string[]) {
    const mark = await api.addLocalFiles(instance, kind, paths);
    if (mark) this.toast('good', mark.label, 'You can undo this from Waymarks.');
    await this.refresh();
    this.contentTick++;
  }

  /** The window closes during play, so a new one picks up what it missed. */
  private async recover() {
    if (!inTauri) return;
    for (const entry of this.library.filter((e) => e.running)) {
      this.logs.set(entry.id, await api.recentLogs(entry.id));
      this.logTick++;
    }
    const crash = await api.takePendingCrash();
    if (crash) this.crash = crash;
  }

  /** What's waiting for the player, for the number beside Friends. */
  get friendAlerts(): number {
    return (this.friends?.incoming.length ?? 0) + (this.friends?.shares.length ?? 0);
  }

  private async watchFriends() {
    if (!inTauri) return;
    this.friendsAvailable = await api.friendsOpen();
    if (!this.friendsAvailable) return;
    this.friendAccount = await api.friendAccount().catch(() => null);
    await this.loadFriends();
    setInterval(() => this.loadFriends(), FRIENDS_EVERY);
  }

  /** A failed refresh keeps what's on screen, because the server may just be away. */
  async loadFriends() {
    if (!this.friendAccount) return;
    this.friends = await api.friendOverview().catch(() => this.friends);
  }

  /** Rust does the checking, so this only hears about it. */
  private watchUpdates() {
    if (!inTauri) return;
    onUpdateReady((version) => (this.update = version));
    api.updateStatus().then((status) => {
      this.version = status.current;
      this.update = status.ready;
    });
  }

  async installUpdate() {
    try {
      await api.installUpdate();
    } catch (error) {
      this.fail(error, 'Could not install the update');
    }
  }

  async refresh() {
    this.library = await api.library();
  }

  async refreshAccounts() {
    this.accounts = await api.accounts();
  }

  private handle(event: CoreEvent) {
    switch (event.type) {
      case 'taskStarted':
        this.tasks.push({ id: event.id, title: event.title, done: 0, total: 0, detail: 'Starting' });
        break;
      case 'taskProgress': {
        const task = this.tasks.find((t) => t.id === event.id);
        if (task) Object.assign(task, { done: event.done, total: event.total, detail: event.detail });
        break;
      }
      case 'taskFinished':
      case 'taskFailed':
        this.tasks = this.tasks.filter((t) => t.id !== event.id);
        break;
      case 'gameStarted':
        this.logs.set(event.instance, []);
        this.logTick++;
        this.refresh();
        break;
      case 'serverStopped':
        this.contentTick++;
        break;
      case 'gameExited':
        this.refresh();
        this.contentTick++;
        if (event.crash) this.crash = { instance: event.instance, report: event.crash };
        break;
    }
  }

  /** Quietly starts the game after an update, and says so if the update had to be undone. */
  async checkUpdate(id: string) {
    const name = this.entry(id)?.name ?? 'That instance';
    try {
      const check = await api.checkStart(id);
      if (check.result === 'passed') this.toast('good', `${name} still starts`, 'Waymark Launcher tried the update and it loads.');
      else if (check.result === 'unclear') this.toast('info', `${name} took too long to check`, 'The update stayed. You can undo it from Waymarks.');
      else if (check.result === 'undone') {
        this.toast('bad', `That update stopped ${name} from starting`, `Waymark Launcher put it back the way it was. ${check.reason}`);
        this.contentTick++;
        await this.refresh();
      }
    } catch (error) {
      this.fail(error, 'Could not check the update');
    }
  }

  toast(tone: Toast['tone'], title: string, detail?: string, action?: ToastAction) {
    const id = ++this.toastId;
    this.toasts.push({ id, tone, title, detail, action });
    setTimeout(() => this.dismiss(id), action ? UNDO_TOAST_MS : TOAST_MS);
  }

  dismiss(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  fail(error: unknown, title = 'Something went wrong') {
    const problem = error as Partial<ApiError>;
    this.toast('bad', title, problem?.message ?? String(error));
  }

  /** Adds the player's starter mods to a new instance and says what was left out. */
  async addStarters(instance: string) {
    try {
      const { added, skipped } = await api.addStarters(instance);
      await Promise.all([this.refresh(), this.loadInstalled(instance)]);
      this.contentTick++;
      const missing = skipped.filter((item) => !item.reason).map((item) => item.title);
      const absent = missing.length ? `No version for this instance: ${missing.join(', ')}` : undefined;
      if (added.length) this.toast('good', added.length === 1 ? `Added the starter mod ${added[0]}` : `Added ${added.length} starter mods`, absent);
      else if (missing.length) this.toast('info', 'None of your starter mods have a version for this instance', missing.join(', '));
      for (const item of skipped.filter((item) => item.reason).slice(0, 3)) this.toast('bad', `Could not add ${item.title}`, item.reason ?? undefined);
    } catch (error) {
      this.fail(error, 'Could not add starter mods');
    }
  }

  /** The listed server being got ready to join, if any. */
  joiningServer = $state<string | null>(null);

  /**
   * Joins a listed server: the instance made for it last time, or a new one the
   * first time, then straight into the server. A pack that wants files fetched
   * by hand opens its instance instead, where those are listed.
   */
  async joinServer(server: { id: string; name: string }) {
    if (this.joiningServer) return;
    this.joiningServer = server.id;
    try {
      const ready = await api.serverInstance(server.id);
      await this.refresh();
      if (ready.updated) this.toast('good', `Updated ${ready.instance.name} for ${server.name}`, 'The server moved to a new version of its pack. You can undo this from Waymarks.');
      if (ready.manual.length) {
        this.open(ready.instance.id);
        this.toast('info', 'A few files need downloading by hand', `${server.name} needs them before you can join.`);
      } else {
        await this.play(ready.instance.id, { kind: 'server', address: ready.address });
      }
    } catch (error) {
      this.fail(error, `Could not join ${server.name}`);
    }
    this.joiningServer = null;
  }

  /** Brings back something deleted, wherever it was. */
  async restore(id: string) {
    try {
      const item = await api.restoreTrashed(id);
      await this.refresh();
      this.contentTick++;
      this.toast('good', `Restored ${item.name}`);
    } catch (error) {
      this.fail(error, 'Could not restore that');
    }
  }

  async play(id: string, join?: Join) {
    // An instance made for a listed server goes straight into it, wherever Play is pressed.
    const server = this.entry(id)?.server;
    if (!join && server) join = { kind: 'server', address: server.address };
    try {
      await api.launch(id, join);
    } catch (error) {
      const code = (error as ApiError)?.code;
      if (code === 'sign_in_required') this.dialog = 'signin';
      else if (code === 'app_not_approved') {
        this.toast('info', 'Sign-in is not available yet', 'Waymark Launcher is waiting on Mojang to approve it for Minecraft sign-in.');
      } else this.fail(error, 'The game could not start');
    }
  }

  async stop(id: string) {
    try {
      await api.stopGame(id);
    } catch (error) {
      this.fail(error);
    }
  }
}

export const app = new AppState();

export async function showWindow() {
  if (!inTauri) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow().show();
}

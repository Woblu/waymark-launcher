import { api, inTauri, onCoreEvent, onGameLog } from './api';
import type { Account, ApiError, CoreEvent, CrashReport, LibraryEntry } from './api';

export interface AppUpdate {
  version: string;
  install: () => Promise<void>;
}

export type View = 'library' | 'discover' | 'settings';
export type InstanceTab = 'mods' | 'trail' | 'logs' | 'settings';

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
}

export interface LogLine {
  text: string;
  error: boolean;
}

const MAX_LOG_LINES = 8000;
const TOAST_MS = 6000;

class AppState {
  view = $state<View>('library');
  selected = $state<string | null>(null);
  tab = $state<InstanceTab>('mods');
  discoverTarget = $state<string | undefined>(undefined);

  library = $state<LibraryEntry[]>([]);
  accounts = $state<Account[]>([]);
  tasks = $state<TaskState[]>([]);
  toasts = $state<Toast[]>([]);
  crash = $state<{ instance: string; report: CrashReport } | null>(null);
  dialog = $state<'none' | 'new' | 'signin'>('none');
  loaded = $state(false);
  update = $state<AppUpdate | null>(null);

  /** Log text stays out of reactive state; the tick says when it changed. */
  logs = new Map<string, LogLine[]>();
  logTick = $state(0);

  /** Bumped whenever an instance's contents change, so open tabs reload. */
  contentTick = $state(0);

  private toastId = 0;

  get activeAccount(): Account | undefined {
    return this.accounts.find((a) => a.active);
  }

  get current(): LibraryEntry | undefined {
    return this.library.find((e) => e.id === this.selected) ?? this.library[0];
  }

  entry(id: string): LibraryEntry | undefined {
    return this.library.find((e) => e.id === id);
  }

  go(view: View, discoverTarget?: string) {
    this.view = view;
    this.discoverTarget = discoverTarget;
  }

  open(id: string, tab: InstanceTab = 'mods') {
    this.selected = id;
    this.tab = tab;
    this.view = 'library';
  }

  async start() {
    onCoreEvent((event) => this.handle(event));
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
    this.checkForUpdate();
    if (!inTauri) {
      const { applyPreview } = await import('./mock');
      applyPreview(this);
    }
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

  /** A failed check says nothing, because being offline isn't a problem. */
  private async checkForUpdate() {
    if (!inTauri || import.meta.env.DEV) return;
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const found = await check();
      if (!found) return;
      this.update = {
        version: found.version,
        install: async () => {
          await found.downloadAndInstall();
          const { relaunch } = await import('@tauri-apps/plugin-process');
          await relaunch();
        },
      };
    } catch {
      // No release published yet, or no connection.
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
      case 'gameExited':
        this.refresh();
        this.contentTick++;
        if (event.crash) this.crash = { instance: event.instance, report: event.crash };
        break;
    }
  }

  toast(tone: Toast['tone'], title: string, detail?: string) {
    const id = ++this.toastId;
    this.toasts.push({ id, tone, title, detail });
    setTimeout(() => this.dismiss(id), TOAST_MS);
  }

  dismiss(id: number) {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  fail(error: unknown, title = 'Something went wrong') {
    const problem = error as Partial<ApiError>;
    this.toast('bad', title, problem?.message ?? String(error));
  }

  async play(id: string) {
    try {
      await api.launch(id);
    } catch (error) {
      const code = (error as ApiError)?.code;
      if (code === 'sign_in_required') this.dialog = 'signin';
      else if (code === 'app_not_approved') {
        this.toast('info', 'Sign-in is not available yet', 'Waymark is waiting on Mojang to approve it for Minecraft sign-in.');
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

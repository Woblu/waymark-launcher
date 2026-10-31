// Sample data for working on the UI in a plain browser. Never used in the app.

import type { ContentHit, LibraryEntry, LockedMod, Waymark } from './api';
import type { app as App } from './state.svelte';

const hoursAgo = (hours: number) => new Date(Date.now() - hours * 3600e3).toISOString();

function instance(id: string, name: string, minecraft: string, kind: LibraryEntry['loader']['kind'], mods: number, hours: number, last: number): LibraryEntry {
  return {
    id,
    name,
    minecraft,
    loader: { kind, version: kind === 'vanilla' ? undefined : '0.20.1' },
    versionId: minecraft,
    createdAt: hoursAgo(900),
    lastPlayed: hoursAgo(last),
    playtimeSecs: hours * 3600,
    mods,
    waymarks: Math.max(2, Math.round(mods / 9)),
    lastWaymark: hoursAgo(last + 0.4),
    running: false,
    bisecting: false,
  };
}

const LIBRARY: LibraryEntry[] = [
  instance('emberfall', 'Emberfall', '1.21.4', 'fabric', 214, 38, 3),
  instance('deep-industry', 'Deep Industry', '1.21.1', 'neoforge', 312, 121, 26),
  instance('hollow-earth', 'Hollow Earth', '1.20.1', 'forge', 188, 64, 74),
  instance('quiet-skies', 'Quiet Skies', '1.21.4', 'fabric', 42, 12, 120),
  instance('starfall', 'Starfall', '26.3', 'vanilla', 0, 3, 200),
  instance('frontier', 'Frontier', '1.21.1', 'quilt', 96, 22, 340),
];

const NAMES = ['Sodium', 'Iris Shaders', 'Lithium', 'Fabric API', 'Mod Menu', 'Distant Horizons', 'Terralith', 'Xaero’s Minimap', 'Jade', 'JourneyMap', 'Create', 'Farmer’s Delight', 'Supplementaries', 'Waystones', 'Cloth Config', 'YetAnotherConfigLib'];

function sampleMods(): LockedMod[] {
  return Array.from({ length: 214 }, (_, i) => {
    const name = i < NAMES.length ? NAMES[i] : `${NAMES[i % NAMES.length]} Addon ${i}`;
    const origin: LockedMod['origin'] = i % 5 === 3 ? { kind: 'dependency', of: ['Sodium'] } : i % 3 === 0 ? { kind: 'manual' } : { kind: 'pack', name: 'Emberfall' };
    return {
      name,
      file: `${name.toLowerCase().replaceAll(' ', '-')}.jar`,
      hash: `hash-${i}`,
      size: 80_000 + ((i * 7919) % 900) * 12_000,
      enabled: i % 17 !== 9,
      origin,
      side: 'both',
      version: `${(i % 4) + 1}.${i % 9}.${i % 7}+mc1.21.4`,
      source: { kind: 'modrinth' },
    };
  });
}

function waymark(id: string, hours: number, label: string, kind: Waymark['kind'], added: string[], removed: string[], changed: string[]): Waymark {
  return { id, at: hoursAgo(hours), label, kind, added, removed, changed, files: 1402, mods: 214, bytes: 2_140_000_000 };
}

const TRAIL: Waymark[] = [
  waymark('w1', 340, 'Installed from modpack', 'import', ['mods/sodium.jar', 'mods/lithium.jar'], [], []),
  waymark('w2', 200, 'Added Distant Horizons and 1 dependency', 'install', ['mods/distanthorizons.jar', 'mods/forgeconfigapiport.jar'], [], []),
  waymark('w3', 96, 'Before the end fight', 'manual', [], [], ['config/sodium-options.json']),
  waymark('w4', 50, 'Updated 12 mods', 'update', ['mods/iris-1.8.8.jar'], ['mods/iris-1.8.6.jar'], ['config/iris.properties']),
  waymark('w5', 28, 'Added Create and 3 dependencies', 'install', ['mods/create.jar', 'mods/flywheel.jar', 'mods/ponder.jar', 'mods/registrate.jar'], [], []),
  waymark('w6', 3.4, 'Before playing', 'launch', [], [], ['options.txt']),
];

const SUMMARIES = [
  'A modern rendering engine that makes the game dramatically faster.',
  'Build contraptions, factories and moving machines that feel alive.',
  'A rich, cinematic world generator with towering mountains.',
  'Quality of life tweaks that stay out of your way.',
];

function sampleHits(kind: ContentHit['kind']): ContentHit[] {
  return Array.from({ length: 12 }, (_, i) => ({
    source: 'modrinth',
    id: `project-${i}`,
    slug: `project-${i}`,
    title: ['Lantern Light', 'Skyward', 'Ironworks', 'Mossy Ruins', 'Northern Reach', 'Tideglass', 'Ashen Peaks', 'Hearthstead', 'Wayfarer', 'Duskwood', 'Glimmer', 'Old Roads'][i],
    summary: SUMMARIES[i % SUMMARIES.length],
    author: ['ember', 'stonewright', 'northwind', 'oakhollow'][i % 4],
    downloads: 12_400_000 / (i + 1),
    icon: null,
    banner: null,
    kind,
    categories: ['Optimization'],
  }));
}

/** Lets a screenshot open any screen directly, like  or . */
export function applyPreview(app: typeof App) {
  const hash = location.hash.slice(1);
  if (hash === 'discover' || hash === 'settings') app.go(hash);
  if (hash === 'mods' || hash === 'bisect') app.tab = 'mods';
  if (hash === 'trail') app.tab = 'trail';
  if (hash === 'instance-settings') app.tab = 'settings';
  if (hash === 'new') app.dialog = 'new';
  if (hash === 'logs') {
    app.tab = 'logs';
    app.logs.set('emberfall', SAMPLE_LOG);
    app.logTick++;
  }
  if (hash === 'task') {
    app.tasks.push({ id: 't', title: 'Launching Emberfall', done: 318, total: 506, detail: 'Downloading Minecraft' });
  }
  if (hash === 'crash') {
    app.crash = {
      instance: 'emberfall',
      report: {
        reportFile: null,
        findings: [
          { title: 'sodium is missing', detail: 'Another mod needs sodium, but it isn’t installed.', suspects: ['sodium'], fix: { kind: 'installMod', id: 'sodium' } },
        ],
      },
    };
  }
}

const SAMPLE_LOG = [
  '[main/INFO] Loading Minecraft 1.21.4 with Fabric Loader 0.20.1',
  '[main/INFO] Loading 214 mods:',
  '[main/WARN] Mod sodium uses a deprecated API',
  '[Render thread/INFO] Setting user: Wayfarer',
  '[Render thread/INFO] Backend library: LWJGL version 3.3.3+5',
  '[Render thread/ERROR] Failed to load texture minecraft:textures/block/stone.png',
  '[Render thread/INFO] OpenAL initialized on device Speakers',
  '[Render thread/INFO] Sound engine started',
].map((text) => ({ text, error: text.includes('/ERROR]') }));

export function mockCall(command: string, args?: Record<string, unknown>): unknown {
  switch (command) {
    case 'library':
      return location.hash === '#empty' ? [] : LIBRARY.map((entry, index) => ({ ...entry, running: location.hash === '#running' && index === 1 }));
    case 'accounts':
      return [{ uuid: 'a', name: 'Wayfarer', ownsGame: true, verifiedAt: hoursAgo(2), active: true }];
    case 'minecraft_versions':
      return { latest: '26.3', versions: ['26.3', '26.2', '26.1.2', '1.21.11', '1.21.4', '1.21.1', '1.20.1'].map((id) => ({ id, kind: 'release' })) };
    case 'instance_mods':
    case 'set_mod_enabled':
      return sampleMods();
    case 'trail':
      return TRAIL;
    case 'pending_changes':
      return { added: ['mods/new-mod.jar'], removed: [], changed: ['config/a.toml', 'options.txt'] };
    case 'check_updates':
      return [
        { hash: 'hash-1', name: 'Iris Shaders', current: '1.8.6', latest: '1.8.8', held: false },
        { hash: 'hash-2', name: 'Lithium', current: '0.14', latest: '0.15', held: false },
        { hash: 'hash-5', name: 'Terralith', current: '2.5', latest: '2.6', held: true },
      ];
    case 'search_content':
      return { hits: sampleHits((args?.query as { kind: ContentHit['kind'] }).kind), total: 12 };
    case 'curseforge_available':
      return true;
    case 'bisect_status':
      return location.hash === '#bisect' ? { status: 'testing', round: 3, estimate: 11, enabled: 107, total: 214 } : null;
    case 'external_instances':
      return [{ launcher: 'prism', name: 'Old Survival', gameDir: 'x', minecraft: '1.20.1', loader: { kind: 'fabric' } }];
    case 'settings':
      return { memoryMb: 6144, releaseDelayHours: 48, showSnapshots: false };
    default:
      return null;
  }
}

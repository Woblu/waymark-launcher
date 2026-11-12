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
  { ...instance('first-light', 'First Light', '1.5.2', 'vanilla', 0, 5, 420), legacyFixes: false },
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
      source: { kind: 'modrinth', project: 'sample' },
    };
  });
}

function waymark(id: string, hours: number, label: string, kind: Waymark['kind'], added: string[], removed: string[], changed: string[]): Waymark {
  return { id, at: hoursAgo(hours), label, kind, added, removed, changed, files: 1402, mods: 214, bytes: 2_140_000_000 };
}

const TRAIL: Waymark[] = [
  waymark('w1', 340, 'Installed from modpack', 'import', ['mods/sodium.jar', 'mods/lithium.jar'], [], []),
  waymark('w2', 200, 'Installed Distant Horizons and 1 dependency', 'install', ['mods/distanthorizons.jar', 'mods/forgeconfigapiport.jar'], [], []),
  waymark('w3', 96, 'Before the end fight', 'manual', [], [], ['config/sodium-options.json']),
  waymark('w4', 50, 'Updated 12 mods', 'update', ['mods/iris-1.8.8.jar'], ['mods/iris-1.8.6.jar'], ['config/iris.properties']),
  waymark('w5', 28, 'Installed Create and 3 dependencies', 'install', ['mods/create.jar', 'mods/flywheel.jar', 'mods/ponder.jar', 'mods/registrate.jar'], [], []),
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

const SAMPLE_BODY = `# Sodium

A free and open-source **rendering engine** for Minecraft that _greatly improves_ frame rates. See [the wiki](https://github.com/CaffeineMC/sodium/wiki) or [Lithium](https://modrinth.com/mod/lithium), and read [how to install](#how-to-install).

![Screenshot](https://cdn.modrinth.com/data/AANobbMI/images/f4b5a9b6a3f7d7a3.png)

## Features

- Modern OpenGL rendering pipeline
- Much faster chunk building
  - Nested item with \`inline code\`
- [x] Finished task
- [ ] Unfinished task

> This mod replaces parts of the rendering engine, so other rendering mods may not work with it.

### How to install

1. Install Fabric
2. Drop the file into your mods folder

| Version | Minecraft | Status |
|---|---|---|
| 0.6 | 1.21 | Supported |
| 0.5 | 1.20 | Bug fixes |

\`\`\`toml
[rendering]
threads = 4
\`\`\`

<details><summary>What about OptiFine?</summary>

They can't be used together.

</details>

<center>Centered with <kbd>F3</kbd> and <sub>small</sub> text.</center>

<p style="text-align: right; position: fixed; top: 0; color: red">Right aligned, and the rest of this style is removed.</p>

<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>

<iframe src="https://evil.example/frame"></iframe>

<script>window.pwned = true</script>
<img src=x onerror="window.pwned = true">
<a href="javascript:window.pwned = true">A hostile link with its address removed</a>
<button class="btn primary">A fake button</button>
<form action="https://evil.example"><input name="password"></form>
<svg onload="window.pwned = true"></svg>
<style>body { display: none }</style>
`;

const SAMPLE_PROJECT = {
  source: 'modrinth',
  id: 'AANobbMI',
  slug: 'sodium',
  kind: 'mod',
  title: 'Sodium',
  summary: 'The fastest and most compatible rendering optimization mod for Minecraft.',
  body: SAMPLE_BODY,
  format: 'markdown',
  icon: null,
  downloads: 258107554,
  followers: 42360,
  published: '2021-01-03T00:53:34Z',
  updated: new Date(Date.now() - 86400000 * 16).toISOString(),
  categories: ['optimization', 'game-mechanics'],
  loaders: ['fabric', 'neoforge', 'quilt'],
  gameVersions: ['1.16.5', '1.17.1', '1.18.2', '1.19.4', '1.20.1', '1.21.4', '26.1', '26.2', '26.3-rc-1', '26.3'],
  client: 'required',
  server: 'unsupported',
  license: { label: 'Polyform Shield License 1.0.0', url: 'https://example.com/license' },
  links: [
    { label: 'Issues', url: 'https://github.com/CaffeineMC/sodium/issues' },
    { label: 'Source', url: 'https://github.com/CaffeineMC/sodium' },
    { label: 'Ko-fi', url: 'https://ko-fi.com/x' },
  ],
  gallery: [
    { url: 'https://cdn.modrinth.com/data/AANobbMI/images/a.webp', full: 'https://cdn.modrinth.com/data/AANobbMI/images/a.png', title: 'Underwater lighting', description: 'Smooth lighting and fog under water.' },
    { url: 'https://cdn.modrinth.com/data/AANobbMI/images/b.webp', full: 'https://cdn.modrinth.com/data/AANobbMI/images/b.png', title: null, description: null },
  ],
  creators: [
    { name: 'jellysquid3', role: 'Founder', avatar: null, url: 'https://modrinth.com/user/jellysquid3' },
    { name: 'IMS', role: 'Maintainer', avatar: null, url: 'https://modrinth.com/user/IMS' },
  ],
  page: 'https://modrinth.com/mod/sodium',
};

const SAMPLE_VERSIONS = ['release', 'beta', 'alpha'].map((channel, index) => ({
  id: `v${index}`,
  name: `Sodium 0.6.${13 - index} for Fabric 1.21.4`,
  number: `mc1.21.4-0.6.${13 - index}-fabric`,
  channel,
  published: hoursAgo(24 * (index + 1) * 9),
  downloads: 1267694 >> index,
  loaders: ['fabric'],
  gameVersions: ['1.21.4'],
  file: `sodium-fabric-0.6.${13 - index}+mc1.21.4.jar`,
  size: 1234567,
  available: true,
}));

/** Lets a screenshot open any screen directly, like  or . */
export function applyPreview(app: typeof App) {
  const hash = location.hash.slice(1);
  if (['mods', 'bisect', 'trail', 'worlds', 'instance-settings', 'logs'].includes(hash)) app.selected = 'emberfall';
  if (hash === 'discover' || hash === 'servers' || hash === 'settings' || hash === 'skins' || hash === 'screenshots') app.go(hash);
  // `#settings-updates` opens one part of Settings.
  if (hash.startsWith('settings-')) {
    app.settingsSection = hash.slice('settings-'.length);
    app.go('settings');
  }
  if (hash === 'server') app.go('servers');
  if (hash === 'mods' || hash === 'bisect') app.tab = 'mods';
  if (hash === 'trail') app.tab = 'trail';
  if (hash === 'worlds') app.tab = 'worlds';
  if (hash === 'instance-settings') app.tab = 'settings';
  if (hash === 'old-release-settings') {
    app.selected = 'first-light';
    app.tab = 'settings';
  }
  if (hash === 'new') app.dialog = 'new';
  if (hash === 'project') app.openProject({ source: 'modrinth', id: 'sample' });
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

// Made-up servers for the browser preview of the server list.
const SAMPLE_SERVERS = [
  { id: 'srv-lantern', slug: 'lanternfall', name: 'Lanternfall SMP', summary: 'A slow, friendly survival world with towns, a shared market and seasons that end in a festival.', icon: null, banner: null, address: 'play.lanternfall.example', content: { kind: 'vanilla', versions: ['1.21.4', '1.21.5'], recommended: '1.21.4' }, online: true, players: 84, capacity: 200, versionName: 'Paper 1.21.4', region: 'europe', languages: ['en', 'de'], categories: ['smp', 'towns', 'economy'], plays: 1840 },
  { id: 'srv-copper', slug: 'copperline', name: 'Copperline Minigames', summary: 'Bed wars, parkour races and a new arena every Friday.', icon: null, banner: null, address: 'copperline.example:25570', content: { kind: 'vanilla', versions: ['1.20.1'], recommended: '1.20.1' }, online: true, players: 1290, capacity: null, versionName: 'Velocity 3.3', region: 'us_east', languages: ['en'], categories: ['minigames', 'bedwars', 'parkour'], plays: 9120 },
  { id: 'srv-forge', slug: 'deepforge', name: 'Deepforge Industries', summary: 'Tech and magic on a hand-tuned pack. Whitelist by application.', icon: null, banner: null, address: 'deepforge.example', content: { kind: 'modpack', project: 'srv-forge', version: 'v12', name: 'Deepforge Industries', icon: null }, online: false, players: null, capacity: null, versionName: null, region: 'us_west', languages: ['en'], categories: ['technical', 'whitelisted'], plays: 210 },
];
let mockKept: { id: string; name: string; address: string; icon: string | null }[] = [];

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

let mockSettings = {
  memoryMb: 6144,
  releaseDelayHours: 48,
  showSnapshots: false,
  closeWhilePlaying: true,
  theme: 'system',
  customThemes: [],
  autoUpdate: true,
  checkUpdates: true,
  language: 'system',
  shareSettings: false,
  discordPresence: false,
  shareServers: true,
  sharedChoices: {} as Record<string, boolean>,
  starters: {
    '1.20.1': [
      { source: 'modrinth', project: 'AANobbMI', kind: 'mod', title: 'Sodium', icon: null },
      { source: 'modrinth', project: 'mOgUt4GM', kind: 'mod', title: 'Mod Menu', icon: null },
    ],
  } as Record<string, unknown[]>,
};

export function mockCall(command: string, args?: Record<string, unknown>): unknown {
  switch (command) {
    case 'library':
      return location.hash === '#empty' ? [] : LIBRARY.map((entry, index) => ({ ...entry, running: location.hash === '#running' && index === 1 }));
    case 'accounts':
      return [{ uuid: 'a', name: 'Wayfarer', ownsGame: true, verifiedAt: hoursAgo(2), active: true }];
    case 'minecraft_versions':
      return { latest: '26.3', versions: ['26.3', '26.2', '26.1.2', '1.21.11', '1.21.4', '1.21.1', '1.20.1'].map((id) => ({ id, kind: 'release' })) };
    case 'instance_content':
    case 'set_content_enabled':
      return args?.kind === 'mod' ? sampleMods() : sampleMods().slice(0, 6).map((pack) => ({ ...pack, file: pack.file.replace('.jar', '.zip') }));
    case 'installed_projects':
      return ['modrinth:project-1', 'modrinth:project-4'];
    case 'trail':
      return TRAIL;
    case 'pending_changes':
      return { added: ['mods/new-mod.jar'], removed: [], changed: ['config/a.toml', 'options.txt'] };
    case 'check_updates':
      return [
        { kind: 'mod', source: 'modrinth', project: 'iris', hash: 'hash-1', name: 'Iris Shaders', current: '1.8.6', latest: '1.8.8', held: false },
        { kind: 'mod', source: 'modrinth', project: 'lithium', hash: 'hash-2', name: 'Lithium', current: '0.14', latest: '0.15', held: false },
        { kind: 'mod', source: 'modrinth', project: 'terralith', hash: 'hash-5', name: 'Terralith', current: '2.5', latest: '2.6', held: true },
      ];
    case 'search_content':
      return { hits: sampleHits((args?.query as { kind: ContentHit['kind'] }).kind), total: 12 };
    case 'curseforge_available':
      return true;
    case 'search_categories':
      return ['adventure', 'decoration', 'magic', 'optimization', 'technology', 'utility'].map((id) => ({ id, name: id, group: 'categories' }));
    case 'project_page':
      return SAMPLE_PROJECT;
    case 'project_fit':
      return { version: '0.6.13+mc1.21.4', requires: [{ source: 'modrinth', id: 'fabric-api', title: 'Fabric API', icon: null }] };
    case 'project_versions':
      return { versions: SAMPLE_VERSIONS, total: SAMPLE_VERSIONS.length };
    case 'version_changelog':
      return '- Fixed a crash with **Iris** on startup\n- Improved chunk meshing by about 12%';
    case 'worlds':
      return {
        canJoin: true,
        worlds: [
          { folder: 'New World', name: 'River Base', lastPlayed: Date.now() - 3 * 3600e3, mode: 'Survival', version: '1.21.4', hasIcon: false },
          { folder: 'Creative', name: 'Test Flat', lastPlayed: Date.now() - 80 * 3600e3, mode: 'Creative', version: '1.21.4', hasIcon: false },
        ],
      };
    case 'recent_worlds':
      return [{ instance: 'emberfall', instanceName: 'Emberfall', folder: 'New World', name: 'River Base', lastPlayed: Date.now() - 3 * 3600e3, mode: 'Survival', version: '1.21.4', hasIcon: false }];
    case 'servers':
      return [{ index: 0, name: 'Hypixel', address: 'mc.hypixel.net', icon: null }];
    case 'server_status':
      return { online: true, players: 38210, capacity: 200000, version: '1.8-1.21', motd: 'Hypixel Network', latencyMs: 41, icon: null };
    case 'trashed':
    case 'screenshots':
    case 'manual_files':
    case 'log_files':
      return [];
    case 'take_pending_launch':
      return null;
    case 'host_status':
      return { ready: false, eulaAccepted: false, running: false, address: '192.168.1.20:25565', port: 25565 };
    case 'skin_library':
      return { saved: [], defaults: [{ name: 'Steve', variant: 'classic', url: 'https://textures.minecraft.net/texture/31f477eb1a7beee631c2ca64d06f8f68fa93a3386d04452ab27f43acdf1b60cb' }] };
    case 'data_packs':
      return [];
    case 'bisect_status':
      return location.hash === '#bisect' ? { status: 'testing', round: 3, estimate: 11, enabled: 107, total: 214 } : null;
    case 'external_instances':
      return [
        { launcher: 'prism', name: 'Old Survival', gameDir: 'x', minecraft: '1.20.1', loader: { kind: 'fabric' } },
        { launcher: 'modrinth', name: 'MCSR Ranked', gameDir: 'y', minecraft: '1.16.1', loader: { kind: 'fabric' } },
        { launcher: 'modrinth', name: 'Cozy Farm', gameDir: 'z', minecraft: '1.21.1', loader: { kind: 'neoforge' } },
        { launcher: 'official', name: 'Minecraft Launcher', gameDir: 'w', minecraft: 'latest-release', loader: { kind: 'vanilla' } },
      ];
    case 'settings':
      return structuredClone(mockSettings);
    case 'save_settings':
      mockSettings = structuredClone(args?.settings as typeof mockSettings);
      return null;
    case 'public_servers': {
      const kind = (args?.query as { kind?: string })?.kind;
      const found = SAMPLE_SERVERS.filter((server) => !kind || (kind === 'vanilla') === (server.content.kind === 'vanilla'));
      return { servers: found, total: found.length };
    }
    case 'server_categories':
      return [
        { id: 'minigames', name: 'minigames', group: 'gameplay' },
        { id: 'towns', name: 'towns', group: 'gameplay' },
        { id: 'economy', name: 'economy', group: 'features' },
        { id: 'smp', name: 'smp', group: 'community' },
        { id: 'whitelisted', name: 'whitelisted', group: 'meta' },
      ];
    case 'public_server': {
      const server = SAMPLE_SERVERS.find((item) => item.id === args?.id) ?? SAMPLE_SERVERS[0];
      return { ...server, description: `## Welcome to ${server.name}\n\nJoin from Waymark Launcher and you land straight in the spawn town. Be kind, build big, and read the rules at spawn.`, gallery: [], links: [{ label: 'Discord', url: 'https://discord.gg/example' }], followers: 1260, page: `https://modrinth.com/server/${server.slug}`, instance: null };
    }
    case 'kept_servers':
      return mockKept;
    case 'keep_server': {
      const server = args?.server as (typeof mockKept)[number];
      mockKept = mockKept.filter((other) => other.id !== server.id);
      if (args?.keep) mockKept.unshift(server);
      return mockKept;
    }
    case 'discord_available':
      return true;
    case 'known_servers':
      return [{ name: 'Hypixel', address: 'mc.hypixel.net', icon: null, instances: [{ id: 'emberfall', name: 'Emberfall', minecraft: '1.21.4' }, { id: 'quiet-skies', name: 'Quiet Skies', minecraft: '1.21.4' }] }];
    case 'shared_settings':
      return {
        sets: mockSettings.shareSettings
          ? [{ family: 'modern', label: 'Minecraft 1.13 and newer', from: 'Emberfall', options: [
              { key: 'fov', value: '0.5', shared: mockSettings.sharedChoices.fov ?? true },
              { key: 'key_key.jump', value: 'key.keyboard.space', shared: mockSettings.sharedChoices['key_key.jump'] ?? true },
              { key: 'resourcePacks', value: '["vanilla"]', shared: mockSettings.sharedChoices.resourcePacks ?? false },
            ] }]
          : [],
        serversFrom: mockSettings.shareSettings ? 'Emberfall' : null,
      };
    case 'set_legacy_fixes': {
      const entry = LIBRARY.find((item) => item.id === args?.id);
      if (entry) entry.legacyFixes = Boolean(args?.on);
      return entry;
    }
    case 'add_starters':
      return { added: ['Sodium', 'Mod Menu'], skipped: [{ title: 'Embeddium', reason: null }] };
    default:
      return null;
  }
}

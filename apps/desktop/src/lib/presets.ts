// Ready-made setups, picked by what the player wants to play. The words live here so
// they're translated like the rest of the app, and the core does the installing.

import type { ContentKind, LoaderKind, Starter } from './api';

/** A Modrinth project and the name it goes by, which stays in English. */
interface Item {
  project: string;
  title: string;
  kind?: ContentKind;
}

export interface Preset {
  id: string;
  title: string;
  summary: string;
  /** A Modrinth modpack, installed the way Discover installs one. */
  modpack?: string;
  loader?: LoaderKind;
  /** One version only. Otherwise every release the core projects support is offered. */
  minecraft?: string;
  /** Decide which versions are offered, so each of these is always added. */
  core?: Item[];
  /** Added wherever there's a build for the chosen version. */
  extra?: Item[];
  /** Exact jars from speedrun.com's list instead of store builds. */
  speedrun?: 'rsg' | 'ssg' | 'ranked';
  /** Worth knowing before picking it. */
  note?: string;
}

export interface Category {
  id: string;
  title: string;
  summary: string;
  presets: Preset[];
}

const SODIUM = { project: 'AANobbMI', title: 'Sodium' };
const LITHIUM = { project: 'gvQqBUqZ', title: 'Lithium' };
const FERRITE = { project: 'uXXizFIs', title: 'FerriteCore' };
const ENTITY_CULLING = { project: 'NNAgCjsB', title: 'Entity Culling' };
const IMMEDIATELY_FAST = { project: '5ZwdcRci', title: 'ImmediatelyFast' };
const MODERN_FIX = { project: 'nmDcB62a', title: 'ModernFix' };
const DYNAMIC_FPS = { project: 'LQ3K71Q1', title: 'Dynamic FPS' };
const IRIS = { project: 'YL57xq9U', title: 'Iris' };
const CONTINUITY = { project: '1IjD5062', title: 'Continuity' };
const DYNAMIC_LIGHTS = { project: 'yBW8D80W', title: 'LambDynamicLights' };
const SKIN_LAYERS = { project: 'zV5r3pPn', title: '3D Skin Layers' };
const MOD_MENU = { project: 'mOgUt4GM', title: 'Mod Menu' };
const AXOLOTL = { project: 'p2rxzX0q', title: 'AxolotlClient' };

const PERFORMANCE = [ENTITY_CULLING, IMMEDIATELY_FAST, MODERN_FIX, DYNAMIC_FPS];
const QUALITY = [
  { project: 'EsAfCjCV', title: 'AppleSkin' },
  { project: 'aC3cM3Vq', title: 'Mouse Tweaks' },
  { project: 'w7ThoJFB', title: 'Zoomify' },
  { project: 'nvQzSEkH', title: 'Jade' },
];

export const CATEGORIES: Category[] = [
  {
    id: 'performance',
    title: 'Performance',
    summary: 'More frames and quicker loading. The game plays the same.',
    presets: [
      {
        id: 'performance',
        title: 'Faster',
        summary: 'Sodium, Lithium and FerriteCore, with a few more that cut memory use and stutter.',
        loader: 'fabric',
        core: [SODIUM, LITHIUM, FERRITE],
        extra: PERFORMANCE,
      },
      {
        id: 'performance-video',
        title: 'Faster, with more video settings',
        summary: "Everything in Faster, plus Sodium Extra's settings for fog, particles and animations.",
        loader: 'fabric',
        core: [SODIUM, LITHIUM, FERRITE],
        extra: [...PERFORMANCE, { project: 'PtjYWJkn', title: 'Sodium Extra' }, { project: 'Bh37bMuy', title: "Reese's Sodium Options" }],
      },
    ],
  },
  {
    id: 'speedrunning',
    title: 'Speedrunning',
    summary: 'Minecraft 1.16.1 with the mods speedrun.com allows, in the exact builds it lists.',
    presets: [
      {
        id: 'rsg',
        title: 'Random seed (RSG)',
        summary: 'A new world every run, with resetting, world preview and the in-game timer.',
        loader: 'fabric',
        minecraft: '1.16.1',
        speedrun: 'rsg',
      },
      {
        id: 'ssg',
        title: 'Set seed (SSG)',
        summary: 'The same world every run, with the set seed tools and the in-game timer.',
        loader: 'fabric',
        minecraft: '1.16.1',
        speedrun: 'ssg',
      },
      {
        id: 'ranked',
        title: 'MCSR Ranked',
        summary: 'Race other players on the same seed, with the Ranked mod and the mods it accepts.',
        loader: 'fabric',
        minecraft: '1.16.1',
        speedrun: 'ranked',
        core: [{ project: 'I9W1u5Ac', title: 'MCSR Ranked' }],
        note: "MCSR Ranked only starts from the launchers it allows, and Waymark Launcher isn't one of them yet.",
      },
    ],
  },
  {
    id: 'pvp',
    title: 'PvP',
    summary: 'Smooth frames, a HUD you can arrange, and zoom. No aim or reach helpers.',
    presets: [
      {
        id: 'pvp-modern',
        title: 'Modern PvP',
        summary: "The newest Minecraft with AxolotlClient's HUD, keystrokes and zoom, and performance mods.",
        loader: 'fabric',
        core: [AXOLOTL, SODIUM],
        extra: [LITHIUM, FERRITE, ENTITY_CULLING, IMMEDIATELY_FAST, { project: 'LF4WFeMi', title: 'CookeyMod' }],
      },
      {
        id: 'pvp-189',
        title: '1.8.9 PvP',
        summary: 'The version classic PvP is played on, with AxolotlClient on Legacy Fabric.',
        loader: 'legacyfabric',
        minecraft: '1.8.9',
        core: [AXOLOTL],
      },
    ],
  },
  {
    id: 'shaders',
    title: 'Shaders and visuals',
    summary: 'Iris and Sodium with a shader pack, or lighter touches without one.',
    presets: [
      {
        id: 'shaders-complementary',
        title: 'Complementary Reimagined',
        summary: 'Soft, natural light that stays close to how Minecraft looks.',
        loader: 'fabric',
        core: [SODIUM, IRIS],
        extra: [{ project: 'HVnmMxH1', title: 'Complementary Reimagined', kind: 'shader' }, CONTINUITY, DYNAMIC_LIGHTS, SKIN_LAYERS],
      },
      {
        id: 'shaders-bsl',
        title: 'BSL',
        summary: 'A classic shader pack with bright, colorful light.',
        loader: 'fabric',
        core: [SODIUM, IRIS],
        extra: [{ project: 'Q1vvjJYV', title: 'BSL', kind: 'shader' }, CONTINUITY, DYNAMIC_LIGHTS, SKIN_LAYERS],
      },
      {
        id: 'visuals',
        title: 'Visuals without shaders',
        summary: 'Connected textures, dynamic lights and 3D skin layers, light enough for any computer.',
        loader: 'fabric',
        core: [SODIUM],
        extra: [CONTINUITY, DYNAMIC_LIGHTS, SKIN_LAYERS],
      },
    ],
  },
  {
    id: 'quality',
    title: 'Quality of life',
    summary: 'Small helpers that keep Minecraft feeling like Minecraft.',
    presets: [
      {
        id: 'quality',
        title: 'Essentials',
        summary: 'Mod Menu, AppleSkin, Mouse Tweaks, Zoomify and Jade.',
        loader: 'fabric',
        core: [MOD_MENU],
        extra: QUALITY,
      },
      {
        id: 'quality-more',
        title: 'Essentials and more',
        summary: 'Everything in Essentials, plus a minimap, a recipe viewer and a clearer debug screen.',
        loader: 'fabric',
        core: [MOD_MENU],
        extra: [...QUALITY, { project: '1bokaNcj', title: "Xaero's Minimap" }, { project: 'u6dRKJwZ', title: 'JEI' }, { project: '8shC1gFX', title: 'BetterF3' }],
      },
    ],
  },
  {
    id: 'building',
    title: 'Building',
    summary: 'Tools for builders: schematics, WorldEdit and more.',
    presets: [
      {
        id: 'building-litematica',
        title: 'Litematica and WorldEdit',
        summary: 'Schematics you can place and check block by block, with WorldEdit and Tweakeroo.',
        loader: 'fabric',
        core: [{ project: 'bEpr0Arc', title: 'Litematica' }],
        extra: [{ project: '1u6JkXh5', title: 'WorldEdit' }, { project: 't5wuYk45', title: 'Tweakeroo' }, SODIUM],
      },
      {
        id: 'building-axiom',
        title: 'Axiom',
        summary: 'A full building editor inside the game, in single player and on servers that allow it.',
        loader: 'fabric',
        core: [{ project: 'N6n5dqoA', title: 'Axiom' }],
        extra: [SODIUM],
      },
    ],
  },
  {
    id: 'adventure',
    title: 'Adventure modpacks',
    summary: 'Big hand-picked modpacks that change how you play.',
    presets: [
      { id: 'cobblemon', title: 'Cobblemon', summary: 'The official Cobblemon modpack: catch, train and battle Pokémon.', modpack: '5FFgwNNP' },
      { id: 'better-mc', title: 'Better MC', summary: "A big vanilla-plus modpack that keeps Minecraft's feel.", modpack: 'shFhR8Vx' },
      { id: 'prominence', title: 'Prominence II', summary: 'A lore-rich RPG with levels and custom content.', modpack: 'EGs3lC8D' },
      { id: 'create-plus', title: 'Create Plus', summary: "A light vanilla-plus pack built around Create's machines.", modpack: 't1tOiUHZ' },
    ],
  },
];

/** The store projects to install, core ones first. */
export function presetItems(preset: Preset): Starter[] {
  return [...(preset.core ?? []), ...(preset.extra ?? [])].map((item) => ({ source: 'modrinth', project: item.project, kind: item.kind ?? 'mod', title: item.title }));
}

/** Everything a preset adds, by name, for showing before it's picked. */
export function presetContents(preset: Preset): string[] {
  const named = [...(preset.core ?? []), ...(preset.extra ?? [])].map((item) => item.title);
  return preset.speedrun ? ['Allowed speedrun mods', ...named] : named;
}

// Words for the server list, shared by the list and a server's own page.

import type { LibraryEntry, PublicServer, ServerContent } from './api';
import { loaderName, number, titleCase } from './format';
import { currentLanguage } from './i18n';

const REGIONS: Record<string, string> = {
  europe: 'Europe',
  us_east: 'US East',
  us_west: 'US West',
  north_america: 'North America',
  south_america: 'South America',
  asia: 'Asia',
  oceania: 'Oceania',
  australia: 'Australia',
  russia: 'Russia',
  middle_east: 'Middle East',
  africa: 'Africa',
};

/** The regions owners pick from, in the order the filter lists them. */
export const REGION_IDS = ['europe', 'us_east', 'us_west', 'south_america', 'asia', 'russia', 'australia', 'middle_east'];

/** Languages servers are listed in, most common first. */
export const LANGUAGE_IDS = ['en', 'de', 'fr', 'es', 'ru', 'pt', 'zh', 'uk', 'nl', 'pl', 'it', 'tr', 'ja', 'ko', 'cs', 'hi'];

export const GROUP_NAMES: Record<string, string> = { gameplay: 'Gameplay', features: 'Features', community: 'Community', meta: 'Rules and modes' };
export const GROUP_ORDER = ['gameplay', 'features', 'community', 'meta'];

const SHORT_NAMES: Record<string, string> = { smp: 'SMP', pvp: 'PvP', pve: 'PvE', kitpvp: 'KitPvP', mmo: 'MMO', rpg: 'RPG', op: 'OP', oneblock: 'OneBlock', bedwars: 'Bed Wars' };

/** A category's name as players write it, so `smp` reads SMP and `kitpvp` KitPvP. */
export function categoryName(id: string): string {
  return SHORT_NAMES[id] ?? titleCase(id);
}

/** Enough to draw a server's page at once. The rest arrives with its details. */
export type ServerPreview = Pick<PublicServer, 'id' | 'name' | 'address'> & Partial<PublicServer>;

export function regionName(id: string): string {
  return REGIONS[id] ?? titleCase(id);
}

let names: Intl.DisplayNames | null = null;

/** A language's name in the language the interface is shown in. */
export function languageName(code: string): string {
  try {
    names ??= new Intl.DisplayNames([currentLanguage()], { type: 'language' });
    const name = names.of(code);
    return name ? name.charAt(0).toUpperCase() + name.slice(1) : code;
  } catch {
    return code;
  }
}

/** The pack's loader and Minecraft version, like `NeoForge 1.21.1`, or just `Modded`. */
export function packLine(content: ServerContent): string {
  if (content.kind !== 'modpack') return '';
  const loader = content.loader ? loaderName(content.loader) : '';
  return [loader, content.minecraft].filter(Boolean).join(' ') || 'Modded';
}

/**
 * What joining needs, in a few words: `Minecraft 1.21.4`, `NeoForge 1.21.1` for a
 * server's own pack, or `Modpack: Cobblemon` for a pack someone else made.
 */
export function contentLine(content: ServerContent, server?: string): string {
  if (content.kind === 'modpack') return content.name && content.name !== server ? `Modpack: ${content.name}` : packLine(content);
  const version = content.recommended ?? content.versions.at(-1);
  return version ? `Minecraft ${version}` : 'Vanilla';
}

/** `21 of 300 online`, `21 online` or `Offline`. */
export function playersLine(server: PublicServer): string {
  if (!server.online) return 'Offline';
  if (server.players === null) return 'Online';
  return server.capacity ? `${number(server.players)} of ${number(server.capacity)} online` : `${number(server.players)} online`;
}

/** The instance made for this server, if it still exists. */
export function linkedInstance(library: LibraryEntry[], id: string): LibraryEntry | undefined {
  return library.find((entry) => entry.server?.project === id);
}

/** Saved server icons come as bare base64 from servers.dat. */
export function serverPicture(icon: string): string {
  return icon.startsWith('data:') ? icon : `data:image/png;base64,${icon}`;
}

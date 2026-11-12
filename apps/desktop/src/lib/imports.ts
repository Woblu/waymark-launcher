// What the library and the import dialog both need to know about instances in
// other launchers on this computer.

import type { ExternalInstance, LibraryEntry } from './api';

/** Product names, so they stay in English everywhere. */
export const LAUNCHERS: Record<ExternalInstance['launcher'], string> = {
  official: 'Minecraft Launcher',
  prism: 'Prism Launcher',
  multimc: 'MultiMC',
  curseforge: 'CurseForge',
  modrinth: 'Modrinth App',
  atlauncher: 'ATLauncher',
  gdlauncher: 'GDLauncher',
};

/**
 * Already in the library: imported as an instance that's still there or, for imports
 * from before Waymark Launcher kept track, one with the same name, version and loader.
 */
export function inLibrary(instance: ExternalInstance, library: LibraryEntry[]): boolean {
  if (instance.importedAs) return library.some((entry) => entry.id === instance.importedAs);
  return library.some((entry) => entry.name === instance.name && entry.minecraft === instance.minecraft && entry.loader.kind === instance.loader.kind);
}

/** The Minecraft Launcher's installations can follow the newest version instead of naming one. */
export function externalVersion(instance: ExternalInstance): string {
  if (instance.minecraft === 'latest-release') return 'Latest release';
  if (instance.minecraft === 'latest-snapshot') return 'Latest snapshot';
  return instance.minecraft;
}

/** How many come from each launcher, most first. */
export function byLauncher(instances: ExternalInstance[]): [string, number][] {
  const counts = new Map<string, number>();
  for (const instance of instances) counts.set(LAUNCHERS[instance.launcher], (counts.get(LAUNCHERS[instance.launcher]) ?? 0) + 1);
  return [...counts].sort((a, b) => b[1] - a[1]);
}

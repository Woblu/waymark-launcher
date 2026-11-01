// What can be done to an instance, shared by its right-click menu and the
// More actions button on its page.

import { api, openFolder, pickImage } from './api';
import type { LibraryEntry } from './api';
import { app } from './state.svelte';
import type { MenuItem } from './state.svelte';

async function chooseIcon(entry: LibraryEntry) {
  const picture = await pickImage();
  if (!picture) return;
  await changeIcon(entry, picture);
}

async function changeIcon(entry: LibraryEntry, picture: string | null) {
  try {
    await api.setInstanceIcon(entry.id, picture);
    app.iconTick++;
    await app.refresh();
  } catch (error) {
    app.fail(error, 'Could not change the icon');
  }
}

async function makeShortcut(entry: LibraryEntry) {
  try {
    await api.createShortcut(entry.id);
    app.toast('good', 'Shortcut added', `${entry.name} is on your desktop.`);
  } catch (error) {
    app.fail(error, 'Could not add a shortcut');
  }
}

export function instanceMenu(entry: LibraryEntry): MenuItem[] {
  const items: MenuItem[] = [
    { label: 'Play', run: () => app.play(entry.id) },
    { label: 'Open folder', run: async () => openFolder(await api.gameDirectory(entry.id)) },
    { label: 'Copy', run: () => (app.instanceDialog = { kind: 'copy', id: entry.id }) },
    { label: 'Move to a folder', run: () => (app.instanceDialog = { kind: 'group', id: entry.id }) },
    { label: 'Export', run: () => (app.instanceDialog = { kind: 'export', id: entry.id }) },
    { label: 'Share with a code', run: () => (app.instanceDialog = { kind: 'share', id: entry.id }) },
    { label: 'Change icon', run: () => chooseIcon(entry) },
  ];
  if (entry.icon === 'custom') items.push({ label: 'Remove icon', run: () => changeIcon(entry, null) });
  items.push({ label: 'Add a desktop shortcut', run: () => makeShortcut(entry) });
  items.push({ label: 'Settings', run: () => app.open(entry.id, 'settings') });
  return items;
}

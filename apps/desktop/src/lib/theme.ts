// Follows the saved choice, or the computer's setting. It's remembered here too
// so the first frame is the right colour, before the settings have loaded.

import type { Theme } from './api';

const KEY = 'waymark.theme';
const darkMode = matchMedia('(prefers-color-scheme: dark)');

let choice: Theme = 'system';

function paint() {
  const light = choice === 'light' || (choice === 'system' && !darkMode.matches);
  document.documentElement.dataset.theme = light ? 'light' : 'dark';
}

export function showTheme() {
  try {
    choice = (localStorage.getItem(KEY) as Theme | null) ?? 'system';
  } catch {
    choice = 'system';
  }
  paint();
  darkMode.addEventListener('change', paint);
}

export function useTheme(theme: Theme) {
  choice = theme;
  paint();
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // The saved setting still applies once it loads.
  }
}

// Follows the saved choice, or the computer's setting. It's remembered here too
// so the first frame is the right colour, before the settings have loaded.
// Dark and Light live in tokens.css. Every other theme is two colours, a
// background and an accent, and the rest of the palette is worked out from them.

import type { CustomTheme, Theme } from './api';

const KEY = 'waymark.theme';
const COLORS_KEY = 'waymark.theme-colors';
const darkMode = matchMedia('(prefers-color-scheme: dark)');

export interface ThemeColors {
  base: string;
  accent: string;
}

export const PRESETS: CustomTheme[] = [
  { id: 'ember', name: 'Ember', base: '#140c0e', accent: '#f0a657' },
  { id: 'pine', name: 'Pine', base: '#091418', accent: '#b7d0a8' },
  { id: 'moss', name: 'Moss', base: '#0f1510', accent: '#d6bf94' },
  { id: 'twilight', name: 'Twilight', base: '#110d1b', accent: '#eba262' },
];

type Rgb = [number, number, number];

const WHITE: Rgb = [255, 255, 255];
const BLACK: Rgb = [0, 0, 0];
const DARK_TEXT = '#1a1f27';
const LIGHT_TEXT = '#f0ebe1';

function rgb(hex: string): Rgb {
  return [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16)];
}

const toHex = (color: Rgb) => `#${color.map((part) => Math.round(part).toString(16).padStart(2, '0')).join('')}`;
const mix = (a: Rgb, b: Rgb, amount: number): Rgb => [a[0] + (b[0] - a[0]) * amount, a[1] + (b[1] - a[1]) * amount, a[2] + (b[2] - a[2]) * amount];

/** How bright a colour looks, from 0 to 1. */
function brightness([red, green, blue]: Rgb): number {
  const linear = (part: number) => {
    const value = part / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue);
}

export const isLight = (base: string) => brightness(rgb(base)) > 0.3;

/** The whole palette for a background and an accent. */
export function themeTokens({ base, accent }: ThemeColors): Record<string, string> {
  const background = rgb(base);
  const highlight = rgb(accent);
  const light = isLight(base);
  const lift = light ? BLACK : WHITE;
  const step = (amount: number) => toHex(mix(background, lift, amount));
  const text = rgb(light ? DARK_TEXT : LIGHT_TEXT);
  const clear = (color: Rgb, alpha: number) => `rgb(${color.map(Math.round).join(' ')} / ${alpha}%)`;
  return {
    '--console': toHex(mix(background, light ? WHITE : BLACK, 0.35)),
    '--bg': base,
    '--panel': step(0.04),
    '--raised': step(0.09),
    '--control': step(0.14),
    '--control-hover': step(0.2),
    '--line': step(0.08),
    '--line-strong': step(0.2),
    '--text': toHex(text),
    '--text-2': toHex(mix(text, background, 0.27)),
    '--text-3': toHex(mix(text, background, 0.45)),
    '--accent': accent,
    '--accent-hover': toHex(mix(highlight, lift, 0.2)),
    '--accent-soft': clear(highlight, 13),
    '--on-accent': brightness(highlight) > 0.3 ? '#1c1206' : '#ffffff',
    '--selected': clear(highlight, 9),
    '--scrim': background.join(' '),
  };
}

let choice: Theme = 'system';
let colors: ThemeColors | null = null;

function paint() {
  const root = document.documentElement;
  const light = colors ? isLight(colors.base) : choice === 'light' || (choice === 'system' && !darkMode.matches);
  root.dataset.theme = light ? 'light' : 'dark';
  root.removeAttribute('style');
  if (colors) for (const [name, value] of Object.entries(themeTokens(colors))) root.style.setProperty(name, value);
}

export function showTheme() {
  try {
    choice = localStorage.getItem(KEY) ?? 'system';
    colors = JSON.parse(localStorage.getItem(COLORS_KEY) ?? 'null');
  } catch {
    choice = 'system';
    colors = null;
  }
  paint();
  darkMode.addEventListener('change', paint);
}

/** Tries colours out without saving them, while a theme is being edited. */
export function previewTheme(preview: ThemeColors) {
  colors = preview;
  paint();
}

export function useTheme(theme: Theme, custom: CustomTheme[] = []) {
  choice = theme;
  colors = [...PRESETS, ...custom].find((other) => other.id === theme) ?? null;
  paint();
  try {
    localStorage.setItem(KEY, theme);
    localStorage.setItem(COLORS_KEY, JSON.stringify(colors && { base: colors.base, accent: colors.accent }));
  } catch {
    // The saved setting still applies once it loads.
  }
}

const MINUTE = 60;
const HOUR = 3600;
const DAY = 86400;

export function timeAgo(date: string | null | undefined): string {
  if (!date) return 'never';
  const seconds = (Date.now() - new Date(date).getTime()) / 1000;
  if (seconds < 60) return 'just now';
  if (seconds < HOUR) return plural(Math.floor(seconds / MINUTE), 'minute');
  if (seconds < DAY) return plural(Math.floor(seconds / HOUR), 'hour');
  if (seconds < DAY * 30) return plural(Math.floor(seconds / DAY), 'day');
  return new Date(date).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

function plural(count: number, unit: string): string {
  return `${count} ${unit}${count === 1 ? '' : 's'} ago`;
}

export function playtime(seconds: number): string {
  if (seconds < HOUR) return `${Math.round(seconds / MINUTE)} min played`;
  return `${(seconds / HOUR).toFixed(seconds < 10 * HOUR ? 1 : 0)} hours played`;
}

export function bytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1048576) return `${(size / 1024).toFixed(0)} KB`;
  if (size < 1073741824) return `${(size / 1048576).toFixed(1)} MB`;
  return `${(size / 1073741824).toFixed(2)} GB`;
}

export function count(value: number): string {
  if (value < 1000) return String(value);
  if (value < 1e6) return `${(value / 1000).toFixed(value < 1e4 ? 1 : 0)}K`;
  return `${(value / 1e6).toFixed(value < 1e7 ? 1 : 0)}M`;
}

export function loaderName(kind: string): string {
  const names: Record<string, string> = { vanilla: 'Vanilla', fabric: 'Fabric', quilt: 'Quilt', forge: 'Forge', neoforge: 'NeoForge' };
  return names[kind] ?? kind;
}

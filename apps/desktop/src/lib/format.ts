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
  const names: Record<string, string> = {
    vanilla: 'Vanilla',
    fabric: 'Fabric',
    quilt: 'Quilt',
    forge: 'Forge',
    neoforge: 'NeoForge',
    legacyfabric: 'Legacy Fabric',
    'legacy-fabric': 'Legacy Fabric',
    babric: 'Babric',
    ornithe: 'Ornithe',
  };
  return names[kind] ?? titleCase(kind);
}

export function day(date: string): string {
  return new Date(date).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

export function titleCase(slug: string): string {
  return slug.replace(/[-_]/g, ' ').replace(/^./, (first) => first.toUpperCase());
}

const RELEASE = /^\d+\.\d+(\.\d+)?$/;

function parts(version: string): number[] {
  return version.split('.').map(Number);
}

function newestFirst(a: string, b: string): number {
  const [x, y] = [parts(a), parts(b)];
  return y[0] - x[0] || y[1] - x[1] || (y[2] ?? 0) - (x[2] ?? 0);
}

/** Full releases, newest first. Minecraft moved from 1.21 to year numbers like 26.3. */
export function sortMinecraft(versions: string[]): string[] {
  return versions.filter((version) => RELEASE.test(version)).sort(newestFirst);
}

/** Collapses a long list into something like `1.16 to 1.21, 26.1 to 26.3`. */
export function minecraftRange(versions: string[]): string {
  const releases = sortMinecraft(versions).reverse();
  if (!releases.length) return versions.length ? `${versions.length} snapshots` : 'Unknown';
  const lines = [...new Set(releases.map((version) => parts(version).slice(0, 2).join('.')))];
  if (versions.length <= 2) return releases.join(', ');
  const spans: [string, string][] = [];
  for (const line of lines) {
    const last = spans.at(-1);
    const [major, minor] = parts(line);
    if (last && parts(last[1])[0] === major && parts(last[1])[1] + 1 === minor) last[1] = line;
    else spans.push([line, line]);
  }
  return spans.map(([from, to]) => (from === to ? from : `${from} to ${to}`)).join(', ');
}

/** Modrinth's client and server support, in words. */
export function environment(client?: string | null, server?: string | null): string | null {
  const known = ['required', 'optional', 'unsupported'];
  if (!client || !server || !known.includes(client) || !known.includes(server)) return null;
  if (server === 'unsupported') return client === 'unsupported' ? null : 'Client only';
  if (client === 'unsupported') return 'Server only';
  if (client === 'required' && server === 'required') return 'Client and server';
  if (client === 'required') return 'Client, optional on server';
  if (server === 'required') return 'Server, optional on client';
  return 'Client or server';
}

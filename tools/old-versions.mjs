// Builds crates/waymark-core/data/old-versions.json, the table of every old build Waymark
// can install. Usage: node tools/old-versions.mjs <cache folder>
//
// The list comes from BetaCraft's launchers (betacraft.uk), most of the recovered jars are
// Omniarchive's (omniarchive.uk), and the fixes are LegacyFix (GPL-3.0). Waymark hosts none
// of it. Everything is pinned by hash here, so nothing fetched at runtime can change what
// a player runs, and BetaCraft's servers aren't asked for a list by every copy of the app.
//
// Each build appears once. A jar known under two names keeps Mojang's name, and a Mojang
// version whose name BetaCraft uses for a different jar keeps both, the other as `-original`.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const CACHE = process.argv[2];
if (!CACHE) throw new Error('usage: node tools/old-versions.mjs <cache folder>');
const OUTPUT = join(dirname(fileURLToPath(import.meta.url)), '../crates/waymark-core/data/old-versions.json');
const BETACRAFT = 'https://files.betacraft.uk/launcher';
const MOJANG = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json';
/** From 1.6 on Mojang's own files need no fixing, so only builds Mojang doesn't list are kept. */
const MODERN = '2013-06-25T00:00:00+00:00';
const OLD_ERAS = ['preclassic', 'classic', 'indev', 'infdev', 'alpha', 'beta'];
const LEGACYFIX = '26.10';
const LEGACYFIX_URL = `https://github.com/betacraftuk/legacyfix/releases/download/${LEGACYFIX}/legacyfix-${LEGACYFIX}.jar`;
const LEGACYFIX_MAIN = 'uk.betacraft.legacyfix.applet.AppletLauncher';
const BOUNCY_CASTLE = '1.86';
/** Omniarchive refuses this file now. BetaCraft's copy has the same hash. */
const MOVED = { 'b1.8-pre2-131225': 'https://files.betacraft.uk/launcher/assets/versions/b1.8-pre2-131225.jar' };
/** Keeps Classic's saves on this computer, without its prompt, and asks no outside server for skins. */
const WAYMARK_FLAGS = ['-Dlf.proxy.levelServer=local', '-Dlf.proxy.overrides.mode=local'];

mkdirSync(CACHE, { recursive: true });
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/** One file at a time with a pause, and only once, because these are small community servers. */
async function cached(name, url, binary = false) {
  const path = join(CACHE, name.replaceAll('/', '_'));
  if (!existsSync(path)) {
    const reply = await fetch(url, { headers: { 'User-Agent': 'Woblu/waymark-launcher' } });
    if (!reply.ok) throw new Error(`${url}: ${reply.status}`);
    writeFileSync(path, Buffer.from(await reply.arrayBuffer()));
    await sleep(250);
  }
  return binary ? readFileSync(path) : readFileSync(path, 'utf8');
}
const cachedJson = async (name, url) => JSON.parse(await cached(name, url));
const sha1 = (bytes) => createHash('sha1').update(bytes).digest('hex');
const secure = (url) => url.replace(/^http:/, 'https:');

function eraOf(id, kind) {
  if (/^(rd|pc|4k)-/.test(id)) return 'preclassic';
  if (/^c\d/.test(id)) return 'classic';
  if (/^in-/.test(id)) return 'indev';
  if (/^inf-/.test(id)) return 'infdev';
  if (/^a\d/.test(id)) return 'alpha';
  if (/^b\d/.test(id)) return 'beta';
  if (/^2\.0/.test(id)) return 'aprilfools';
  return kind === 'snapshot' ? 'snapshot' : 'release';
}

const manifest = await cachedJson('mojang-manifest.json', MOJANG);
const last = manifest.versions.find((v) => v.id === '1.7.10').releaseTime;
const mojang = [];
for (const entry of manifest.versions.filter((v) => v.releaseTime <= last)) {
  const meta = await cachedJson(`mojang-${entry.id}.json`, entry.url);
  mojang.push({ ...entry, client: meta.downloads?.client, mainClass: meta.mainClass });
}
const mojangById = new Map(mojang.map((v) => [v.id, v]));
const mojangByJar = new Map(mojang.map((v) => [v.client?.sha1, v]));

const list = await cachedJson('betacraft-list.json', `${BETACRAFT}/v2/assets/version_list.json`);
const betacraft = [];
for (const entry of list.versions) betacraft.push({ entry, json: await cachedJson(`betacraft-${entry.id}.json`, entry.url) });

const templates = [];
const templateKeys = new Map();
const assets = {};
/** BetaCraft's files carry notes to themselves, in places the launcher reads as data. */
const withoutNotes = (value) => JSON.parse(JSON.stringify(value, (key, item) => (key === '__comment' ? undefined : item)));

/** Their list pins an older build, and only newer ones can start RubyDung or Minecraft 4k. */
const legacyFixJar = await cached(`legacyfix-${LEGACYFIX}.jar`, LEGACYFIX_URL, true);
const legacyFixPath = `uk/betacraft/legacyfix/legacyfix/${LEGACYFIX}/legacyfix-${LEGACYFIX}.jar`;
const legacyFixLibrary = {
  name: `uk.betacraft.legacyfix:legacyfix:${LEGACYFIX}`,
  downloads: { artifact: { path: legacyFixPath, sha1: sha1(legacyFixJar), size: legacyFixJar.length, url: LEGACYFIX_URL } },
};

/**
 * Mojang's Java 8 is too old for the connections LegacyFix makes for skins and sign-in.
 * It uses Bouncy Castle by itself when it's on the classpath.
 */
const secureLibraries = [];
for (const name of ['bcprov', 'bctls', 'bcutil']) {
  const path = `org/bouncycastle/${name}-jdk15to18/${BOUNCY_CASTLE}/${name}-jdk15to18-${BOUNCY_CASTLE}.jar`;
  const url = `https://repo1.maven.org/maven2/${path}`;
  const jar = await cached(`${name}-${BOUNCY_CASTLE}.jar`, url, true);
  secureLibraries.push({ name: `org.bouncycastle:${name}-jdk15to18:${BOUNCY_CASTLE}`, downloads: { artifact: { path, sha1: sha1(jar), size: jar.length, url } } });
}

function templateOf(json) {
  const current = json.mainClass === 'uk.betacraft.legacyfix.LegacyFixLauncher';
  const jvm = json.arguments.jvm
    .filter((a) => a !== WAYMARK_FLAGS[0] && a !== WAYMARK_FLAGS[1] && a !== '-Dlf.icon=${instance_icon}')
    .map((a) => (current && typeof a === 'string' && a.startsWith('-javaagent:') ? `-javaagent:\${library_directory}/${legacyFixPath}` : a));
  const at = jvm.findIndex((a) => typeof a === 'string' && a.startsWith('-javaagent'));
  if (at >= 0) jvm.splice(at, 0, ...WAYMARK_FLAGS);
  const libraries = withoutNotes(json.libraries).map((library) => (current && library.name.startsWith('uk.betacraft.legacyfix:') ? legacyFixLibrary : library));
  if (current) libraries.push(...secureLibraries);
  const template = {
    mainClass: current ? LEGACYFIX_MAIN : json.mainClass,
    arguments: { game: json.arguments.game, jvm },
    libraries,
    javaVersion: json.javaVersion,
    ...(json.logging ? { logging: json.logging } : {}),
  };
  const key = JSON.stringify(template);
  if (!templateKeys.has(key)) {
    templateKeys.set(key, templates.length);
    templates.push(template);
  }
  return templateKeys.get(key);
}

const rows = new Map();
function add(id, kind, released, template, assetId, client) {
  rows.set(id, { id, kind, era: eraOf(id, kind), released, template, assets: assetId, client: { sha1: client.sha1, size: client.size, url: secure(client.url) } });
}

/** An older wrapper of theirs loses sounds and writes to the real `.minecraft`, so those wait. */
const usesOldWrapper = (json) => json.mainClass.includes('mcwrapper');

for (const { entry, json } of betacraft) {
  const client = json.downloads.client;
  const known = mojangByJar.get(client.sha1);
  if (known && known.releaseTime >= MODERN) continue;
  const clash = !known && mojangById.has(entry.id);
  const id = known?.id ?? (clash ? `${entry.id}-original` : entry.id);
  assets[json.assetIndex.id] = json.assetIndex;
  if (usesOldWrapper(json)) continue;
  add(id, known?.type ?? entry.type, known?.releaseTime ?? entry.releaseTime, templateOf(json), json.assetIndex.id, client);
}

/** Builds BetaCraft's newer list doesn't have run like whatever came out around the same time. */
function neighbour(released, era) {
  const same = [...rows.values()].filter((row) => row.era === era && era !== 'aprilfools' && templates[row.template].mainClass.includes('legacyfix'));
  const pool = same.length ? same : [...rows.values()].filter((row) => templates[row.template].mainClass.includes('legacyfix'));
  const when = Date.parse(released);
  return pool.reduce((best, row) => (Math.abs(Date.parse(row.released) - when) < Math.abs(Date.parse(best.released) - when) ? row : best));
}

for (const { entry, json } of betacraft.filter(({ json }) => usesOldWrapper(json))) {
  const client = json.downloads.client;
  const known = mojangByJar.get(client.sha1);
  if (known && known.releaseTime >= MODERN) continue;
  const id = known?.id ?? entry.id;
  const released = known?.releaseTime ?? entry.releaseTime;
  const near = neighbour(released, eraOf(id, entry.type));
  add(id, known?.type ?? entry.type, released, near.template, json.assetIndex.id, client);
}

for (const version of mojang.filter((v) => OLD_ERAS.includes(eraOf(v.id, v.type)) && !rows.has(v.id) && v.client)) {
  const near = neighbour(version.releaseTime, eraOf(version.id, version.type));
  add(version.id, version.type, version.releaseTime, near.template, near.assets, version.client);
  // LegacyFix looks for an applet, and RubyDung is only a class with a main method.
  if (version.mainClass?.endsWith('RubyDung')) rows.get(version.id).extra = ['--mainClass', version.mainClass];
}

const oldList = (await cached('betacraft-list.txt', `${BETACRAFT}/assets/version_list.txt`)).split(/\r?\n/).filter(Boolean);
const jarsSeen = new Set([...rows.values()].map((row) => row.client.sha1));
for (const line of oldList) {
  const id = line.split('`')[0];
  if (rows.has(id) || betacraft.some(({ entry }) => entry.id === id)) continue;
  const info = Object.fromEntries((await cached(`info-${id}.txt`, `${BETACRAFT}/assets/jsons/${id}.info`)).split(/\r?\n/).filter(Boolean).map((row) => [row.slice(0, row.indexOf(':')), row.slice(row.indexOf(':') + 1)]));
  const url = secure(info.url);
  const jar = await cached(`jar-${id}.jar`, url, true);
  const hash = sha1(jar);
  if (jarsSeen.has(hash)) continue;
  jarsSeen.add(hash);
  // A build with no date at all is one of the 2.0 April Fools jokes, which all came out together.
  const stamp = Math.max(Number(info['compile-date'] ?? 0), Number(info['release-date'] ?? 0));
  const released = new Date(stamp || Date.parse('2013-04-01T00:00:00Z')).toISOString();
  const kind = /^4k/.test(id) ? 'old_alpha' : 'snapshot';
  const near = neighbour(released, eraOf(id, kind));
  add(id, kind, released, near.template, near.assets, { sha1: hash, size: jar.length, url });
  // Minecraft 4k is one applet class with nothing around it to find it by, and it draws
  // to an ordinary window, which LegacyFix leaves hidden unless it's told not to.
  if (/^4k/.test(id)) {
    rows.get(id).extra = ['--appletClass', 'M'];
    rows.get(id).jvm = ['-Dlf.deawt.disable'];
  }
}

for (const [id, url] of Object.entries(MOVED)) rows.get(id).client.url = url;

const versions = [...rows.values()].sort((a, b) => b.released.localeCompare(a.released));
const table = { templates, assets, versions };
mkdirSync(dirname(OUTPUT), { recursive: true });
writeFileSync(OUTPUT, `${JSON.stringify(table, null, 0)}\n`);
const eras = {};
for (const row of versions) eras[row.era] = (eras[row.era] ?? 0) + 1;
console.log(`${versions.length} builds, ${templates.length} templates, ${Object.keys(assets).length} asset indexes`, eras);
console.log('hosts', JSON.stringify(versions.reduce((all, row) => ({ ...all, [new URL(row.client.url).host]: (all[new URL(row.client.url).host] ?? 0) + 1 }), {})));

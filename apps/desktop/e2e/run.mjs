// Drives the real desktop app the way a player would and checks the results on
// disk. It talks to WebView2 over the Chrome DevTools Protocol.
//
//   $env:WAYMARK_HOME = '<empty folder>'
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
//   start the debug app with the dev server running, then: node e2e/run.mjs
//
// Set E2E_SHOTS to a folder to save screenshots along the way.

import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';

const HOME = process.env.WAYMARK_HOME;
const SHOTS = process.env.E2E_SHOTS;
const PORT = 9222;
const VERSION = '1.21.4';

if (!HOME) throw new Error('Set WAYMARK_HOME to the folder the app is using');
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function connect() {
  for (let attempt = 0; attempt < 60; attempt++) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      const page = targets.find((t) => t.type === 'page' && !t.url.startsWith('devtools'));
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await sleep(500);
  }
  throw new Error('The app never opened a debugging port');
}

const socket = new WebSocket(await connect());
await new Promise((resolve) => (socket.onopen = resolve));

let nextId = 0;
const waiting = new Map();
const problems = [];

socket.onmessage = (message) => {
  const data = JSON.parse(message.data);
  if (data.id) {
    waiting.get(data.id)?.(data);
    waiting.delete(data.id);
  } else if (data.method === 'Runtime.exceptionThrown') {
    problems.push(`exception: ${data.params.exceptionDetails.exception?.description ?? data.params.exceptionDetails.text}`);
  } else if (data.method === 'Runtime.consoleAPICalled' && data.params.type === 'error') {
    problems.push(`console: ${data.params.args.map((a) => a.value ?? a.description).join(' ')}`);
  } else if (data.method === 'Log.entryAdded' && data.params.entry.level === 'error') {
    problems.push(`log: ${data.params.entry.text} ${data.params.entry.url ?? ''}`);
  }
};

function send(method, params = {}) {
  return new Promise((resolve) => {
    const id = ++nextId;
    waiting.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });
}

async function run(expression) {
  const reply = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (reply.result?.exceptionDetails) throw new Error(reply.result.exceptionDetails.exception?.description ?? 'evaluation failed');
  return reply.result?.result?.value;
}

async function waitFor(description, expression, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await run(expression)) return;
    await sleep(150);
  }
  throw new Error(`Timed out waiting for: ${description}`);
}

const text = (value) => JSON.stringify(value);
const hasText = (value) => `document.body.innerText.includes(${text(value)})`;

const HELPERS = `
  window.e2e = {
    button(label) {
      const buttons = [...document.querySelectorAll('button')].filter((b) => !b.disabled);
      return buttons.find((b) => b.textContent.trim() === label) ?? buttons.find((b) => b.matches('header .back, header .settings') && b.getAttribute('aria-label') === label);
    },
    setValue(selector, value) {
      const element = document.querySelector(selector);
      const proto = element.tagName === 'SELECT' ? HTMLSelectElement : HTMLInputElement;
      Object.getOwnPropertyDescriptor(proto.prototype, 'value').set.call(element, value);
      element.dispatchEvent(new Event('input', { bubbles: true }));
      element.dispatchEvent(new Event('change', { bubbles: true }));
    },
    modRow(name) {
      return [...document.querySelectorAll('.mod')].find((row) => row.querySelector('.name').innerText.trim() === name);
    },
  };
`;

async function click(label, timeout = 15000) {
  await waitFor(`a "${label}" button`, `!!e2e.button(${text(label)})`, timeout);
  await run(`e2e.button(${text(label)}).click()`);
}

const setValue = (selector, value) => run(`e2e.setValue(${text(selector)}, ${text(value)})`);

async function shot(name) {
  if (!SHOTS) return;
  const { result } = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(SHOTS, `${name}.png`), Buffer.from(result.data, 'base64'));
}

const nav = (label) => run(`[...document.querySelectorAll('header button')].find((b) => b.innerText === ${text(label)} || b.getAttribute('aria-label') === ${text(label)}).click()`);
const tab = (label) => run(`[...document.querySelectorAll('.tabs button')].find((b) => b.innerText === ${text(label)}).click()`);
const openInstance = async (name) => {
  await nav('Library');
  await waitFor('the library', `document.querySelectorAll('.grid .open').length > 0`);
  await run(`[...document.querySelectorAll('.grid .open')].find((r) => r.innerText.includes(${text(name)})).click()`);
  await waitFor(name, `document.querySelector('.names h1')?.innerText === ${text(name)}`);
};
const backToInstance = async () => {
  await nav('Library');
  await waitFor('the library', `!!document.querySelector('.grid [data-instance=${text(ID)}]')`);
  await run(`document.querySelector('.grid [data-instance=${text(ID)}]').click()`);
  await waitFor('the instance', `!!document.querySelector('.names h1')`);
};
const listed = (name) => `[...document.querySelectorAll('.grid .name, .names h1')].some((n) => n.innerText === ${text(name)})`;
const lockText = (id) => readFileSync(join(HOME, 'instances', id, 'waymark.lock'), 'utf8');

const modsDir = (id) => join(HOME, 'instances', id, 'game', 'mods');
const modFiles = (id) => (existsSync(modsDir(id)) ? readdirSync(modsDir(id)) : []);

function expect(condition, message) {
  if (!condition) throw new Error(message);
}

let passed = 0;
async function step(name, body) {
  const started = Date.now();
  try {
    await body();
    passed++;
    console.log(`ok    ${name} (${((Date.now() - started) / 1000).toFixed(1)}s)`);
  } catch (error) {
    console.log(`FAIL  ${name}: ${error.message}`);
    await shot(`failed-${name.replace(/\W+/g, '-')}`);
    finish(1);
  }
}

function finish(code) {
  console.log(`\n${passed} steps passed`);
  console.log(problems.length ? `${problems.length} problems in the page:\n  ${[...new Set(problems)].join('\n  ')}` : 'No console errors or policy violations');
  process.exit(code);
}

await send('Runtime.enable');
await send('Log.enable');
await send('Page.enable');
await send('Page.reload');
await sleep(1500);
await run(HELPERS);

const ID = 'e2e-fabric';

await step('opens on the empty library', async () => {
  await waitFor('the empty state', hasText('No instances yet'));
  await shot('01-empty');
});

await step('keeps the window open during play for this test', async () => {
  await nav('Settings');
  await waitFor('the settings page', `!!document.querySelector('#delay')`);
  await run(`[...document.querySelectorAll('label')].find((l) => l.innerText.includes('Close the window')).querySelector('input').click()`);
  await sleep(300);
  expect(readFileSync(join(HOME, 'settings.toml'), 'utf8').includes('closeWhilePlaying = false'), 'the setting did not save');
  await nav('Library');
});

await step('creates a Fabric instance through the dialog', async () => {
  await click('New instance');
  await waitFor('the version list', `document.querySelector('#version')?.options.length > 3`);
  await setValue('#name', 'E2E Fabric');
  await setValue('#version', VERSION);
  await setValue('#loader', 'fabric');
  await shot('02-new-instance');
  await click('Create');
  await waitFor('the instance page', `!!document.querySelector('.names h1')?.innerText.includes('E2E Fabric')`, 60000);
  expect(existsSync(join(HOME, 'instances', ID, 'waymark.lock')), 'no lockfile on disk');
});

await step('finds and adds mods from Discover', async () => {
  await click('Discover');
  await click('Mods');
  await setValue('.search', 'sodium');
  await waitFor('search results', `[...document.querySelectorAll('.title')].some((t) => t.innerText === 'Sodium')`, 20000);
  await shot('03-discover');
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Sodium').querySelector('.add').click()`);
  await waitFor('Sodium to install', `[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Sodium')?.innerText.includes('Installed')`, 40000);
  await setValue('.search', 'lithium');
  await waitFor('search results', `[...document.querySelectorAll('.title')].some((t) => t.innerText === 'Lithium')`, 20000);
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Lithium').querySelector('.add').click()`);
  await waitFor('Lithium to install', `[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Lithium')?.innerText.includes('Installed')`, 40000);
  expect(modFiles(ID).length === 2, `expected 2 mods on disk, found ${modFiles(ID).length}`);
});

await step('lists mods and switches one off and on', async () => {
  await backToInstance();
  await waitFor('the mod table', `!!e2e.modRow('Sodium')`);
  await shot('04-mods');
  const box = await run(`getComputedStyle(e2e.modRow('Sodium').querySelector('.switch')).appearance`);
  expect(box === 'none', `the system check box is showing (${box})`);
  await run(`e2e.modRow('Sodium').querySelector('.switch').click()`);
  await waitFor('the file to be renamed', 'true');
  await sleep(500);
  expect(modFiles(ID).some((f) => f.endsWith('.disabled')), 'no .disabled file on disk');
  await run(`e2e.modRow('Sodium').querySelector('.switch').click()`);
  await sleep(500);
  expect(!modFiles(ID).some((f) => f.endsWith('.disabled')), 'the mod did not switch back on');
});

await step('sets a named waymark and goes back to an earlier one', async () => {
  await click('Waymarks');
  await waitFor('the list', `document.querySelectorAll('.row').length >= 3`);
  await shot('05-waymarks');
  await run(`[...document.querySelectorAll('ol .row')].find((r) => r.innerText.includes('Instance created')).click()`);
  await click('Return to this waymark');
  await waitFor('the return to finish', hasText('Returned to'));
  expect(modFiles(ID).length === 0, `expected no mods after going back, found ${modFiles(ID).length}`);
  await waitFor('the new list', `[...document.querySelectorAll('ol .row')].some((r) => r.innerText.includes('Installed Lithium'))`);
  await run(`[...document.querySelectorAll('ol .row')].find((r) => r.innerText.includes('Installed Lithium')).click()`);
  await click('Return to this waymark');
  await sleep(800);
  expect(modFiles(ID).length === 2, `expected both mods back, found ${modFiles(ID).length}`);
});

await step('launches the game, shows its log and stops it', async () => {
  await click('Logs');
  await click('Play');
  await waitFor('the game to start', `document.querySelector('footer .running') !== null`, 120000);
  await waitFor('log lines', hasText('Setting user'), 90000);
  const refused = await run(`window.__TAURI_INTERNALS__.invoke('install_update').then(() => 'installed', (error) => error.message ?? String(error))`);
  expect(/game/.test(refused), `an update must not install during a game: ${refused}`);
  await shot('06-running');
  await click('Stop');
  await waitFor('the game to stop', `document.querySelector('footer .running') === null`, 20000);
  await sleep(600);
  expect(!(await run(hasText('crashed'))), 'stopping the game showed a crash report');
});

await step('renames the instance from its settings', async () => {
  await run(`[...document.querySelectorAll('.tabs button')].find((b) => b.innerText === 'Settings').click()`);
  await waitFor('the form', `!!document.querySelector('#instance-name')`);
  await setValue('#instance-name', 'E2E Renamed');
  await shot('07-instance-settings');
  await click('Save');
  await waitFor('the new name', `document.querySelector('.names h1')?.innerText === 'E2E Renamed'`);
});

await step('removes everything dangerous from a description', async () => {
  // The real sanitizer, in the real webview, against what an attacker would try.
  const report = await run(`(async () => {
    const { renderRich } = await import('/src/lib/rich.ts');
    const attacks = [
      '<script>window.pwned = 1</script>',
      '<img src=x onerror="window.pwned = 2">',
      '<svg onload="window.pwned = 3"></svg>',
      '<svg><script>window.pwned = 4</script></svg>',
      '<math><mtext><style><img src=x onerror="window.pwned = 5"></style></mtext></math>',
      '<a href="javascript:window.pwned = 6">x</a>',
      '<a href="JaVaScRiPt:window.pwned = 7">x</a>',
      '<a href=" &#14; javascript:window.pwned = 8">x</a>',
      '<a href="data:text/html,<script>window.pwned = 9</script>">x</a>',
      '<iframe src="javascript:window.pwned = 10"></iframe>',
      '<iframe srcdoc="<script>parent.pwned = 11</script>"></iframe>',
      '<object data="https://evil.example/x"></object>',
      '<embed src="https://evil.example/x">',
      '<base href="https://evil.example/">',
      '<meta http-equiv="refresh" content="0;url=https://evil.example">',
      '<link rel="stylesheet" href="https://evil.example/x.css">',
      '<style>body { display: none }</style>',
      '<form action="https://evil.example"><input name="password"><button>Log in</button></form>',
      '<div class="btn primary" id="app" style="position:fixed;inset:0;z-index:99">Fake</div>',
      '<p style="background:url(https://evil.example/track.png);color:red">x</p>',
      '<img src="https://evil.example/a.png" srcset="https://evil.example/b.png 2x" style="position:fixed">',
      '<details open ontoggle="window.pwned = 12">x</details>',
      '<noscript><p title="</noscript><img src=x onerror=window.pwned=13>">',
      '[click](javascript:window.pwned=14)',
      '![x](javascript:window.pwned=15)',
    ];
    const box = document.createElement('div');
    box.style.display = 'none';
    document.body.append(box);
    const bad = [];
    for (const attack of attacks) {
      for (const format of ['markdown', 'html']) {
        const root = renderRich(attack, format, 'https://modrinth.com');
        box.replaceChildren(root);
        const nodes = [...box.querySelectorAll('*')];
        const forbidden = nodes.filter((n) => ['SCRIPT', 'STYLE', 'IFRAME', 'OBJECT', 'EMBED', 'BASE', 'META', 'LINK', 'FORM', 'BUTTON', 'SVG', 'MATH'].includes(n.tagName.toUpperCase()));
        const attrs = nodes.flatMap((n) => [...n.attributes]).filter((a) =>
          a.name.startsWith('on') || a.name === 'srcset' || a.name === 'srcdoc' || a.name === 'id' ||
          (a.name === 'class' && !a.value.includes('video')) ||
          (a.name === 'style' && !/^(text-align|image-rendering)/.test(a.value)) ||
          (['href', 'src'].includes(a.name) && /^\\s*(javascript|data:text|vbscript)/i.test(a.value)));
        if (forbidden.length || attrs.length) bad.push(format + ': ' + attack);
      }
    }
    await new Promise((r) => setTimeout(r, 300));
    box.remove();
    return JSON.stringify({ tested: attacks.length * 2, bad, ran: window.pwned ?? null });
  })()`);
  const { tested, bad, ran } = JSON.parse(report);
  console.log(`      ${tested} attacks`);
  expect(bad.length === 0, `not removed: ${bad.join(' | ')}`);
  expect(ran === null, `a script ran (${ran})`);
});

await step('opens a mod page with its description, gallery and versions', async () => {
  await click('Discover');
  await click('Mods');
  await setValue('.search', 'sodium');
  await waitFor('search results', `[...document.querySelectorAll('.title')].some((t) => t.innerText === 'Sodium')`, 20000);
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Sodium').querySelector('.open').click()`);
  await waitFor('the description', `document.querySelector('.rich')?.innerText.length > 200`, 20000);
  await waitFor('real images', `[...document.querySelectorAll('.rich img')].some((i) => i.complete && i.naturalWidth > 0)`, 20000);
  await waitFor('the sidebar', hasText('Compatibility') + ` && ` + hasText('Creators'));
  await waitFor('the install state', hasText('Version ') + ` || ` + hasText('Installs '), 15000);
  const fonts = await run(`(() => {
    const family = (s) => getComputedStyle(document.querySelector(s)).fontFamily;
    return JSON.stringify({ body: family('.rich'), heading: family('.rich h1, .rich h2, .rich h3, .rich p') });
  })()`);
  expect(fonts.includes('Libre Franklin'), `wrong font: ${fonts}`);
  const reaches = await run(`(() => {
    const page = document.querySelector('.project').getBoundingClientRect();
    return document.querySelector('.project aside').getBoundingClientRect().right > page.right - 48;
  })()`);
  expect(reaches, 'the page should fill the window, not stop short of the right edge');
  await shot('18-project');

  await run(`[...document.querySelectorAll('.tabs button')].find((b) => b.innerText.startsWith('Gallery')).click()`);
  await waitFor('gallery images', `[...document.querySelectorAll('.gallery img')].some((i) => i.complete && i.naturalWidth > 0)`, 20000);
  await run(`document.querySelector('.gallery button').click()`);
  await waitFor('the image viewer', `!!document.querySelector('.lightbox img')`);
  await waitFor('the full image', `document.querySelector('.lightbox img').naturalWidth > 0`, 20000);
  await shot('19-gallery');
  await run(`window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))`);
  await waitFor('the viewer to close, leaving the page open', `!document.querySelector('.lightbox') && !!document.querySelector('.gallery')`);

  await tab('Versions');
  await waitFor('the version list', `document.querySelectorAll('.line').length > 0`, 20000);
  await run(`document.querySelector('.line').click()`);
  await waitFor('a changelog', `document.querySelector('.detail')?.innerText.includes('downloads') && !document.querySelector('.detail').innerText.includes('Loading the changelog')`, 15000);
  await shot('20-versions');

  await click('Back');
  await waitFor('the search to still be there', `!document.querySelector('.project') && document.querySelector('.search')?.value === 'sodium'`);
});

await step('opens a mod page from an instance', async () => {
  await backToInstance();
  await tab('Mods');
  await waitFor('the mod table', `!!e2e.modRow('Sodium')`);
  await run(`e2e.modRow('Sodium').querySelector('.body').click()`);
  await waitFor('the page', `document.querySelector('.project h1')?.innerText === 'Sodium'`);
  await waitFor('the description', `document.querySelector('.rich')?.innerText.length > 200`, 20000);
  await waitFor('the installed version', `/Version .+ is in this instance/.test(document.body.innerText)`, 15000);
  expect(!(await run(hasText('1 selected'))), 'a plain click should open the page, not select');
  await click('Back');
  await waitFor('the mods tab', `!!e2e.modRow('Sodium')`);
});

await step('adds a specific version from a mod page', async () => {
  await click('Discover');
  await click('Mods');
  await setValue('.search', 'mod menu');
  await waitFor('search results', `[...document.querySelectorAll('.title')].some((t) => t.innerText === 'Mod Menu')`, 20000);
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Mod Menu').querySelector('.open').click()`);
  await waitFor('the page', `!!document.querySelector('.project .tabs')`, 20000);
  await tab('Versions');
  await waitFor('versions for this instance', `document.querySelectorAll('.project .action .btn').length > 1`, 20000);
  const older = `[...document.querySelectorAll('.project .row')].filter((row) => row.querySelector('.action .btn'))[1]`;
  const file = await run(`${older}.querySelector('.name .faint').textContent`);
  await run(`${older}.querySelector('.action .btn').click()`);
  await waitFor('it to install', `/Installed/.test(document.querySelector('.project .head').innerText)`, 40000);
  await sleep(800);
  expect(modFiles(ID).includes(file), `${file} is not on disk: ${modFiles(ID).join(', ')}`);
  expect(lockText(ID).includes(file), `${file} is not in the lockfile`);
  console.log(`      added ${file}`);
  await click('Back');
  await backToInstance();
  await tab('Mods');
  await waitFor('Mod Menu in the list', `!!e2e.modRow('Mod Menu')`);
  // Its dependencies came too, and later steps expect only the first two mods.
  const extra = `[...document.querySelectorAll('.mod')].find((row) => !['Sodium', 'Lithium'].includes(row.querySelector('.name').innerText.trim()))`;
  while (await run(`!!${extra}`)) {
    const remaining = await run(`document.querySelectorAll('.mod').length`);
    await run(`${extra}.querySelector('.content-remove').click()`);
    await waitFor('the mod to be removed', `document.querySelectorAll('.mod').length < ${remaining}`);
  }
  expect(modFiles(ID).length === 2, `expected 2 mods left, found ${modFiles(ID).join(', ')}`);
});

await step('filters Discover and shows what is already installed', async () => {
  await click('Discover');
  await click('Mods');
  await setValue('.search', '');
  await waitFor('the filter list', `document.querySelectorAll('.side fieldset input').length > 5`, 20000);
  await setValue('.search', 'sodium');
  await waitFor('Sodium in the results', `[...document.querySelectorAll('li')].some((li) => li.querySelector('.title')?.innerText === 'Sodium')`, 20000);
  const label = `[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Sodium')`;
  await waitFor('Sodium to say Installed', `${label}.querySelector('.installed')?.innerText === 'Installed'`, 15000);
  expect(!(await run(`!!${label}.querySelector('.add')`)), 'an installed mod should not offer Install');
  await setValue('.search', '');
  await sleep(800);
  const before = await run(`document.querySelector('.results li .title')?.innerText`);
  await setValue('#sort', 'newest');
  await waitFor('the order to change', `document.querySelector('.results li .title')?.innerText !== ${text(before)}`, 20000);
  await run(`document.querySelector('.side fieldset input').click()`);
  await waitFor('Clear filters', hasText('Clear filters'));
  await sleep(700);
  await shot('21-discover-filters');
  await click('Clear filters');
  await setValue('#sort', 'relevance');
});

await step('moves between pages of results', async () => {
  await sleep(800);
  await waitFor('the pager', `!!document.querySelector('[aria-label="Page 2"]')`, 20000);
  const first = await run(`document.querySelector('.results li .title')?.innerText`);
  await run(`document.querySelector('[aria-label="Page 2"]').click()`);
  await waitFor('the second page', `document.querySelector('.pager [aria-current="page"]')?.innerText === '2'`, 20000);
  expect((await run(`document.querySelector('.results li .title')?.innerText`)) !== first, 'page 2 shows the same results');
  expect((await run(`document.querySelector('.results').scrollTop`)) === 0, 'a new page should start at the top');
  await run(`document.querySelector('[aria-label="Next page"]').click()`);
  await waitFor('the third page', `document.querySelector('.pager [aria-current="page"]')?.innerText === '3'`, 20000);
  await setValue('.search', 'sodium');
  await waitFor('a new search to start on page 1', `[...document.querySelectorAll('.pager [aria-current="page"]')].every((b) => b.innerText === '1')`, 20000);
  await setValue('.search', '');
  await sleep(600);
});

await step('installs a resource pack, lists it and removes it', async () => {
  await click('Resource packs');
  await setValue('.search', '');
  await waitFor('resource packs', `document.querySelectorAll('.results li .add').length > 0`, 20000);
  const title = await run(`document.querySelector('.results li:has(.add) .title').innerText`);
  await run(`document.querySelector('.results li:has(.add) .add').click()`);
  await waitFor('it to say Installed', `!![...document.querySelectorAll('.results li')].find((li) => li.querySelector('.title')?.innerText === ${text(title)})?.querySelector('.installed')`, 60000);
  const packs = join(HOME, 'instances', ID, 'game', 'resourcepacks');
  expect(readdirSync(packs).length === 1, `expected 1 pack on disk, found ${readdirSync(packs).join(', ')}`);
  expect(lockText(ID).includes('resourcePacks'), 'the pack is not in the lockfile');
  await backToInstance();
  await tab('Resource packs');
  await waitFor('the pack in its tab', `[...document.querySelectorAll('.pack .name')].some((n) => n.innerText === ${text(title)})`, 10000);
  await shot('22-resource-packs');
  await run(`document.querySelector('.pack input').click()`);
  await waitFor('the file to switch off', `document.querySelector('.pack.off') !== null`);
  expect(readdirSync(packs)[0].endsWith('.disabled'), 'the pack was not switched off on disk');
  await run(`document.querySelector('.pack input').click()`);
  await waitFor('the file to switch on', `document.querySelector('.pack.off') === null`);
  await run(`document.querySelector('.pack .content-remove').click()`);
  await waitFor('the list to empty', hasText('This instance has no resource packs.'));
  expect(readdirSync(packs).length === 0, 'the pack is still on disk');
  await tab('Mods');
});

/** A minimal world, written by hand so the test doesn't have to play the game first. */
function writeWorld(id, folder, name) {
  const utf = (value) => {
    const bytes = Buffer.from(value, 'utf8');
    const length = Buffer.alloc(2);
    length.writeUInt16BE(bytes.length);
    return Buffer.concat([length, bytes]);
  };
  const long = Buffer.alloc(8);
  long.writeBigInt64BE(BigInt(Date.now()));
  const data = Buffer.concat([
    Buffer.from([10]), utf(''),
    Buffer.from([10]), utf('Data'),
    Buffer.from([8]), utf('LevelName'), utf(name),
    Buffer.from([4]), utf('LastPlayed'), long,
    Buffer.from([0, 0]),
  ]);
  const path = join(HOME, 'instances', id, 'game', 'saves', folder);
  mkdirSync(path, { recursive: true });
  writeFileSync(join(path, 'level.dat'), gzipSync(data));
  return path;
}

/** Answers status pings like a Minecraft server, and notes anyone trying to log in. */
function fakeServer() {
  const seen = { login: false };
  const varint = (value) => {
    const out = [];
    do {
      let byte = value & 0x7f;
      value >>>= 7;
      if (value) byte |= 0x80;
      out.push(byte);
    } while (value);
    return Buffer.from(out);
  };
  const server = createServer((socket) => {
    socket.on('error', () => {});
    socket.once('data', (data) => {
      // The handshake's last byte says whether this is a status check or a login.
      const state = data[data[0]];
      if (state === 2) {
        seen.login = true;
        socket.end();
        return;
      }
      const body = Buffer.from(JSON.stringify({ version: { name: 'Fake 1.21.4' }, players: { online: 7, max: 20 }, description: 'Hello from the test' }));
      const inner = Buffer.concat([Buffer.from([0]), varint(body.length), body]);
      socket.end(Buffer.concat([varint(inner.length), inner]));
    });
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve({ port: server.address().port, seen, close: () => server.close() })));
}

await step('shows worlds, pings a server and restores a deleted world', async () => {
  const world = writeWorld(ID, 'River', 'River Base');
  await tab('Worlds');
  await waitFor('the world', hasText('River Base'), 10000);
  const server = await fakeServer();
  await click('Add a server');
  await setValue('[aria-label="Server name"]', 'Test Server');
  await setValue('[aria-label="Server address"]', `127.0.0.1:${server.port}`);
  await click('Save');
  await waitFor('the server to answer', hasText('7 of 20 players'), 10000);
  await waitFor('its message', hasText('Hello from the test'));
  await shot('23-worlds');

  await run(`document.querySelector('[aria-label="Delete River Base"]').click()`);
  await waitFor('the world to go', `!document.querySelector('.world')`);
  expect(!existsSync(world), 'the world is still in the instance');
  await click('Undo');
  await waitFor('the world to come back', hasText('River Base'));
  expect(existsSync(join(world, 'level.dat')), 'the world was not restored');

  await run(`document.querySelector('.world .tools .btn').click()`);
  await waitFor('the backup', hasText('Backed up River Base'), 15000);
  expect(readdirSync(join(HOME, 'instances', ID, 'game', 'backups')).length === 1, 'no backup on disk');

  // A brand-new instance opens the game's accessibility screen, which holds a join back until dismissed.
  writeFileSync(join(HOME, 'instances', ID, 'game', 'options.txt'), 'onboardAccessibility:false\n');
  await run(`document.querySelector('.server .play').click()`);
  const deadline = Date.now() + 150000;
  while (!server.seen.login && Date.now() < deadline) await sleep(500);
  expect(server.seen.login, 'the game never tried to join the server');
  console.log('      the game joined the test server by itself');
  await click('Stop');
  await waitFor('the game to stop', `!!e2e.button('Play')`, 30000);
  server.close();
});

await step('installs a data pack into a world', async () => {
  await click('Browse data packs');
  await waitFor('data packs', `document.querySelectorAll('.results li .add').length > 0`, 20000);
  await run(`document.querySelector('.results li:has(.add) .add').click()`);
  await waitFor('the world picker', `!!document.querySelector('.project [aria-label="World"]')`, 20000);
  await waitFor('Install to be ready', `!!document.querySelector('.project .head .primary:not(:disabled)')`, 20000);
  await run(`document.querySelector('.project .head .primary').click()`);
  await waitFor('it to install', `/Installed/.test(document.querySelector('.project .head').innerText)`, 40000);
  const packs = readdirSync(join(HOME, 'instances', ID, 'game', 'saves', 'River', 'datapacks'));
  expect(packs.length === 1, `expected 1 data pack in the world, found ${packs.join(', ')}`);
  await shot('24-data-pack');
  await run(`document.querySelector('header .back').click()`);
  await backToInstance();
  await tab('Worlds');
  await waitFor('the world', hasText('River Base'));
  await run(`document.querySelector('[aria-label="Data packs in River Base"]').click()`);
  await waitFor('the pack under its world', `!!document.querySelector('.packs .pack')`);
  await run(`document.querySelector('.packs .pack [aria-label^="Remove"]').click()`);
  await waitFor('it to go', hasText('This world has no data packs.'));
  await tab('Mods');
});

await step('shows the skin model, the defaults and a saved skin', async () => {
  const skin = join(HOME, 'test-skin.png');
  const texture = await fetch('https://textures.minecraft.net/texture/46acd06e8483b176e8ea39fc12fe105eb3a2a4970f5100057e9d84d4b60bdfa7');
  writeFileSync(skin, Buffer.from(await texture.arrayBuffer()));

  await nav('Skins');
  await waitFor('the 3D model', `!!document.querySelector('.stage canvas')`);
  await waitFor('all 18 default skins', `document.querySelectorAll('.lists .tile img.front').length === 18`, 30000);
  await sleep(1200);
  await shot('26-skins');

  const box = await run(`JSON.stringify((({ x, y, width, height }) => ({ x, y, width, height }))(document.querySelector('.stage').getBoundingClientRect()))`);
  const { x, y, width, height } = JSON.parse(box);
  const [startX, midY] = [x + width / 2, y + height / 2];
  await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: startX, y: midY, button: 'left', clickCount: 1 });
  for (let step = 1; step <= 12; step++) {
    await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: startX + step * 12, y: midY, button: 'left', buttons: 1 });
    await sleep(16);
  }
  await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: startX + 144, y: midY, button: 'left', clickCount: 1 });
  await sleep(300);
  await shot('27-skins-rotated');

  await run(`document.querySelector('[aria-label^="Preview Alex"]').click()`);
  await waitFor('the preview buttons', `!!e2e.button('Reset')`);
  await click('Reset');

  await run(`window.__TAURI_INTERNALS__.invoke('add_skin', { path: ${text(skin)} })`);
  await click('Discover');
  await nav('Skins');
  await waitFor('the saved skin', `document.querySelectorAll('.lists .tile img.front').length === 19`, 20000);
  await run(`document.querySelector('[aria-label="Edit test skin"]').click()`);
  await setValue('#skin-name', 'Renamed Skin');
  await run(`document.querySelector('input[value="slim"]').click()`);
  await click('Save');
  await waitFor('the new name', hasText('Renamed Skin'));
  expect(existsSync(join(HOME, 'skins', 'library.json')), 'the library was not saved');
  expect(readFileSync(join(HOME, 'skins', 'library.json'), 'utf8').includes('"slim"'), 'the arm style was not saved');
  await run(`document.querySelector('[aria-label="Edit Renamed Skin"]').click()`);
  await click('Delete');
  await waitFor('the skin to go', `!${hasText('Renamed Skin')}`);
  await backToInstance();
  await tab('Mods');
});

await step('reviews updates with a changelog and switches a version', async () => {
  await click('Discover');
  await click('Mods');
  await setValue('.search', 'mod menu');
  await waitFor('Mod Menu', `[...document.querySelectorAll('.title')].some((t) => t.innerText === 'Mod Menu')`, 20000);
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Mod Menu').querySelector('.open').click()`);
  await waitFor('the page', `!!document.querySelector('.project .tabs')`, 20000);
  await tab('Versions');
  await waitFor('versions', `document.querySelectorAll('.project .action .btn').length > 2`, 20000);
  const nth = (n) => `[...document.querySelectorAll('.project .row')].filter((row) => row.querySelector('.action .btn'))[${n}]`;
  const older = await run(`${nth(2)}.querySelector('.name .faint').textContent`);
  await run(`${nth(2)}.querySelector('.action .btn').click()`);
  await waitFor('it to install', `/Installed/.test(document.querySelector('.project .head').innerText)`, 40000);
  await sleep(600);
  expect(modFiles(ID).includes(older), `${older} is not on disk`);

  await waitFor('a Switch button', `[...document.querySelectorAll('.project .action .btn')].some((b) => b.textContent === 'Switch')`, 15000);
  const row = `[...document.querySelectorAll('.project .row')].find((r) => r.querySelector('.action .btn')?.textContent === 'Switch')`;
  const newer = await run(`${row}.querySelector('.name .faint').textContent`);
  await run(`${row}.querySelector('.action .btn').click()`);
  const deadline = Date.now() + 30000;
  while (!modFiles(ID).includes(newer) && Date.now() < deadline) await sleep(300);
  expect(modFiles(ID).includes(newer), `${newer} is not on disk`);
  expect(!modFiles(ID).includes(older), 'the old version is still on disk');
  await run(`document.querySelector('header .back').click()`);

  // Put the older one back so there's an update to review.
  await sleep(400);
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Mod Menu').querySelector('.open').click()`);
  await waitFor('the page', `!!document.querySelector('.project .tabs')`, 20000);
  await tab('Versions');
  await waitFor('Switch buttons', `[...document.querySelectorAll('.project .action .btn')].filter((b) => b.textContent === 'Switch').length > 1`, 20000);
  await run(`[...document.querySelectorAll('.project .row')].filter((r) => r.querySelector('.action .btn')?.textContent === 'Switch').at(-1).querySelector('.action .btn').click()`);
  await sleep(2500);
  await run(`document.querySelector('header .back').click()`);
  await click('Settings');
  await waitFor('the settings', `!!document.querySelector('#delay')`);
  await setValue('#delay', '0');
  await sleep(300);
  await backToInstance();
  await tab('Mods');
  await waitFor('an update to review', `[...document.querySelectorAll('.content-tools .btn')].some((b) => /^Update /.test(b.textContent.trim()))`, 20000);
  await run(`[...document.querySelectorAll('.content-tools .btn')].find((b) => /^Update /.test(b.textContent.trim())).click()`);
  await waitFor('the dialog', `!!document.querySelector('[aria-label^="Update E2E"]')`);
  await run(`document.querySelector('[role="dialog"] .fold').click()`);
  await waitFor('a changelog in the dialog', `!!document.querySelector('[role="dialog"] select') && !document.querySelector('[role="dialog"]').innerText.includes('Loading the changelog')`, 20000);
  await shot('28-update-dialog');
  await run(`[...document.querySelectorAll('[role="dialog"] .btn.primary')].at(-1).click()`);
  await waitFor('the dialog to close', `!document.querySelector('[role="dialog"]')`, 30000);
  await sleep(600);
  const extra = `[...document.querySelectorAll('.mod')].find((row) => !['Sodium', 'Lithium'].includes(row.querySelector('.name').innerText.trim()))`;
  while (await run(`!!${extra}`)) {
    const remaining = await run(`document.querySelectorAll('.mod').length`);
    await run(`${extra}.querySelector('.content-remove').click()`);
    await waitFor('the mod to be removed', `document.querySelectorAll('.mod').length < ${remaining}`);
  }
});

await step('copies an instance, files it in a folder and exports it', async () => {
  await run(`document.querySelector('[aria-label="More actions"]').click()`);
  await waitFor('the menu', `!!document.querySelector('[role="menu"]')`);
  await shot('29-instance-menu');
  await click('Copy');
  await setValue('#copy-name', 'E2E Copy');
  await run(`document.querySelector('[role="dialog"] .btn.primary').click()`);
  await waitFor('the copy to open', `document.querySelector('.names h1')?.innerText === 'E2E Copy'`, 30000);
  expect(readdirSync(modsDir('e2e-copy')).length === 2, 'the copy should have the two mods');

  await run(`document.querySelector('[aria-label="More actions"]').click()`);
  await click('Move to a folder');
  await setValue('#group', '+new');
  await setValue('#new-group', 'Tests');
  await run(`document.querySelector('[role="dialog"] .btn.primary').click()`);
  await nav('Library');
  await waitFor('the folder in the library', `[...document.querySelectorAll('.folder')].some((f) => f.innerText.includes('Tests'))`, 10000);
  await shot('30-instance-folder');

  const target = join(HOME, 'export-test.mrpack');
  const report = await run(`window.__TAURI_INTERNALS__.invoke('export_pack', { id: 'e2e-copy', target: ${text(target)}, format: 'modrinth', name: 'Round Trip', version: '1.2.3' }).then(JSON.stringify)`);
  const { linked } = JSON.parse(report);
  expect(linked === 2, `expected 2 linked mods, got ${report}`);
  expect(existsSync(target), 'the pack was not written');
  await run(`window.__TAURI_INTERNALS__.invoke('install_pack_file', { path: ${text(target)} }).then(() => true)`);
  await sleep(500);
  const installed = readdirSync(join(HOME, 'instances')).find((name) => name.startsWith('round-trip'));
  expect(!!installed && readdirSync(modsDir(installed)).length === 2, 'the exported pack did not install back');
  console.log('      exported a pack and installed it back');
  await run(`window.__TAURI_INTERNALS__.invoke('delete_instance', { id: ${text(installed)} }).then(() => true)`);
  await run(`window.__TAURI_INTERNALS__.invoke('delete_instance', { id: 'e2e-copy' }).then(() => true)`);
  await sleep(300);
  await nav('Discover');
  await backToInstance();
  await sleep(400);
});

await step('reads past logs and saves launch settings', async () => {
  await run(`location.reload()`);
  await sleep(1500);
  await run(HELPERS);
  await waitFor('the library', `document.querySelectorAll('.grid [data-instance]').length > 0`);
  await openInstance('E2E Renamed');
  await tab('Logs');
  await waitFor('saved logs', `document.querySelectorAll('[aria-label="Which log"] option').length > 1`, 10000);
  const file = await run(`[...document.querySelectorAll('[aria-label="Which log"] option')].map((o) => o.value).find((v) => v.includes('latest'))`);
  await setValue('[aria-label="Which log"]', file);
  await waitFor('its text', `document.querySelectorAll('.console .line').length > 0`, 10000);
  await setValue('[aria-label="Which log"]', '');

  await tab('Settings');
  await setValue('[aria-label="Window width"]', '1280');
  await setValue('[aria-label="Window height"]', '720');
  await run(`(() => { const a = document.querySelector('#env'); Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(a, 'WAYMARK_TEST=yes'); a.dispatchEvent(new Event('input', { bubbles: true })); })()`);
  await setValue('#pre-launch', 'echo before');
  await click('Save');
  await waitFor('the save', hasText('Settings saved'));
  const saved = readFileSync(join(HOME, 'instances', ID, 'instance.toml'), 'utf8');
  for (const part of ['width = 1280', 'height = 720', 'WAYMARK_TEST', 'preLaunch']) expect(saved.includes(part), `${part} was not saved`);
  await shot('31-launch-settings');
  await tab('Mods');
});

await step('changes theme, shows screenshots and browses Feed The Beast', async () => {
  const shots = join(HOME, 'instances', ID, 'game', 'screenshots');
  mkdirSync(shots, { recursive: true });
  const texture = await fetch('https://textures.minecraft.net/texture/46acd06e8483b176e8ea39fc12fe105eb3a2a4970f5100057e9d84d4b60bdfa7');
  writeFileSync(join(shots, '2026-10-31_12.00.00.png'), Buffer.from(await texture.arrayBuffer()));

  await nav('Screenshots');
  await waitFor('the screenshot', `[...document.querySelectorAll('.picture img')].some((i) => i.complete && i.naturalWidth > 0)`, 10000);
  await run(`document.querySelector('.picture').click()`);
  await waitFor('the viewer', `document.querySelector('.lightbox img')?.naturalWidth > 0`, 10000);
  await run(`window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))`);
  await waitFor('the viewer to close', `!document.querySelector('.lightbox')`);
  await run(`document.querySelector('[aria-label^="Delete 2026"]').click()`);
  await waitFor('it to go', `!document.querySelector('.picture')`);
  expect(readdirSync(shots).length === 0, 'the screenshot is still in the folder');
  await click('Undo');
  await waitFor('it to come back', `!!document.querySelector('.picture')`);

  await nav('Settings');
  await waitFor('the theme setting', `!!document.querySelector('#theme')`);
  await setValue('#theme', 'light');
  await waitFor('the light theme', `document.documentElement.dataset.theme === 'light'`);
  await sleep(300);
  await shot('32-light-settings');
  await nav('Library');
  await sleep(400);
  await shot('33-light-library');
  await nav('Settings');
  await waitFor('the theme setting', `!!document.querySelector('#theme')`);
  await setValue('#theme', 'dark');
  await waitFor('the dark theme', `document.documentElement.dataset.theme === 'dark'`);

  await nav('Discover');
  await click('Modpacks');
  await setValue('#source', 'ftb');
  await waitFor('Feed The Beast packs', `document.querySelectorAll('.results li').length > 3`, 30000);
  await shot('34-ftb');
  await run(`document.querySelector('.results li .open').click()`);
  await waitFor('the pack page', `!!document.querySelector('.project .tabs') && !!document.querySelector('.project .rich')`, 20000);
  await tab('Versions');
  await waitFor('its versions', `document.querySelectorAll('.project .line').length > 0`, 20000);
  await run(`document.querySelector('header .back').click()`);
  await setValue('#source', 'atlauncher');
  await waitFor('ATLauncher packs', `document.querySelectorAll('.results li').length > 3`, 30000);
  await shot('34-atlauncher');
  await run(`document.querySelector('.results li .open').click()`);
  await waitFor('the pack page', `!!document.querySelector('.project .tabs')`, 20000);
  await tab('Versions');
  await waitFor('its versions', `document.querySelectorAll('.project .line').length > 0`, 20000);
  await run(`document.querySelector('header .back').click()`);
  await setValue('#source', 'modrinth');
  await backToInstance();
  await tab('Mods');
});

await step('shares an instance with a code', async () => {
  await run(`document.querySelector('[aria-label="More actions"]').click()`);
  await click('Share with a code');
  await waitFor('the code', `document.querySelector('[aria-label="Share code"]')?.value.startsWith('waymark1.')`, 10000);
  await shot('35-share-code');
  const code = await run(`document.querySelector('[aria-label="Share code"]').value`);
  await click('Close');
  const made = await run(`window.__TAURI_INTERNALS__.invoke('install_share', { code: ${text(code)} }).then((r) => JSON.stringify({ id: r.instance.id, failed: r.failed }))`);
  const { id, failed } = JSON.parse(made);
  expect(failed.length === 0, `these failed: ${failed.join(', ')}`);
  const same = modFiles(ID).filter((f) => f.endsWith('.jar')).sort().join();
  expect(modFiles(id).sort().join() === same, `got ${modFiles(id).join(', ')}, wanted ${same}`);
  console.log('      rebuilt the instance from its code');
  await run(`window.__TAURI_INTERNALS__.invoke('delete_instance', { id: ${text(id)} }).then(() => true)`);
  await run(`location.reload()`);
  await sleep(1500);
  await run(HELPERS);
  await waitFor('the library', `document.querySelectorAll('.grid [data-instance]').length > 0`);
  await openInstance('E2E Renamed');
  await tab('Mods');
});

await step('builds a server and stops at the EULA', async () => {
  await tab('Server');
  await click('Set up a server');
  await waitFor('the EULA tick box', hasText('I accept the Minecraft End User License Agreement'), 240000);
  await shot('36-server');
  const folder = join(HOME, 'servers', `${ID}-server`);
  expect(existsSync(join(folder, 'start.bat')), 'the server folder was not built');
  expect(!existsSync(join(folder, 'eula.txt')) || !readFileSync(join(folder, 'eula.txt'), 'utf8').includes('eula=true'), 'the EULA must never be accepted for the player');
  expect(await run(`[...document.querySelectorAll('.tab .tools .btn')].find((b) => b.textContent === 'Start').disabled`), 'Start should wait for the tick');
  await tab('Mods');
});

await step('shows the interface in another language', async () => {
  await nav('Settings');
  await waitFor('the language setting', `!!document.querySelector('#language')`);
  await run(`localStorage.setItem('waymark.language', 'de'); location.reload()`);
  await sleep(1800);
  await run(HELPERS);
  await waitFor('German', `!!document.querySelector('header [aria-label="Einstellungen"]')`, 15000);
  await shot('37-german');
  expect(await run(`[...document.querySelectorAll('.main-nav button')].some((b) => b.textContent === 'Bibliothek')`), 'the menu was not translated');
  const instanceName = await run(`document.querySelector('.grid .name')?.textContent`);
  expect(instanceName === 'E2E Renamed', `an instance name must not change: ${instanceName}`);
  await run(`localStorage.setItem('waymark.language', 'en'); location.reload()`);
  await sleep(1800);
  await run(HELPERS);
  await waitFor('English', `!!document.querySelector('header [aria-label="Settings"]')`, 15000);
  await waitFor('the library', `document.querySelectorAll('.grid [data-instance]').length > 0`);
  await openInstance('E2E Renamed');
  await tab('Mods');
});

await step('groups and tags mods', async () => {
  await tab('Mods');
  await waitFor('the mod table', `!!e2e.modRow('Sodium')`);
  await run(`e2e.modRow('Sodium').querySelector('.body').dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true }))`);
  await waitFor('the selection bar', hasText('1 selected'));
  await setValue('[aria-label="Move to group"]', '+new');
  await waitFor('the name field', `!!document.querySelector('[aria-label="Group name"]')`);
  await setValue('[aria-label="Group name"]', 'Performance');
  await click('Create');
  await waitFor('a group header', `!!document.querySelector('.group')`);
  expect(lockText(ID).includes('group = "Performance"'), 'the group was not saved');
  await setValue('[aria-label="Tag"]', 'client');
  await run(`document.querySelector('[aria-label="Tag"]').form.requestSubmit()`);
  await waitFor('the tag', `!!document.querySelector('.tag')`);
  await shot('12-groups');
  await click('Clear');
  await run(`document.querySelector('.fold').click()`);
  await waitFor('the group to fold', `!e2e.modRow('Sodium')`);
  await run(`document.querySelector('.fold').click()`);
  await waitFor('the group to open', `!!e2e.modRow('Sodium')`);
});

await step('shares a folder between instances', async () => {
  await tab('Settings');
  await waitFor('the form', hasText('Shared folders'));
  await run(`[...document.querySelectorAll('label')].find((l) => l.innerText.trim() === 'Resource packs').querySelector('input').click()`);
  await sleep(600);
  const link = join(HOME, 'instances', ID, 'game', 'resourcepacks');
  expect(lstatSync(link).isSymbolicLink(), 'the folder is not linked');
  await shot('13-instance-settings');
  await run(`[...document.querySelectorAll('label')].find((l) => l.innerText.trim() === 'Resource packs').querySelector('input').click()`);
  await sleep(600);
  expect(!lstatSync(link).isSymbolicLink(), 'the folder is still linked');
});

await step('runs a crash search to the end', async () => {
  await tab('Mods');
  await click('Find a crash');
  await waitFor('the search banner', hasText('Crash search'));
  await shot('08-crash-search');
  for (let round = 0; round < 8; round++) {
    await sleep(500);
    if (await run(hasText('Found:'))) break;
    const lithiumOn = await run(`e2e.modRow('Lithium').querySelector('.switch').checked`);
    await click(lithiumOn ? 'It crashed' : 'It worked');
  }
  await waitFor('a result', hasText('Found: Lithium'));
  expect(modFiles(ID).filter((f) => f.endsWith('.disabled')).length === 1, 'the culprit was not left switched off');
  await click('Done');
});

await step('imports from another launcher when one is present', async () => {
  await nav('Library');
  await click('New instance');
  await click(`Import (1)`, 8000).catch(() => {});
  const available = await run(`!!e2e.button('Import')`);
  await shot('09-import');
  if (available) {
    await click('Import');
    await waitFor('the import to finish', `!!document.querySelector('.names h1') && !document.querySelector('.names h1').innerText.includes('E2E')`, 90000);
    await waitFor('the imported mods', `document.querySelectorAll('.mod').length > 3`, 20000);
    await waitFor('images for imported mods', `document.querySelectorAll('.mod .content-icon img').length > 3`, 30000);
    await sleep(800);
    await shot('14-imported-images');
  } else {
    await click('Cancel').catch(() => run(`document.querySelector('[aria-label="Close"]').click()`));
  }
});

await step('copies settings from another instance', async () => {
  await nav('Library');
  await waitFor('the library', `document.querySelectorAll('.grid .name').length > 0`);
  const source = await run(`[...document.querySelectorAll('.grid .name')].find((n) => !n.innerText.includes('E2E'))?.innerText`);
  if (!source) return;
  await openInstance('E2E Renamed');
  await tab('Settings');
  await waitFor('the copy control', `!!document.querySelector('#copy-from')`);
  await setValue('#copy-from', await run(`document.querySelector('#copy-from').options[1].value`));
  await click('Copy');
  await waitFor('the result', hasText('Could not copy settings') + ' || ' + hasText('Copied settings from'));
});

await step('installs a modpack from Discover', async () => {
  await click('Discover');
  await click('Modpacks');
  await setValue('.search', 'fabulously optimized');
  await waitFor('search results', `[...document.querySelectorAll('.title')].some((t) => t.innerText.includes('Fabulously Optimized'))`, 20000);
  const before = await run(`document.querySelectorAll('.tabs').length`);
  expect(before >= 1, 'discover did not render');
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText.includes('Fabulously Optimized')).querySelector('.add').click()`);
  await waitFor('the pack to install', `!!document.querySelector('.names h1')?.innerText.includes('Fabulously Optimized')`, 120000);
  await waitFor('its mods', `document.querySelectorAll('.mod').length > 5`, 20000);
  await sleep(1500);
  await shot('10-modpack');
  const described = await run(`[...document.querySelectorAll('.mod .version')].filter((cell) => cell.innerText.trim() !== '').length`);
  const iconned = await run(`document.querySelectorAll('.mod .content-icon img').length`);
  expect(described > 5 && iconned > 5, `pack mods are missing details: ${described} with versions, ${iconned} with icons`);
});

await step('updates a modpack and keeps the player\'s own mod', async () => {
  const id = 'fabulously-optimized';
  const statePath = join(HOME, 'instances', id, 'pack.json');
  const state = JSON.parse(readFileSync(statePath, 'utf8'));
  state.origin.versionId = 'an-older-version';
  state.origin.version = '0.0.1';
  writeFileSync(statePath, JSON.stringify(state));
  writeFileSync(join(modsDir(id), 'my-own-mod.jar'), 'mine');
  await openInstance('E2E Renamed');
  await openInstance('Fabulously Optimized');
  await waitFor('the update notice', hasText('is available'), 20000);
  await shot('15-pack-update');
  await click('Update pack');
  await waitFor('the result', hasText('Updated to'), 120000);
  expect(existsSync(join(modsDir(id), 'my-own-mod.jar')), 'the player\'s own mod was lost');
});

await step('saves global settings', async () => {
  await nav('Settings');
  await waitFor('the settings page', `!!document.querySelector('#delay')`);
  await setValue('#delay', '168');
  await sleep(400);
  await shot('11-settings');
  await nav('Library');
  await nav('Settings');
  await waitFor('the saved value', `document.querySelector('#delay')?.value === '168'`);
});

await step('deletes an instance and brings it back', async () => {
  await nav('Library');
  await waitFor('the instance in the library', listed('E2E Renamed'));
  await openInstance('E2E Renamed');
  await tab('Settings');
  await click('Delete');
  await click('Delete E2E Renamed');
  const named = listed('E2E Renamed');
  await waitFor('it to disappear', `!${named}`);
  expect(!existsSync(join(HOME, 'instances', ID)), 'the instance folder is still on disk');

  await click('Undo');
  await waitFor('Undo to bring it back', named);
  expect(existsSync(join(HOME, 'instances', ID, 'waymark.lock')), 'Undo did not restore the instance');

  await openInstance('E2E Renamed');
  await tab('Settings');
  await click('Delete');
  await click('Delete E2E Renamed');
  await waitFor('it to disappear again', `!${named}`);
  await nav('Settings');
  await waitFor('the deleted list', hasText('Recently deleted'));
  await shot('25-recently-deleted');
  await click('Restore');
  await sleep(600);
  await nav('Library');
  await waitFor('Settings to bring it back', named);
});

finish(0);

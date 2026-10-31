// Drives the real desktop app the way a player would and checks the results on
// disk. It talks to WebView2 over the Chrome DevTools Protocol.
//
//   $env:WAYMARK_HOME = '<empty folder>'
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9222'
//   start the debug app with the dev server running, then: node e2e/run.mjs
//
// Set E2E_SHOTS to a folder to save screenshots along the way.

import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

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
      return [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === label && !b.disabled);
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

const nav = (label) => run(`[...document.querySelectorAll('header nav button')].find((b) => b.innerText === ${text(label)}).click()`);
const tab = (label) => run(`[...document.querySelectorAll('.tabs button')].find((b) => b.innerText === ${text(label)}).click()`);
const openInstance = async (name) => {
  await run(`[...document.querySelectorAll('aside .row')].find((r) => r.innerText.includes(${text(name)})).click()`);
  await waitFor(name, `document.querySelector('.names h1')?.innerText === ${text(name)}`);
};
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
  await click('Add an instance');
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
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Sodium').querySelector('button').click()`);
  await waitFor('Sodium to install', `[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Sodium')?.innerText.includes('Added')`, 40000);
  await setValue('.search', 'lithium');
  await waitFor('search results', `[...document.querySelectorAll('.title')].some((t) => t.innerText === 'Lithium')`, 20000);
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Lithium').querySelector('button').click()`);
  await waitFor('Lithium to install', `[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText === 'Lithium')?.innerText.includes('Added')`, 40000);
  expect(modFiles(ID).length === 2, `expected 2 mods on disk, found ${modFiles(ID).length}`);
});

await step('lists mods and switches one off and on', async () => {
  await click('Library');
  await waitFor('the mod table', `!!e2e.modRow('Sodium')`);
  await shot('04-mods');
  await run(`e2e.modRow('Sodium').querySelector('input').click()`);
  await waitFor('the file to be renamed', 'true');
  await sleep(500);
  expect(modFiles(ID).some((f) => f.endsWith('.disabled')), 'no .disabled file on disk');
  await run(`e2e.modRow('Sodium').querySelector('input').click()`);
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
  await waitFor('the new list', `[...document.querySelectorAll('ol .row')].some((r) => r.innerText.includes('Added Lithium'))`);
  await run(`[...document.querySelectorAll('ol .row')].find((r) => r.innerText.includes('Added Lithium')).click()`);
  await click('Return to this waymark');
  await sleep(800);
  expect(modFiles(ID).length === 2, `expected both mods back, found ${modFiles(ID).length}`);
});

await step('launches the game, shows its log and stops it', async () => {
  await click('Logs');
  await click('Play');
  await waitFor('the game to start', `document.querySelector('.sub .running') !== null`, 120000);
  await waitFor('log lines', hasText('Setting user'), 90000);
  await shot('06-running');
  await click('Stop');
  await waitFor('the game to stop', `document.querySelector('.sub .running') === null`, 20000);
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

await step('groups and tags mods', async () => {
  await tab('Mods');
  await waitFor('the mod table', `!!e2e.modRow('Sodium')`);
  await run(`e2e.modRow('Sodium').querySelector('.body').click()`);
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
    const lithiumOn = await run(`e2e.modRow('Lithium').querySelector('input').checked`);
    await click(lithiumOn ? 'It crashed' : 'It worked');
  }
  await waitFor('a result', hasText('Found: Lithium'));
  expect(modFiles(ID).filter((f) => f.endsWith('.disabled')).length === 1, 'the culprit was not left switched off');
  await click('Done');
});

await step('imports from another launcher when one is present', async () => {
  await run(`document.querySelector('[aria-label="Add an instance"]').click()`);
  await click(`Import (1)`, 8000).catch(() => {});
  const available = await run(`!!e2e.button('Import')`);
  await shot('09-import');
  if (available) {
    await click('Import');
    await waitFor('the import to finish', `document.querySelectorAll('aside li').length === 2`, 90000);
    await waitFor('the imported mods', `document.querySelectorAll('.mod').length > 3`, 20000);
    await waitFor('images for imported mods', `document.querySelectorAll('.mod .icon img').length > 3`, 30000);
    await sleep(800);
    await shot('14-imported-images');
  } else {
    await click('Cancel').catch(() => run(`document.querySelector('[aria-label="Close"]').click()`));
  }
});

await step('copies settings from another instance', async () => {
  const source = await run(`[...document.querySelectorAll('aside .row')].find((r) => !r.innerText.includes('E2E'))?.innerText.split('\\n')[0]`);
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
  await run(`[...document.querySelectorAll('li')].find((li) => li.querySelector('.title')?.innerText.includes('Fabulously Optimized')).querySelector('button').click()`);
  await waitFor('the pack to install', `!!document.querySelector('.names h1')?.innerText.includes('Fabulously Optimized')`, 120000);
  await waitFor('its mods', `document.querySelectorAll('.mod').length > 5`, 20000);
  await sleep(1500);
  await shot('10-modpack');
  const described = await run(`[...document.querySelectorAll('.mod .version')].filter((cell) => cell.innerText.trim() !== '').length`);
  const iconned = await run(`document.querySelectorAll('.mod .icon img').length`);
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
  await run(`[...document.querySelectorAll('header nav button')].find((b) => b.innerText === 'Settings').click()`);
  await waitFor('the settings page', `!!document.querySelector('#delay')`);
  await setValue('#delay', '168');
  await sleep(400);
  await shot('11-settings');
  await run(`[...document.querySelectorAll('header nav button')].find((b) => b.innerText === 'Library').click()`);
  await run(`[...document.querySelectorAll('header nav button')].find((b) => b.innerText === 'Settings').click()`);
  await waitFor('the saved value', `document.querySelector('#delay')?.value === '168'`);
});

await step('deletes an instance', async () => {
  await nav('Library');
  await waitFor('the sidebar', `[...document.querySelectorAll('aside .name')].some((n) => n.innerText === 'E2E Renamed')`);
  await openInstance('E2E Renamed');
  await tab('Settings');
  await click('Delete');
  await click('Delete E2E Renamed');
  await waitFor('it to disappear', `![...document.querySelectorAll('aside .name')].some((n) => n.innerText === 'E2E Renamed')`);
  expect(!existsSync(join(HOME, 'instances', ID)), 'the instance folder is still on disk');
});

finish(0);

// Finds interface text with no row in src/lib/catalog.ts.
//
// The app translates by swapping exact strings in the page as they're drawn, so
// a string without a row stays English in every language. This reads every
// .svelte template, the string arguments to toast(), fail(), changed() and
// askToSignIn(), and `label:` properties, and reports what the catalog lacks.
// It also checks the catalog itself: seven filled columns and no repeated keys.
//
// Run with `npm run check:i18n`. It exits with 1 when something is missing.

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { parse } from 'svelte/compiler';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', 'src');
const CATALOG = join(ROOT, 'lib', 'catalog.ts');
/** Sample data for the browser preview, never shown in the app. */
const SKIPPED_FILES = new Set(['lib/catalog.ts', 'lib/mock.ts']);
/** The same list as LEAVE_ALONE in lib/i18n.ts: text inside these is never translated. */
const LEAVE_ALONE = new Set(['rich', 'selectable', 'mono', 'name', 'title']);
const LEAVE_ALONE_TAGS = new Set(['script', 'style', 'textarea', 'svelte:head']);
/** Attributes the translator rewrites, and component props that end up as visible text. */
const ATTRIBUTES = new Set(['placeholder', 'title', 'aria-label']);
const COMPONENT_PROPS = new Set(['title', 'what']);
const CALLS = new Map([
  ['toast', [1, 2]],
  ['fail', [1]],
  ['changed', [1, 2, 3]],
  ['askToSignIn', [0]],
  // The plain-words explanations in lib/errors.ts.
  ['say', [0]],
]);

/**
 * Text that stays as written in every language: names of products, companies,
 * loaders and stores, units, file names, and the app's own name.
 */
const ALLOWED = new Set([
  'Waymark Launcher',
  // The feature keeps its name in every language, as the catalog already writes it.
  'Waymarks',
  'Waymark',
  'Markdown',
  'CSV',
  'Modrinth',
  'CurseForge',
  'Feed The Beast',
  'ATLauncher',
  'Technic',
  'Prism Launcher',
  'MultiMC',
  'Modrinth App',
  'GDLauncher',
  'Discord',
  'Microsoft',
  'Minecraft',
  'Mojang',
  'NVIDIA',
  'AMD',
  'Intel',
  'Fabric',
  'Quilt',
  'Forge',
  'NeoForge',
  'Legacy Fabric',
  'Babric',
  'Ornithe',
  'LiteLoader',
  'Rift',
  'Cleanroom',
  'Infdev',
  'Indev',
  'Pre-Classic',
  'SMP',
  'PvP',
  'PvE',
  'KitPvP',
  'MMO',
  'RPG',
  'OP',
  'OneBlock',
  'Bed Wars',
  'B',
  'KB',
  'MB',
  'GB',
  '{} B',
  '{} KB',
  '{} MB',
  '{} GB',
  '{} ms',
  'mclo.gs',
  'README.txt',
  'NAME=value',
  'Minecraft {}',
  'Waymark Launcher {}',
]);

const PLACEHOLDER = /\{(?:n|x|y)\}/g;

function normalize(text) {
  return text.replace(PLACEHOLDER, '{}').replace(/\s+/g, ' ').trim();
}

function hasWords(text) {
  return /\p{L}{2,}/u.test(text.replace(/\{\}/g, ''));
}

// The catalog is TypeScript, so it's transpiled and imported as a module.
async function loadCatalog() {
  const source = readFileSync(CATALOG, 'utf8');
  const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  const module = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
  return module;
}

function files(folder) {
  const out = [];
  for (const name of readdirSync(folder)) {
    const path = join(folder, name);
    if (statSync(path).isDirectory()) out.push(...files(path));
    else if (/\.(svelte|ts)$/.test(name)) out.push(path);
  }
  return out;
}

function lineOf(source, offset) {
  let line = 1;
  for (let index = 0; index < offset && index < source.length; index++) if (source.charCodeAt(index) === 10) line++;
  return line;
}

// Strings an expression can become, with `{}` for anything worked out at run time.

/** For template expressions, which the Svelte parser gives as ESTree. */
function treeStrings(node) {
  if (!node) return [];
  switch (node.type) {
    case 'Literal':
      return typeof node.value === 'string' ? [node.value] : [];
    case 'TemplateLiteral':
      return [node.quasis.map((quasi, index) => quasi.value.cooked + (index < node.expressions.length ? '{}' : '')).join('')];
    case 'ConditionalExpression':
      return [...branch(node.consequent), ...branch(node.alternate)];
    case 'LogicalExpression':
      return [...branch(node.left), ...branch(node.right)];
    case 'TSAsExpression':
    case 'TSNonNullExpression':
      return treeStrings(node.expression);
    default:
      return [];
  }
}

/** One side of a choice: its strings, or `{}` when that side is worked out at run time. */
function branch(node) {
  const found = treeStrings(node);
  return found.length ? found : ['{}'];
}

/** Whether an expression could only ever be text worked out at run time. */
function isDynamic(node) {
  return treeStrings(node).length === 0;
}

/** For scripts, read with the TypeScript compiler. */
function scriptStrings(node) {
  if (!node) return [];
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return [node.text];
  if (ts.isTemplateExpression(node)) return [node.head.text + node.templateSpans.map((span) => `{}${span.literal.text}`).join('')];
  if (ts.isConditionalExpression(node)) return [...scriptStrings(node.whenTrue), ...scriptStrings(node.whenFalse)];
  if (ts.isBinaryExpression(node) && [ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.BarBarToken].includes(node.operatorToken.kind)) {
    return [...scriptStrings(node.left), ...scriptStrings(node.right)];
  }
  if (ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isNonNullExpression(node)) return scriptStrings(node.expression);
  return [];
}

function readScript(code, report, offset = 0) {
  const file = ts.createSourceFile('script.ts', code, ts.ScriptTarget.Latest, true);
  const visit = (node) => {
    if (ts.isCallExpression(node)) {
      const callee = node.expression;
      const name = ts.isPropertyAccessExpression(callee) ? callee.name.text : ts.isIdentifier(callee) ? callee.text : '';
      for (const index of CALLS.get(name) ?? []) {
        for (const text of scriptStrings(node.arguments[index])) report(text, offset + node.arguments[index].getStart(file));
      }
    }
    if (ts.isPropertyAssignment(node) && ts.isIdentifier(node.name) && node.name.text === 'label') {
      for (const text of scriptStrings(node.initializer)) report(text, offset + node.initializer.getStart(file));
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
}

function staticClasses(attributes) {
  const classes = new Set();
  for (const attribute of attributes ?? []) {
    if (attribute.type === 'Attribute' && attribute.name === 'class' && Array.isArray(attribute.value)) {
      for (const part of attribute.value) if (part.type === 'Text') part.data.split(/\s+/).forEach((name) => name && classes.add(name));
    }
    // A class switched on at run time might be one of the untranslated ones, so it counts.
    if (attribute.type === 'ClassDirective') classes.add(attribute.name);
  }
  return classes;
}

/** The texts a run of text and {expressions} can produce, or none if it's all worked out at run time. */
function runTexts(parts) {
  if (parts.length === 1 && parts[0].type === 'ExpressionTag') return treeStrings(parts[0].expression);
  let variants = [''];
  for (const part of parts) {
    if (part.type === 'Text') {
      variants = variants.map((text) => text + part.data);
      continue;
    }
    const options = isDynamic(part.expression) ? ['{}'] : treeStrings(part.expression);
    const next = [];
    for (const text of variants) for (const option of options) next.push(text + option);
    variants = next.slice(0, 16);
  }
  return variants;
}

function readTemplate(fragment, report) {
  const walk = (nodes, quiet) => {
    let run = [];
    const flush = () => {
      if (run.length && !quiet) for (const text of runTexts(run)) report(text, run[0].start);
      run = [];
    };
    for (const node of nodes ?? []) {
      if (node.type === 'Text' || node.type === 'ExpressionTag') {
        run.push(node);
        continue;
      }
      flush();
      visit(node, quiet);
    }
    flush();
  };
  const attributes = (node, quiet) => {
    const isComponent = node.type === 'Component';
    for (const attribute of node.attributes ?? []) {
      if (attribute.type !== 'Attribute' || quiet) continue;
      if (!ATTRIBUTES.has(attribute.name) && !(isComponent && COMPONENT_PROPS.has(attribute.name))) continue;
      if (attribute.value === true) continue;
      const parts = Array.isArray(attribute.value) ? attribute.value : [attribute.value];
      for (const text of runTexts(parts)) report(text, attribute.start);
    }
  };
  const visit = (node, quiet) => {
    switch (node.type) {
      case 'RegularElement':
      case 'SvelteElement':
      case 'Component':
      case 'SvelteComponent':
      case 'SlotElement': {
        const leave = quiet || LEAVE_ALONE_TAGS.has(node.name) || [...staticClasses(node.attributes)].some((name) => LEAVE_ALONE.has(name));
        attributes(node, quiet || LEAVE_ALONE_TAGS.has(node.name));
        walk(node.fragment?.nodes, leave);
        break;
      }
      case 'IfBlock':
        walk(node.consequent?.nodes, quiet);
        walk(node.alternate?.nodes, quiet);
        break;
      case 'EachBlock':
        walk(node.body?.nodes, quiet);
        walk(node.fallback?.nodes, quiet);
        break;
      case 'KeyBlock':
        walk(node.fragment?.nodes, quiet);
        break;
      case 'AwaitBlock':
        walk(node.pending?.nodes, quiet);
        walk(node.then?.nodes, quiet);
        walk(node.catch?.nodes, quiet);
        break;
      case 'SnippetBlock':
        walk(node.body?.nodes, quiet);
        break;
      default:
        break;
    }
  };
  walk(fragment.nodes, false);
}

const catalog = await loadCatalog();
const problems = [];
const known = new Set();
const patterns = [];
const seen = new Map();
catalog.ROWS.forEach((row, index) => {
  const where = `lib/catalog.ts row ${index + 1}`;
  if (row.length !== catalog.LANGUAGES.length + 1) problems.push(`${where}: has ${row.length} columns instead of ${catalog.LANGUAGES.length + 1}`);
  if (row.some((cell) => typeof cell !== 'string' || !cell.trim())) problems.push(`${where}: has an empty column (${row[0]})`);
  if (seen.has(row[0])) problems.push(`${where}: repeats "${row[0]}" from row ${seen.get(row[0]) + 1}`);
  seen.set(row[0], index);
  known.add(normalize(row[0]));
  if (row[0].includes('{')) patterns.push(compile(row[0]));
});

/**
 * Patterns the way lib/i18n.ts matches them, so text built from a few pieces counts
 * as covered when a broader row would translate it in the app, like `Only for {x}`
 * covering `Only for {version} {loader}`. Each piece worked out at run time stands in as 7.
 */
function compile(english) {
  const escaped = english.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`^${escaped.replace(/\\\{n\\\}/g, '(\\d+)').replace(/\\\{(?:x|y)\\\}/g, '(.+?)')}$`);
}

/** Words of the text itself still need their own row, so a broad row may only take the run-time pieces. */
function covered(key) {
  if (known.has(key)) return true;
  if (!key.includes('{}')) return false;
  const sample = key.replace(/\{\}/g, '7');
  return patterns.some((pattern) => {
    const parts = pattern.exec(sample);
    return !!parts && parts.slice(1).every((part) => !/\p{L}/u.test(part));
  });
}

const missing = new Map();
for (const path of files(ROOT)) {
  const name = relative(ROOT, path).replaceAll('\\', '/');
  if (SKIPPED_FILES.has(name)) continue;
  const source = readFileSync(path, 'utf8');
  const report = (text, offset) => {
    const key = normalize(text);
    if (!key || !hasWords(key) || ALLOWED.has(key) || covered(key)) return;
    if (!missing.has(key)) missing.set(key, []);
    missing.get(key).push(`${name}:${lineOf(source, offset)}`);
  };
  if (name.endsWith('.ts')) {
    readScript(source, report);
    continue;
  }
  const tree = parse(source, { modern: true, filename: name });
  for (const script of [tree.module, tree.instance]) {
    if (script) readScript(source.slice(script.content.start, script.content.end), report, script.content.start);
  }
  readTemplate(tree.fragment, report);
}

for (const problem of problems) console.error(problem);
for (const [text, places] of [...missing].sort(([, a], [, b]) => a[0].localeCompare(b[0]))) {
  console.error(`No catalog row: "${text}"  (${places.join(', ')})`);
}
if (problems.length || missing.size) {
  console.error(`\n${missing.size} missing strings, ${problems.length} catalog problems.`);
  process.exit(1);
}
console.log(`Every interface string has a catalog row (${catalog.ROWS.length} rows).`);

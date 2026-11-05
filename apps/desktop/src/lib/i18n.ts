// Shows the interface in another language by swapping exact strings as they're
// drawn. Only text in the catalog changes, so a missing string falls back to
// English and a mod's name or description can't be mistranslated. Nothing here
// loads for English.

const KEY = 'waymark.language';
/** Anything the player or a store wrote, which is never translated. */
const LEAVE_ALONE = '.rich, .selectable, .mono, .name, .title, input, textarea, script, style';
const ATTRIBUTES = ['placeholder', 'title', 'aria-label'];
const PLURAL = /^(zero|one|two|few|many|other):\s?/;

export const LANGUAGE_NAMES: Record<string, string> = {
  en: 'English',
  es: 'Español',
  pt: 'Português (Brasil)',
  fr: 'Français',
  de: 'Deutsch',
  ru: 'Русский',
  zh: '简体中文',
};

/** The full locale for each language, for dates, numbers and plural rules. */
const LOCALES: Record<string, string> = { en: 'en', es: 'es', pt: 'pt-BR', fr: 'fr', de: 'de', ru: 'ru', zh: 'zh-CN' };

let exact = new Map<string, string>();
let patterns: ReturnType<typeof compile>[] = [];
let rules: Intl.PluralRules | null = null;

function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * Turns `Update {n} mods` into something that can match `Update 3 mods`. A number
 * can be written `1,234`, `1.234` or `1 234`, depending on the language.
 */
function compile(english: string, translated: string) {
  const names: string[] = [];
  const source = escapeRegex(english).replace(/\\\{(n|x|y)\\\}/g, (_, name: string) => {
    names.push(name);
    return name === 'n' ? '(\\d(?:[\\d,.]|[\\u00a0\\u202f ](?=\\d))*)' : '(.+?)';
  });
  return { match: new RegExp(`^${source}$`), names, translated, weight: english.replace(/\{(n|x|y)\}/g, '').length };
}

/**
 * A translation can give a form for each plural category, like
 * `one: {n} мод | few: {n} мода | many: {n} модов | other: {n} мода`, and the
 * first number in the text picks the form the language's own rules call for.
 */
function pluralForm(translated: string, count: string | undefined): string {
  if (!translated.includes('|')) return translated;
  const forms = translated.split('|').map((part) => part.trim());
  if (!forms.every((form) => PLURAL.test(form))) return translated;
  const byCategory = new Map(forms.map((form) => [PLURAL.exec(form)![1], form.replace(PLURAL, '')]));
  const value = parseCount(count ?? '');
  const category = rules && Number.isFinite(value) ? rules.select(value) : 'other';
  return byCategory.get(category) ?? byCategory.get('other') ?? forms[0].replace(PLURAL, '');
}

/** Reads `1,234`, `1 234` and `2.5` alike: a separator before three digits groups them. */
function parseCount(text: string): number {
  if (!text) return NaN;
  const joined = text.replace(/[\s  ]/g, '').replace(/[.,](?=\d{3}(?:\D|$))/g, '');
  return Number(joined.replace(',', '.'));
}

export function translate(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  let found = exact.get(trimmed);
  if (!found) {
    for (const { match, names, translated } of patterns) {
      const parts = match.exec(trimmed);
      if (!parts) continue;
      const count = names.includes('n') ? parts[names.indexOf('n') + 1] : undefined;
      // Sentences get built from smaller pieces, so the pieces are translated too.
      found = names.reduce((text, name, index) => text.replace(`{${name}}`, translate(parts[index + 1]) ?? parts[index + 1]), pluralForm(translated, count));
      break;
    }
  }
  return found ? text.replace(trimmed, found) : null;
}

function fixText(node: Text) {
  if (node.parentElement?.closest(LEAVE_ALONE)) return;
  const next = translate(node.nodeValue ?? '');
  if (next !== null && next !== node.nodeValue) node.nodeValue = next;
}

function fixAttributes(element: Element) {
  for (const name of ATTRIBUTES) {
    const value = element.getAttribute(name);
    const next = value && translate(value);
    if (next && next !== value) element.setAttribute(name, next);
  }
}

function fixTree(root: Node) {
  if (root instanceof Text) return fixText(root);
  if (!(root instanceof Element) || root.closest('.rich, script, style')) return;
  fixAttributes(root);
  for (const element of root.querySelectorAll(ATTRIBUTES.map((name) => `[${name}]`).join(','))) fixAttributes(element);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) fixText(walker.currentNode as Text);
}

function resolve(language: string): string {
  if (language !== 'system') return language;
  const code = navigator.language.slice(0, 2).toLowerCase();
  return code in LANGUAGE_NAMES ? code : 'en';
}

/** The language the interface is shown in, as a code like `de`. */
export function currentLanguage(): string {
  return resolve(savedLanguage());
}

let locale: string | null = null;

/** Where dates and numbers take their format from: the app's language, not the computer's. */
export function currentLocale(): string {
  locale ??= LOCALES[currentLanguage()] ?? 'en';
  return locale;
}

export function savedLanguage(): string {
  try {
    return localStorage.getItem(KEY) ?? 'system';
  } catch {
    return 'system';
  }
}

/** Starts translating, if the chosen language isn't English. */
export async function startTranslation() {
  const language = resolve(savedLanguage());
  if (language === 'en') return;
  document.documentElement.lang = language;
  const { LANGUAGES, ROWS } = await import('./catalog');
  const column = LANGUAGES.indexOf(language as (typeof LANGUAGES)[number]) + 1;
  if (!column) return;
  try {
    rules = new Intl.PluralRules(currentLocale());
  } catch {
    rules = null;
  }
  for (const row of ROWS) {
    if (row[0].includes('{')) patterns.push(compile(row[0], row[column]));
    else exact.set(row[0], row[column]);
  }
  // The row with the most fixed words goes first, so `Remove {x} from your servers`
  // wins over `Remove {x}` wherever each sits in the catalog.
  patterns.sort((a, b) => b.weight - a.weight);
  fixTree(document.body);
  new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === 'characterData') fixText(record.target as Text);
      else if (record.type === 'attributes') fixAttributes(record.target as Element);
      else record.addedNodes.forEach(fixTree);
    }
  }).observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ATTRIBUTES });
}

/** Saves the choice. It takes effect on the reload that follows. */
export function chooseLanguage(language: string) {
  try {
    localStorage.setItem(KEY, language);
  } catch {
    // The saved setting still applies on the next start.
  }
  location.reload();
}

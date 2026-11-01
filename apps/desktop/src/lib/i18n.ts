// Shows the interface in another language by swapping exact strings as they're
// drawn. Only text in the catalog changes, so a missing string falls back to
// English and a mod's name or description can't be mistranslated. Nothing here
// loads for English.

const KEY = 'waymark.language';
/** Anything the player or a store wrote, which is never translated. */
const LEAVE_ALONE = '.rich, .selectable, .mono, .name, .title, input, textarea, script, style';
const ATTRIBUTES = ['placeholder', 'title', 'aria-label'];

export const LANGUAGE_NAMES: Record<string, string> = {
  en: 'English',
  es: 'Español',
  pt: 'Português (Brasil)',
  fr: 'Français',
  de: 'Deutsch',
  ru: 'Русский',
  zh: '简体中文',
};

let exact = new Map<string, string>();
let patterns: ReturnType<typeof compile>[] = [];

function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Turns `Update {n} mods` into something that can match `Update 3 mods`. */
function compile(english: string, translated: string) {
  const names: string[] = [];
  const source = escapeRegex(english).replace(/\\\{(n|x|y)\\\}/g, (_, name: string) => {
    names.push(name);
    return name === 'n' ? '(\\d[\\d,.]*)' : '(.+?)';
  });
  return { match: new RegExp(`^${source}$`), names, translated };
}

export function translate(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  let found = exact.get(trimmed);
  if (!found) {
    for (const { match, names, translated } of patterns) {
      const parts = match.exec(trimmed);
      if (!parts) continue;
      // Sentences get built from smaller pieces, so the pieces are translated too.
      found = names.reduce((text, name, index) => text.replace(`{${name}}`, translate(parts[index + 1]) ?? parts[index + 1]), translated);
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
  for (const row of ROWS) {
    if (row[0].includes('{')) patterns.push(compile(row[0], row[column]));
    else exact.set(row[0], row[column]);
  }
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

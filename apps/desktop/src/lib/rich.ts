// Turns store descriptions into DOM. Anyone can publish a project, so bodies get
// the same distrust as any other outside input: an allow list decides what
// survives, and the result is attached as nodes, never re-parsed as HTML.

import DOMPurify from 'dompurify';
import { marked } from 'marked';

export type RichFormat = 'markdown' | 'html';

const ALLOWED_TAGS = [
  'a', 'abbr', 'b', 'blockquote', 'br', 'caption', 'center', 'cite', 'code', 'col', 'colgroup', 'dd', 'del', 'details', 'dfn', 'div', 'dl', 'dt',
  'em', 'figcaption', 'figure', 'font', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'i', 'iframe', 'img', 'input', 'ins', 'kbd', 'li', 'mark', 'ol',
  'p', 'pre', 'q', 'rp', 'rt', 'ruby', 's', 'samp', 'small', 'span', 'strike', 'strong', 'sub', 'summary', 'sup', 'table', 'tbody', 'td', 'tfoot',
  'th', 'thead', 'tr', 'tt', 'u', 'ul', 'var', 'wbr',
];
// No class or id, so a description can't borrow the app's own buttons.
const ALLOWED_ATTR = [
  'align', 'alt', 'checked', 'colspan', 'disabled', 'height', 'href', 'open', 'reversed', 'rowspan', 'src', 'start', 'style', 'title', 'type', 'width',
];
// Positioning and sizing could paint a fake dialog over the app, and dark text
// colors picked for a white page would be unreadable here.
const SAFE_STYLE = /^(text-align\s*:\s*(left|right|center|justify)|image-rendering\s*:\s*pixelated)$/i;
const OPENABLE = new Set(['http:', 'https:', 'mailto:']);
const VIDEO_ID = /^[\w-]{6,20}$/;
const PROJECT_LINK = /^https?:\/\/(?:www\.)?modrinth\.com\/(?:mod|plugin|datapack|shader|resourcepack|modpack|project)\/([\w.-]+)\/?(?:[?#].*)?$/i;

const purifier = DOMPurify(window);
purifier.addHook('uponSanitizeAttribute', (_node, data) => {
  if (data.attrName !== 'style') return;
  const kept = data.attrValue.split(';').map((rule) => rule.trim()).filter((rule) => SAFE_STYLE.test(rule));
  data.attrValue = kept.join(';');
  data.keepAttr = kept.length > 0;
});

/** Stores use relative links, and CurseForge wraps outside links in a redirect. */
function resolve(raw: string, base: string): string | null {
  try {
    const url = new URL(raw, base);
    if (url.hostname.endsWith('curseforge.com') && url.pathname === '/linkout') {
      const remote = url.searchParams.get('remoteUrl');
      return remote ? resolve(decodeURIComponent(remote), base) : null;
    }
    return OPENABLE.has(url.protocol) ? url.href : null;
  } catch {
    return null;
  }
}

function imageSource(raw: string, base: string): string | null {
  if (raw.startsWith('data:image/')) return raw;
  try {
    const url = new URL(raw, base);
    if (url.protocol === 'http:') url.protocol = 'https:';
    return url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

function videoId(source: string): string | null {
  try {
    const url = new URL(source, 'https://www.youtube.com');
    const host = url.hostname.replace(/^www\./, '');
    const id = host === 'youtu.be' ? url.pathname.slice(1) : ['youtube.com', 'youtube-nocookie.com'].includes(host) ? /^\/embed\/([^/]+)/.exec(url.pathname)?.[1] : null;
    return id && VIDEO_ID.test(id) ? id : null;
  } catch {
    return null;
  }
}

function prepareImage(image: HTMLImageElement) {
  image.setAttribute('loading', 'lazy');
  image.setAttribute('decoding', 'async');
  image.setAttribute('referrerpolicy', 'no-referrer');
}

/** Videos start as a picture, so a page doesn't load a player for each one. */
function videoPreview(doc: Document, id: string): HTMLElement {
  const link = doc.createElement('a');
  link.className = 'video';
  link.href = `https://www.youtube.com/watch?v=${id}`;
  link.title = 'Play video';
  link.dataset.video = id;
  const picture = doc.createElement('img');
  picture.src = `https://i.ytimg.com/vi/${id}/hqdefault.jpg`;
  picture.alt = '';
  prepareImage(picture);
  link.append(picture);
  return link;
}

function embedLink(doc: Document, source: string, base: string): HTMLElement {
  const paragraph = doc.createElement('p');
  const target = resolve(source, base);
  if (!target) return paragraph;
  const link = doc.createElement('a');
  link.href = target;
  link.title = target;
  link.textContent = `Embedded content from ${new URL(target).hostname}`;
  paragraph.append(link);
  return paragraph;
}

function finish(root: DocumentFragment, base: string) {
  const doc = root.ownerDocument;
  for (const link of root.querySelectorAll('a')) {
    const href = link.getAttribute('href');
    if (!href) continue;
    const target = href.startsWith('#') ? href : resolve(href, base);
    if (!target) {
      link.removeAttribute('href');
      continue;
    }
    link.setAttribute('href', target);
    if (!target.startsWith('#')) link.setAttribute('title', target);
  }
  for (const image of root.querySelectorAll('img')) {
    const source = imageSource(image.getAttribute('src') ?? '', base);
    if (!source) {
      image.remove();
      continue;
    }
    image.setAttribute('src', source);
    prepareImage(image);
  }
  for (const frame of root.querySelectorAll('iframe')) {
    const source = frame.getAttribute('src') ?? '';
    const id = videoId(source);
    frame.replaceWith(id ? videoPreview(doc, id) : embedLink(doc, source, base));
  }
  for (const input of root.querySelectorAll('input')) {
    if (input.getAttribute('type') === 'checkbox') input.setAttribute('disabled', '');
    else input.remove();
  }
}

/** Safe nodes for a body written by a stranger, ready to attach. */
export function renderRich(source: string, format: RichFormat, base: string): DocumentFragment {
  const html = format === 'markdown' ? (marked.parse(source, { gfm: true, async: false }) as string) : source;
  const root = purifier.sanitize(html, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
    RETURN_DOM_FRAGMENT: true,
  });
  finish(root, base);
  return root;
}

export function videoFrame(id: string): HTMLElement | null {
  if (!VIDEO_ID.test(id)) return null;
  const frame = document.createElement('iframe');
  frame.className = 'video';
  frame.src = `https://www.youtube-nocookie.com/embed/${id}?autoplay=1`;
  frame.title = 'Video';
  frame.allow = 'autoplay; encrypted-media; picture-in-picture; fullscreen';
  frame.referrerPolicy = 'strict-origin-when-cross-origin';
  frame.setAttribute('sandbox', 'allow-scripts allow-same-origin allow-presentation');
  return frame;
}

/** The slug of a link to another Modrinth project, which opens inside the app. */
export function modrinthProject(href: string): string | null {
  return PROJECT_LINK.exec(href)?.[1] ?? null;
}

/** Matches a `#section` link to its heading the way GitHub and Modrinth do. */
export function headingAnchor(text: string): string {
  return text.trim().toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu, '').replace(/\s/g, '-');
}

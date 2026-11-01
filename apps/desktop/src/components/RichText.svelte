<script lang="ts">
  import { openExternal } from '../lib/api';
  import { headingAnchor, modrinthProject, renderRich, videoFrame } from '../lib/rich';
  import type { RichFormat } from '../lib/rich';

  const MIN_ZOOM_WIDTH = 96;

  let {
    source,
    format,
    base,
    onimage,
    onproject,
  }: {
    source: string;
    format: RichFormat;
    base: string;
    onimage?: (url: string, title: string) => void;
    onproject?: (slug: string) => void;
  } = $props();

  let element = $state<HTMLElement>();

  $effect(() => {
    element?.replaceChildren(renderRich(source, format, base));
  });

  function scrollToHeading(anchor: string) {
    const wanted = decodeURIComponent(anchor).toLowerCase();
    const headings = element?.querySelectorAll('h1, h2, h3, h4, h5, h6') ?? [];
    [...headings].find((heading) => headingAnchor(heading.textContent ?? '') === wanted)?.scrollIntoView({ block: 'start' });
  }

  function followLink(link: HTMLAnchorElement) {
    const href = link.getAttribute('href');
    if (!href) return;
    if (href.startsWith('#')) return scrollToHeading(href.slice(1));
    if (link.dataset.video) {
      const frame = videoFrame(link.dataset.video);
      if (frame) return link.replaceWith(frame);
    }
    const project = onproject && modrinthProject(href);
    if (project) onproject?.(project);
    else openExternal(href);
  }

  function onclick(event: MouseEvent) {
    const clicked = event.target as HTMLElement;
    const link = clicked.closest('a');
    if (link) {
      event.preventDefault();
      followLink(link);
    } else if (clicked instanceof HTMLImageElement && onimage && clicked.naturalWidth >= MIN_ZOOM_WIDTH) {
      onimage(clicked.src, clicked.alt);
    }
  }
</script>

<!-- Links handle the keyboard themselves, and images are also in the gallery. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="rich selectable" bind:this={element} {onclick}></div>

<style>
  .rich {
    min-width: 0;
    font-size: var(--text-m);
    line-height: 1.62;
    overflow-wrap: break-word;
  }
  .rich :global(> :first-child) { margin-top: 0; }
  .rich :global(> :last-child) { margin-bottom: 0; }

  .rich :global(h1), .rich :global(h2), .rich :global(h3), .rich :global(h4), .rich :global(h5), .rich :global(h6) {
    margin: 26px 0 10px;
    font-weight: 600;
    line-height: 1.3;
  }
  .rich :global(h1) { padding-bottom: 7px; border-bottom: 1px solid var(--line); font-size: 23px; }
  .rich :global(h2) { padding-bottom: 6px; border-bottom: 1px solid var(--line); font-size: 19px; }
  .rich :global(h3) { font-size: 16px; }
  .rich :global(h4) { font-size: var(--text-m); }
  .rich :global(h5), .rich :global(h6) { color: var(--text-2); font-size: var(--text-s); }

  .rich :global(p), .rich :global(ul), .rich :global(ol), .rich :global(dl), .rich :global(pre),
  .rich :global(blockquote), .rich :global(table), .rich :global(details), .rich :global(figure) {
    margin: 0 0 13px;
  }
  .rich :global(ul) { padding-left: 22px; list-style: disc; }
  .rich :global(ol) { padding-left: 24px; list-style: decimal; }
  .rich :global(ul ul) { list-style: circle; }
  .rich :global(li) { margin: 3px 0; }
  .rich :global(li > ul), .rich :global(li > ol) { margin-bottom: 0; }
  .rich :global(dt) { font-weight: 600; }
  .rich :global(dd) { margin: 0 0 6px 18px; }

  .rich :global(a) { color: var(--info); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .rich :global(a:hover) { color: var(--text); }
  .rich :global(a[href]:not([href^='#'])) { word-break: break-word; }
  .rich :global(strong), .rich :global(b) { font-weight: 700; }
  .rich :global(em), .rich :global(i), .rich :global(cite) { font-style: italic; }
  .rich :global(mark) { background: var(--selected); color: inherit; }
  .rich :global(hr) { height: 0; margin: 20px 0; border: 0; border-top: 1px solid var(--line); }

  .rich :global(blockquote) { padding: 1px 0 1px 13px; border-left: 3px solid var(--line-strong); color: var(--text-2); }
  .rich :global(code), .rich :global(kbd), .rich :global(samp), .rich :global(tt) { font-family: var(--mono); font-size: 0.92em; }
  .rich :global(code) { padding: 1px 4px; background: var(--control); border-radius: 2px; }
  .rich :global(pre) { overflow-x: auto; padding: 10px 12px; background: var(--panel); border: 1px solid var(--line); border-radius: 2px; }
  .rich :global(pre code) { padding: 0; background: none; font-size: var(--text-s); }
  .rich :global(kbd) { padding: 1px 5px; background: var(--control); border: 1px solid var(--line-strong); border-radius: 2px; }

  .rich :global(table) { display: block; overflow-x: auto; max-width: 100%; border-collapse: collapse; }
  .rich :global(th), .rich :global(td) { padding: 6px 10px; border: 1px solid var(--line); text-align: left; }
  .rich :global(th) { background: var(--panel); font-weight: 600; }

  .rich :global(img) { max-width: 100%; height: auto; vertical-align: middle; cursor: zoom-in; }
  .rich :global(a img) { cursor: pointer; }
  .rich :global(img[align='left']) { float: left; margin: 0 14px 8px 0; }
  .rich :global(img[align='right']) { float: right; margin: 0 0 8px 14px; }
  .rich :global(center), .rich :global([align='center']) { text-align: center; }
  .rich :global([align='right']) { text-align: right; }

  .rich :global(details) { padding: 7px 11px; border: 1px solid var(--line); border-radius: 2px; }
  .rich :global(summary) { font-weight: 600; cursor: pointer; }
  .rich :global(details[open] > summary) { margin-bottom: 8px; }
  .rich :global(input[type='checkbox']) { margin-right: 6px; vertical-align: -3px; }

  .rich :global(.video) { position: relative; display: block; width: 100%; max-width: 960px; aspect-ratio: 16 / 9; margin: 0 0 13px; border: 0; background: #000; }
  .rich :global(a.video img) { display: block; width: 100%; height: 100%; object-fit: cover; }
  .rich :global(a.video::after) {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: 64px;
    height: 44px;
    margin: -22px 0 0 -32px;
    background: rgb(0 0 0 / 78%) url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 44'%3E%3Cpath d='M26 13l18 9-18 9z' fill='%23fff'/%3E%3C/svg%3E") center / contain no-repeat;
    border-radius: 2px;
  }
  .rich :global(a.video:hover::after) { background-color: rgb(0 0 0 / 92%); }
</style>

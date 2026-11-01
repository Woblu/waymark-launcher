<script lang="ts">
  // Draws the open list of every <select> in the app. They stay real selects,
  // so pages, the keyboard, screen readers and translation work as normal, and
  // only the system's popup is swapped for this one.
  import { tick } from 'svelte';
  import { Check, Search } from '@lucide/svelte';

  interface Item {
    value: string;
    label: string;
    disabled: boolean;
  }

  /** Lists this long get a box to search them. */
  const SEARCH_FROM = 12;
  const ROW = 34;
  const SEARCH_ROW = 42;
  const TALLEST = 340;
  const MARGIN = 8;
  const GAP = 4;

  let select = $state<HTMLSelectElement | null>(null);
  let items = $state<Item[]>([]);
  let query = $state('');
  let at = $state(0);
  let menu = $state<HTMLDivElement>();
  let list = $state<HTMLUListElement>();
  let place = $state({ left: 0, width: 0, height: 0, top: null as number | null, bottom: null as number | null });

  const searchable = $derived(items.length >= SEARCH_FROM);
  const shown = $derived.by(() => {
    const text = query.trim().toLowerCase();
    return text ? items.filter((item) => item.label.toLowerCase().includes(text)) : items;
  });

  function isField(element: Element | null): element is HTMLSelectElement {
    return element instanceof HTMLSelectElement && element.classList.contains('field') && !element.disabled;
  }

  async function open(field: HTMLSelectElement) {
    items = [...field.options].map((option) => ({ value: option.value, label: option.text, disabled: option.disabled }));
    query = '';
    at = Math.max(0, items.findIndex((item) => item.value === field.value));

    const box = field.getBoundingClientRect();
    const wanted = Math.min(TALLEST, items.length * ROW + 8 + (items.length >= SEARCH_FROM ? SEARCH_ROW : 0));
    const [below, above] = [innerHeight - box.bottom - GAP - MARGIN, box.top - GAP - MARGIN];
    const up = below < wanted && above > below;
    const width = Math.max(box.width, 190);
    place = {
      left: Math.min(box.left, innerWidth - width - MARGIN),
      width,
      height: Math.min(wanted, up ? above : below),
      top: up ? null : box.bottom + GAP,
      bottom: up ? innerHeight - box.top + GAP : null,
    };
    select = field;

    await tick();
    const start = menu?.querySelector('input') ?? menu;
    start?.focus();
    list?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'center' });
  }

  function close(refocus = true) {
    const field = select;
    select = null;
    if (refocus) field?.focus();
  }

  function choose(item: Item) {
    if (item.disabled || !select) return;
    const field = select;
    const changed = field.value !== item.value;
    close();
    if (!changed) return;
    field.value = item.value;
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.dispatchEvent(new Event('change', { bubbles: true }));
  }

  function step(by: number) {
    if (!shown.length) return;
    let next = at;
    for (let tries = 0; tries < shown.length; tries++) {
      next = (next + by + shown.length) % shown.length;
      if (!shown[next].disabled) break;
    }
    at = next;
  }

  function onKey(event: KeyboardEvent) {
    const typing = event.key.length === 1 && !event.ctrlKey && !event.metaKey;
    if (event.key === 'ArrowDown') step(1);
    else if (event.key === 'ArrowUp') step(-1);
    else if (event.key === 'Home') at = 0;
    else if (event.key === 'End') at = shown.length - 1;
    else if (event.key === 'Enter' || (event.key === ' ' && !searchable)) {
      const item = shown[at];
      if (item) choose(item);
    } else if (event.key === 'Escape') close();
    else if (event.key === 'Tab') close();
    else if (typing && !searchable) {
      const found = shown.findIndex((item) => item.label.toLowerCase().startsWith(event.key.toLowerCase()));
      if (found >= 0) at = found;
      return;
    } else return;
    if (event.key !== 'Tab') event.preventDefault();
    event.stopPropagation();
  }

  $effect(() => {
    list?.children[at]?.scrollIntoView({ block: 'nearest' });
  });

  $effect(() => {
    query;
    at = 0;
  });

  // Runs before the select sees the press, which is what stops the system's popup.
  function onPress(event: MouseEvent) {
    const target = event.target as Element;
    if (menu?.contains(target)) return;
    const field = target.closest?.('select');
    if (event.button === 0 && isField(field)) {
      event.preventDefault();
      field.focus();
      if (select === field) close();
      else open(field);
    } else if (select) close(false);
  }

  function onField(event: KeyboardEvent) {
    if (select || !isField(document.activeElement)) return;
    const opens = event.key === ' ' || event.key === 'Enter' || event.key === 'F4' || (event.altKey && event.key.startsWith('Arrow'));
    if (!opens) return;
    event.preventDefault();
    event.stopPropagation();
    open(document.activeElement);
  }

  function onScroll(event: Event) {
    if (select && !menu?.contains(event.target as Node)) close(false);
  }
</script>

<svelte:window onmousedowncapture={onPress} onkeydowncapture={onField} onscrollcapture={onScroll} onresize={() => close(false)} onblur={() => close(false)} />

{#if select}
  <div class="float" style:left="{place.left}px" style:top={place.top === null ? null : `${place.top}px`} style:bottom={place.bottom === null ? null : `${place.bottom}px`} style:width="{place.width}px">
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="dropdown" role="listbox" tabindex="-1" aria-label={select.getAttribute('aria-label') ?? undefined} style:max-height="{place.height}px" bind:this={menu} onkeydown={onKey}>
      {#if searchable}
        <label class="find"><Search size={15} /><input placeholder="Search" aria-label="Search" bind:value={query} /></label>
      {/if}
      <ul bind:this={list}>
        {#each shown as item, index (item.value)}
          <li>
            <button tabindex="-1" role="option" aria-selected={item.value === select.value} class:active={index === at} class:chosen={item.value === select.value} disabled={item.disabled} onmousemove={() => (at = index)} onclick={() => choose(item)}>
              <span class="truncate">{item.label}</span>
              {#if item.value === select.value}<Check size={15} strokeWidth={3} />{/if}
            </button>
          </li>
        {:else}
          <li class="none">Nothing found.</li>
        {/each}
      </ul>
    </div>
  </div>
{/if}

<style>
  .float { position: fixed; z-index: 80; filter: drop-shadow(0 10px 22px rgb(0 0 0 / 45%)); }
  .dropdown { --step: 3px; display: flex; flex-direction: column; min-height: 0; padding: 4px; background: var(--raised); clip-path: var(--stepped); outline: none; }
  .find { flex: none; display: flex; align-items: center; gap: 8px; height: 34px; margin-bottom: 4px; padding: 0 9px; background: var(--control); clip-path: var(--stepped); color: var(--text-3); }
  .find input { flex: 1; min-width: 0; border: 0; outline: 0; background: none; color: var(--text); }
  .find input::placeholder { color: var(--text-3); }
  ul { flex: 1; min-height: 0; overflow-y: auto; }
  li button { display: flex; align-items: center; justify-content: space-between; gap: 8px; width: 100%; height: 34px; padding: 0 9px; font-weight: 500; text-align: left; }
  li button.active { background: var(--control); }
  li button.chosen { color: var(--accent); }
  li button:disabled { color: var(--text-3); opacity: 0.6; cursor: not-allowed; }
  li button :global(svg) { flex: none; }
  .none { padding: 8px 9px; color: var(--text-3); }
</style>

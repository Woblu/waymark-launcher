<script lang="ts" module>
  /** Open dialogs, newest last, so only the top one answers the keyboard. */
  const open: HTMLElement[] = [];
</script>

<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Snippet } from 'svelte';
  import { X } from '@lucide/svelte';

  let { title, width = 460, onclose, children }: { title: string; width?: number; onclose: () => void; children: Snippet } = $props();

  const FOCUSABLE = 'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';
  let sheet = $state<HTMLDivElement>();

  function focusables(): HTMLElement[] {
    return [...(sheet?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])].filter((element) => element.offsetParent !== null);
  }

  // Focus moves in when the dialog opens and goes back where it was when it closes.
  // A field to type in comes first; otherwise the dialog itself, so its title is read.
  // The opener is read before anything inside can take focus.
  const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  onMount(() => {
    const me = sheet!;
    open.push(me);
    tick().then(() => {
      if (!sheet || sheet.contains(document.activeElement)) return;
      const field = sheet.querySelector<HTMLElement>('[autofocus], .body input:not([type="checkbox"]):not([type="radio"]):not(:disabled), .body textarea:not(:disabled)');
      (field ?? sheet).focus();
    });
    return () => {
      open.splice(open.indexOf(me), 1);
      const top = open.at(-1);
      if (opener?.isConnected && (!top || top.contains(opener))) opener.focus();
    };
  });

  function onkeydown(event: KeyboardEvent) {
    if (open.at(-1) !== sheet) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose();
      return;
    }
    if (event.key !== 'Tab') return;
    const items = focusables();
    if (!items.length) {
      event.preventDefault();
      sheet?.focus();
      return;
    }
    const [first, last] = [items[0], items[items.length - 1]];
    const inside = sheet?.contains(document.activeElement);
    if (event.shiftKey && (!inside || document.activeElement === first || document.activeElement === sheet)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (!inside || document.activeElement === last)) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop">
  <div class="sheet" role="dialog" aria-modal="true" aria-label={title} tabindex="-1" style:width="{width}px" bind:this={sheet}>
    <div class="head">
      <h2>{title}</h2>
      <button class="close" aria-label="Close" onclick={onclose}><X size={15} /></button>
    </div>
    <div class="body">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgb(0 0 0 / 55%);
  }
  .sheet {
    display: flex;
    flex-direction: column;
    max-width: 100%;
    max-height: 100%;
    --step: 3px;
    background: var(--panel);
    clip-path: var(--stepped);
  }
  .sheet:focus-visible { outline: none; }
  .head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 56px;
    padding: 0 12px 0 20px;
  }
  h2 { font-size: var(--text-l); }
  .close { display: grid; place-items: center; width: 32px; height: 32px; color: var(--text-2); }
  .close:hover { color: var(--text); }
  .close:focus-visible { outline-offset: -2px; }
  .body { overflow-y: auto; padding: 0 20px 20px; }
  .body :global(.form-row) { grid-template-columns: 130px minmax(0, 1fr); }
</style>

<script lang="ts" module>
  export interface SettingsSection {
    id: string;
    label: string;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';

  /**
   * A settings page: its sections listed down the left, the chosen one on the right.
   * Shared by Waymark Launcher's own settings and each instance's.
   */
  let {
    sections,
    selected = $bindable(),
    title,
    children,
  }: { sections: SettingsSection[]; selected: string; title?: string; children: Snippet } = $props();

  let pane = $state<HTMLElement>();

  function choose(id: string) {
    selected = id;
    pane?.scrollTo({ top: 0 });
  }

  /** Arrow keys move through the list, as they do through tabs. */
  function keys(event: KeyboardEvent) {
    const at = sections.findIndex((section) => section.id === selected);
    const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = sections[(at + step + sections.length) % sections.length];
    choose(next.id);
    (event.currentTarget as HTMLElement).querySelector<HTMLButtonElement>(`[data-section="${next.id}"]`)?.focus();
  }
</script>

<div class="layout">
  <nav class="side" aria-label={title ?? 'Settings'}>
    {#if title}<h2 class="title">{title}</h2>{/if}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <ul role="list" onkeydown={keys}>
      {#each sections as section (section.id)}
        <li>
          <button
            data-section={section.id}
            class:active={section.id === selected}
            aria-current={section.id === selected ? 'page' : undefined}
            tabindex={section.id === selected ? 0 : -1}
            onclick={() => choose(section.id)}>{section.label}</button
          >
        </li>
      {/each}
    </ul>
  </nav>
  <div class="pane" bind:this={pane}>
    <div class="column">{@render children()}</div>
  </div>
</div>

<style>
  .layout { display: flex; gap: 28px; height: 100%; min-height: 0; padding: 20px var(--gutter) 0; }
  .side { flex: none; width: 196px; overflow-y: auto; padding-bottom: var(--gutter); }
  .title { margin: 2px 0 12px; font-size: var(--text-l); }
  ul { display: flex; flex-direction: column; gap: 2px; }
  button { position: relative; width: 100%; padding: 9px 14px; clip-path: var(--stepped); color: var(--text-2); font-weight: 550; text-align: left; transition: background-color 100ms ease-out, color 100ms ease-out; }
  button:hover { background: var(--control); color: var(--text); }
  button.active { background: var(--panel); color: var(--text); }
  /* The same accent mark the tabs carry, turned on its side. */
  button.active::before { content: ''; position: absolute; top: 9px; bottom: 9px; left: 0; width: 3px; background: var(--accent); }
  .pane { flex: 1; min-width: 0; overflow-y: auto; padding-bottom: var(--gutter); }
  .column { max-width: 760px; }

  @media (max-width: 760px) {
    .layout { flex-direction: column; gap: 12px; }
    .side { width: auto; overflow: visible; padding-bottom: 0; }
    ul { flex-direction: row; flex-wrap: wrap; }
    button { width: auto; }
  }
</style>

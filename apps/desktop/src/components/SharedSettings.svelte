<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import type { Settings, SharedOption, SharedSettings } from '../lib/api';
  import Modal from './Modal.svelte';

  let { settings = $bindable(), onsave, onclose }: { settings: Settings; onsave: () => Promise<void>; onclose: () => void } = $props();

  let shared = $state<SharedSettings | null>(null);
  let problem = $state('');
  let text = $state('');

  onMount(async () => {
    try {
      shared = await api.sharedSettings();
    } catch (error) {
      problem = (error as { message?: string })?.message ?? String(error);
    }
  });

  const isShared = (option: SharedOption) => settings.sharedChoices?.[option.key] ?? option.shared;
  const matches = (option: SharedOption) => {
    const words = text.trim().toLowerCase();
    return !words || option.key.toLowerCase().includes(words) || option.value.toLowerCase().includes(words);
  };
  /** Keybinds first, since they're what most people want to carry over. */
  const groups = (options: SharedOption[]) => [
    { label: 'Controls', items: options.filter((option) => option.key.startsWith('key_') && matches(option)) },
    { label: 'Everything else', items: options.filter((option) => !option.key.startsWith('key_') && matches(option)) },
  ];

  async function choose(option: SharedOption, on: boolean) {
    settings.sharedChoices = { ...settings.sharedChoices, [option.key]: on };
    await onsave();
  }
</script>

<Modal title="Shared game settings" width={640} {onclose}>
  <p class="muted intro">Ticked settings are the same in every instance that shares. Untick one and each instance keeps its own value for it.</p>
  <input class="field" placeholder="Search settings" aria-label="Search settings" bind:value={text} />

  <div class="sets">
    {#if !shared}
      <p class="faint empty">{problem ? `Could not read the shared settings. ${problem}` : 'Loading'}</p>
    {:else if !shared.sets.length}
      <p class="faint empty">Nothing is shared yet. The first instance you play starts the set.</p>
    {:else}
      {#each shared.sets as set (set.family)}
        <h3>{set.label}</h3>
        {#if set.from}<p class="faint small">{`Started from ${set.from}`}</p>{/if}
        {#each groups(set.options) as group (group.label)}
          {#if group.items.length}
            <h4 class="muted small">{group.label}</h4>
            <ul>
              {#each group.items as option (option.key)}
                <li>
                  <label class="check"><input type="checkbox" checked={isShared(option)} onchange={(event) => choose(option, event.currentTarget.checked)} /><span class="mono key">{option.key}</span></label>
                  <span class="mono faint value truncate" title={option.value}>{option.value}</span>
                </li>
              {/each}
            </ul>
          {/if}
        {/each}
      {/each}
    {/if}
  </div>

  <div class="actions">
    <button class="btn primary" onclick={onclose}>Done</button>
  </div>
</Modal>

<style>
  .intro { margin: 0 0 10px; }
  .sets { max-height: 440px; margin-top: 10px; overflow-y: auto; border-top: 1px solid var(--line); }
  .empty { padding: 14px 0; }
  h3 { margin: 14px 0 2px; font-size: var(--text-m); }
  h4 { margin: 12px 0 4px; font-weight: 500; }
  ul { border-top: 1px solid var(--line); }
  li { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 220px); gap: 12px; align-items: center; min-height: 36px; border-bottom: 1px solid var(--line); }
  .check { display: flex; align-items: center; gap: 8px; min-width: 0; color: var(--text); }
  .key { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .value { text-align: right; }
  .actions { display: flex; justify-content: flex-end; padding-top: 12px; }
</style>

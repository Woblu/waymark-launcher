<script lang="ts">
  import { app } from '../lib/state.svelte';

  const task = $derived(app.tasks[0]);
  const percent = $derived(task && task.total > 1 ? Math.min(100, (task.done / task.total) * 100) : null);
  const running = $derived(app.library.filter((entry) => entry.running));

  let installing = $state(false);

  async function install() {
    installing = true;
    try {
      await app.update?.install();
    } catch (error) {
      app.fail(error, 'Could not install the update');
      installing = false;
    }
  }
</script>

<footer>
  {#if task}
    <span class="truncate" aria-live="polite">{task.title}: {task.detail}{percent === null ? '' : ` (${percent.toFixed(0)}%)`}</span>
    {#if percent !== null}
      <span class="bar"><span style:width="{percent}%"></span></span>
    {/if}
    {#if app.tasks.length > 1}<span>and {app.tasks.length - 1} more</span>{/if}
  {:else if running.length}
    <span class="truncate">{running.map((entry) => entry.name).join(', ')} running</span>
  {:else}
    <span>Ready</span>
  {/if}
  {#if app.update}
    <span class="right">
      Waymark {app.update.version} is available.
      <button disabled={installing} onclick={install}>{installing ? 'Installing' : 'Install and restart'}</button>
    </span>
  {/if}
</footer>

<style>
  footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    height: var(--statusbar);
    padding: 0 14px;
    background: var(--panel);
    border-top: 1px solid var(--line);
    color: var(--text-2);
    font-size: var(--text-xs);
  }
  .right { margin-left: auto; white-space: nowrap; }
  .right button { margin-left: 8px; color: var(--accent); text-decoration: underline; text-underline-offset: 2px; }
  .bar { flex: none; width: 160px; height: 4px; background: var(--control-hover); }
  .bar span { display: block; height: 100%; background: var(--accent); }
</style>

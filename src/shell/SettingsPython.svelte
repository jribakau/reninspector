<script lang="ts">
  import { cancelPython, installPython, py, removePython } from '../lib/pylsp.svelte'
</script>

<div class="setting-row">
  <div class="text">
    <span class="setting-name">ty {py.version || '0.0.84'}</span>
    <p class="setting-desc">
      {#if py.status === 'installing'}
        {py.progress?.label || 'Downloading…'}
        {#if py.progress && py.progress.total > 0}
          ({Math.min(100, Math.round((py.progress.done / py.progress.total) * 100))}%)
        {/if}
      {:else if py.status === 'missing'}
        Not installed. Turning the language server on downloads it into this app's data folder.
      {:else if py.status === 'ready'}
        Running. It reads python blocks and $ lines, using the project's Ren'Py SDK when one is set.
      {:else if py.status === 'starting'}
        Starting…
      {:else if py.detail}
        {py.detail}
      {:else}
        Installed, and idle until the language server is turned on.
      {/if}
    </p>
  </div>
  <div class="ctl">
    {#if py.status === 'installing'}
      <button type="button" onclick={cancelPython}>Cancel</button>
    {:else if !py.installed}
      <button type="button" onclick={() => void installPython(false)}>Install</button>
    {:else}
      <button type="button" onclick={() => void installPython(true)}>Update</button>
      <button type="button" onclick={() => void removePython()}>Remove</button>
    {/if}
  </div>
</div>

<style>
  .ctl {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    justify-content: flex-end;
  }
</style>

<script lang="ts">
  import Icon from '../components/Icon.svelte'
  import { api, errorText } from '../lib/api'
  import { openSdkManager } from '../lib/sdk.svelte'
  import { app, chooseAndOpenProject, createNewProject, openProject } from '../lib/store.svelte'

  async function openDemo() {
    app.error = ''
    try {
      await openProject(await api.openSample())
    } catch (e) {
      app.error = errorText(e)
    }
  }

  function baseName(path: string): string {
    const parts = path.split(/[/\\]/).filter(Boolean)
    return parts[parts.length - 1] ?? path
  }

  const keys: { keys: string[]; label: string }[] = [
    { keys: ['Ctrl', 'P'], label: 'Go to file' },
    { keys: ['Ctrl', 'Shift', 'P'], label: 'Commands' },
    { keys: ['Ctrl', 'T'], label: 'Go to symbol' },
    { keys: ['F5'], label: 'Run live' },
  ]
</script>

<div class="welcome anim-fade">
  <header>
    <h1>Ren'Inspector</h1>
    <p class="dim">Open a Ren'Py game, whether you are writing it or it is already released, and start editing.</p>
  </header>

  <div class="actions">
    <button class="primary big" onclick={chooseAndOpenProject} disabled={!!app.busy}>
      <Icon name="folder" size={15} /> Open project…
    </button>
    <button class="big" onclick={createNewProject} disabled={!!app.busy}><Icon name="plus" size={15} /> New project…</button>
    <button class="big" onclick={() => void openSdkManager()} disabled={!!app.busy}>Ren'Py SDKs…</button>
    <button class="big" onclick={() => void openDemo()} disabled={!!app.busy} title="A small game with a broken jump, a duplicate label and an undefined image">Open the demo</button>
  </div>
  {#if app.busy}<p class="dim">{app.busy}</p>{/if}

  <section class="recent">
    <h2>Recent</h2>
    {#if app.recent.length}
      <ul>
        {#each app.recent as p (p)}
          <li>
            <button class="row" title={p} onclick={() => openProject(p)} disabled={!!app.busy}>
              <Icon name="folder" size={16} />
              <span class="text">
                <span class="base">{baseName(p)}</span>
                <span class="path">{p}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="dim empty">No recent projects yet. Open one and it will show up here.</p>
    {/if}
  </section>

  <ul class="keys" aria-label="Keyboard shortcuts">
    {#each keys as k (k.label)}
      <li>
        <span class="combo">{#each k.keys as key (key)}<kbd class="kbd">{key}</kbd>{/each}</span>
        <span class="dim">{k.label}</span>
      </li>
    {/each}
  </ul>
</div>

<style>
  .welcome {
    margin: auto;
    max-width: 640px;
    width: 100%;
    padding: 24px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-5);
  }
  header {
    text-align: center;
    display: grid;
    gap: var(--sp-3);
  }
  h1 {
    font-size: var(--fs-display);
    font-weight: 650;
    letter-spacing: -0.01em;
    margin: 0;
  }
  h2 {
    margin: 0 0 var(--sp-2);
    font-size: var(--fs-sm);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--sp-3);
  }
  .big {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-lg);
    padding: 7px 16px;
  }
  .recent {
    width: min(460px, 100%);
  }
  .recent ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 2px;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-md);
    padding: 6px 10px;
    color: var(--dim);
  }
  .row:hover:not(:disabled) {
    background: var(--hover);
    border-color: transparent;
    color: var(--text);
  }
  .text {
    display: grid;
    min-width: 0;
  }
  .base {
    font-weight: 600;
    color: var(--text);
  }
  .path {
    color: var(--dim);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dim {
    color: var(--dim);
    font-size: var(--fs-md);
    margin: 0;
  }
  .empty {
    padding: 6px 10px;
  }
  .keys {
    list-style: none;
    margin: var(--sp-3) 0 0;
    padding: var(--sp-4) 0 0;
    border-top: 1px solid var(--line-soft);
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--sp-3) var(--sp-5);
  }
  .keys li {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }
  .combo {
    display: inline-flex;
    gap: 2px;
  }
</style>

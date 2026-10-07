<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import Icon from '../components/Icon.svelte'
  import { app, chooseAndOpenProject, createNewProject, openProject } from '../lib/store.svelte'

  let dropping = $state(false)

  function baseName(path: string): string {
    const parts = path.split(/[/\\]/).filter(Boolean)
    return parts[parts.length - 1] ?? path
  }

  onMount(() => {
    const stops: Array<() => void> = []
    const win = getCurrentWindow()
    void Promise.all([
      win.listen('tauri://drag-enter', () => {
        dropping = true
      }),
      win.listen('tauri://drag-leave', () => {
        dropping = false
      }),
      win.listen<{ paths?: string[] }>('tauri://drag-drop', (event) => {
        dropping = false
        const path = event.payload?.paths?.[0]
        if (path && !app.busy) void openProject(path)
      }),
    ])
      .then((fns) => stops.push(...fns))
      .catch(() => {})
    return () => {
      for (const stop of stops) stop()
    }
  })
</script>

<div class="welcome anim-fade" class:dropping>
  <div class="column">
    <header>
      <div class="mark" aria-hidden="true"><Icon name="map" size={26} /></div>
      <h1>Ren'Inspector</h1>
      <p class="dim">Open a Ren'Py game, whether you are writing it or it is already released, and start editing.</p>
    </header>

    <div class="actions">
      <button class="primary hero" onclick={chooseAndOpenProject} disabled={!!app.busy}>
        <Icon name="folder" size={16} /> Open project…
      </button>
      <button class="ghost" onclick={createNewProject} disabled={!!app.busy}><Icon name="plus" size={15} /> New project…</button>
    </div>
    {#if app.busy}
      <p class="busy"><i class="spinner"></i>{app.busy}</p>
    {/if}

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
  </div>
</div>

<style>
  /* Fills the window so the glow fades out on its own instead of ending at a box edge. */
  .welcome {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    background:
      radial-gradient(ellipse 80% 55% at 50% 0%, color-mix(in srgb, var(--accent) 12%, transparent), transparent 75%);
  }
  .welcome.dropping {
    outline: 2px dashed var(--accent);
    outline-offset: calc(var(--sp-3) * -1);
    border-radius: var(--r-lg);
  }
  .column {
    margin: auto;
    width: min(520px, 100%);
    padding: var(--sp-6) var(--sp-5);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-5);
  }
  header {
    text-align: center;
    display: grid;
    justify-items: center;
    gap: var(--sp-3);
  }
  .mark {
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    border-radius: var(--r-lg);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, var(--panel));
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--line));
    box-shadow: var(--shadow-sm);
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
    align-items: center;
    gap: var(--sp-3);
  }
  .hero {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--fs-xl);
    font-weight: 600;
    padding: var(--sp-3) var(--sp-5);
  }
  .busy {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    margin: 0;
    color: var(--accent);
    font-size: var(--fs-md);
  }
  .busy .spinner {
    width: 12px;
    height: 12px;
    border-width: 2px;
  }
  .recent {
    width: 100%;
  }
  .recent ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: var(--sp-3);
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    text-align: left;
    background: color-mix(in srgb, var(--panel) 35%, transparent);
    border: 1px solid color-mix(in srgb, var(--line) 40%, transparent);
    border-radius: var(--r-md);
    padding: var(--sp-3) var(--sp-4);
    color: var(--dim);
  }
  .row:hover:not(:disabled) {
    background: var(--hover);
    border-color: color-mix(in srgb, var(--line) 70%, transparent);
    color: var(--text);
  }
  .row:active:not(:disabled) {
    background: var(--active);
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
    font-size: var(--fs-md);
    margin: 0;
  }
  .empty {
    padding: var(--sp-3) var(--sp-4);
  }
</style>

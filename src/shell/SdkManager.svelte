<script lang="ts">
  import Modal from '../components/Modal.svelte'
  import { errorText } from '../lib/api'
  import { app } from '../lib/model.svelte'
  import {
    addExistingSdk,
    cancelInstall,
    changeSdkFolder,
    chooseCustomLauncher,
    closeSdkManager,
    installVersion,
    installWeb,
    removeSdk,
    sdk,
    setProjectEngine,
  } from '../lib/sdk.svelte'

  let version = $state('')

  $effect(() => {
    if (!version && sdk.catalog.length) version = sdk.catalog[0]
  })

  const percent = $derived(
    sdk.progress && sdk.progress.total > 0
      ? Math.min(100, Math.round((sdk.progress.done / sdk.progress.total) * 100))
      : null,
  )

  async function run(task: () => Promise<unknown>) {
    try {
      await task()
    } catch (e) {
      app.error = errorText(e)
    }
  }

  function requestClose() {
    if (sdk.installing) return
    closeSdkManager()
  }
</script>

{#if sdk.open}
  <Modal title="Ren'Py SDKs" wide busy={sdk.installing} onclose={requestClose}>
      {#if sdk.prompt}<p class="warn">{sdk.prompt}</p>{/if}

      {#if app.info}
        <section>
          <h4 class="section-label">This project</h4>
          <label>
            <span>Run and check with</span>
            <select
              value={sdk.choice}
              onchange={(e) => void run(() => setProjectEngine(e.currentTarget.value))}
              disabled={sdk.installing}
            >
              <option value="auto">Automatic</option>
              {#if app.info.bundledEngine}
                <option value="bundled">The game's own engine</option>
              {/if}
              {#each sdk.items as item (item.path)}
                <option value={`sdk:${item.path}`}>
                  Ren'Py {item.version ?? item.path}{item.unverified ? ' (unverified)' : ''}
                </option>
              {/each}
              {#if sdk.choice.startsWith('custom:')}
                <option value={sdk.choice}>Custom launcher</option>
              {/if}
            </select>
          </label>
          <button type="button" onclick={() => void run(chooseCustomLauncher)} disabled={sdk.installing}>
            Custom launcher…
          </button>
          {#if sdk.mismatch}
            <p class="warn">
              This SDK is a different Ren'Py release than the project. A newer SDK recompiles scripts and can change
              save compatibility.
            </p>
          {/if}
        </section>
      {/if}

      <section>
        <h4 class="section-label">Installed</h4>
        {#if !sdk.items.length}
          <p class="dim">No SDKs yet. Install one, or add a folder you already have.</p>
        {/if}
        <ul>
          {#each sdk.items as item (item.path)}
            <li>
              <div>
                <strong>Ren'Py {item.version ?? 'unknown'}</strong>
                {#if item.unverified}<span class="tag">unverified</span>{/if}
                {#if item.hasWeb}<span class="tag">web</span>{:else}<span class="tag">no web</span>{/if}
                <div class="path" title={item.path}>{item.path}</div>
              </div>
              <span class="actions">
                {#if app.info}
                  <button type="button" onclick={() => void run(() => setProjectEngine(`sdk:${item.path}`))} disabled={sdk.installing}>
                    Use
                  </button>
                {/if}
                {#if !item.hasWeb}
                  <button type="button" onclick={() => void run(() => installWeb(item.path))} disabled={sdk.installing}>
                    Add Web
                  </button>
                {/if}
                <button type="button" onclick={() => void run(() => removeSdk(item))} disabled={sdk.installing}>Remove</button>
              </span>
            </li>
          {/each}
        </ul>
      </section>

      <section>
        <h4 class="section-label">Install</h4>
        <div class="field-row">
          <select bind:value={version} disabled={sdk.installing || !sdk.catalog.length}>
            {#each sdk.catalog as entry (entry)}
              <option value={entry}>{entry}</option>
            {/each}
          </select>
          <button
            type="button"
            class="primary"
            disabled={sdk.installing || !version}
            onclick={() => void run(() => installVersion(version))}
          >
            Install
          </button>
          {#if sdk.installing}
            <button type="button" onclick={() => void run(cancelInstall)}>Cancel</button>
          {/if}
        </div>
        {#if sdk.catalogError}<p class="warn">{sdk.catalogError}</p>{/if}
        {#if sdk.progress}
          <div class="bar" role="progressbar" aria-valuenow={percent ?? 0} aria-valuemin="0" aria-valuemax="100">
            <span style={`width: ${percent ?? 15}%`}></span>
          </div>
          <p class="dim">{sdk.progress.label}{percent !== null ? ` · ${percent}%` : ''}</p>
        {/if}
      </section>

      <p class="dim folder" title={sdk.folder}>SDK folder: {sdk.folder || '…'}</p>
    {#snippet footer()}
      <button type="button" onclick={() => void run(changeSdkFolder)} disabled={sdk.installing}>Change folder…</button>
      <button type="button" onclick={() => void run(addExistingSdk)} disabled={sdk.installing}>Add existing…</button>
      <button type="button" class="primary" onclick={requestClose} disabled={sdk.installing}>Close</button>
    {/snippet}
  </Modal>
{/if}

<style>
  h4.section-label, p { margin: 0; }
  section { display: grid; gap: var(--sp-3); }
  .dim, .warn { font-size: var(--fs-md); }
  label { display: grid; gap: var(--sp-2); font-size: var(--fs-md); color: var(--dim); }
  select { width: 100%; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-3); }
  li {
    display: flex;
    gap: var(--sp-3);
    justify-content: space-between;
    align-items: center;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    padding: var(--sp-3);
  }
  .path {
    color: var(--dim);
    font-size: var(--fs-sm);
    max-width: 360px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    font-size: var(--fs-xs);
    border: 1px solid var(--line);
    border-radius: var(--r-pill);
    padding: 1px 6px;
    margin-left: var(--sp-2);
    color: var(--dim);
  }
  .actions { display: flex; gap: var(--sp-2); flex: none; }
  .field-row select { flex: 1; }
  .folder {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    height: 8px;
    background: var(--panel-2);
    border-radius: var(--r-sm);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
</style>

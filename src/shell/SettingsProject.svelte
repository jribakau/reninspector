<script lang="ts">
  import { untrack } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import { api, errorText } from '../lib/api'
  import { toggleAutoreload } from '../lib/engine.svelte'
  import { setWatchVars } from '../lib/live.svelte'
  import { app } from '../lib/model.svelte'
  import { openSdkManager, refreshSdks, sdk, setProjectEngine } from '../lib/sdk.svelte'
  import { settingsUi } from '../lib/settings.svelte'

  let watchName = $state('')
  let newWord = $state('')
  let words = $state<string[]>([])

  async function run(task: () => Promise<unknown>) {
    try {
      await task()
    } catch (e) {
      app.error = errorText(e)
    }
  }

  async function addWatch() {
    const name = watchName.trim()
    if (!name) return
    watchName = ''
    await setWatchVars([...app.watchVars, name])
  }

  async function loadWords() {
    try {
      words = await api.spellWords()
    } catch {
      words = []
    }
  }

  async function addWord() {
    const word = newWord.trim()
    if (!word) return
    await run(async () => {
      await api.spellAddWord(word)
      newWord = ''
      await loadWords()
    })
  }

  async function removeWord(word: string) {
    await run(async () => {
      await api.spellRemoveWord(word)
      await loadWords()
    })
  }

  $effect(() => {
    if (settingsUi.open && app.info) {
      void loadWords()
      if (!untrack(() => sdk.items.length)) void refreshSdks().catch(() => {})
    } else {
      words = []
    }
  })
</script>

{#if !app.info}
  <p class="dim empty">Open a project to change its engine, watched variables, and spell words.</p>
{:else}
  <p class="scope">Applies to {app.info.name} only.</p>
  <div class="setting-row">
    <div class="text">
      <label class="setting-name" for="set-engine">Ren'Py engine</label>
      <p class="setting-desc">The version used to run, check, and build this project.</p>
    </div>
    <div class="ctl">
      <select
        id="set-engine"
        value={sdk.choice}
        disabled={sdk.installing}
        onchange={(e) => void run(() => setProjectEngine(e.currentTarget.value))}
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
      <button type="button" onclick={() => void openSdkManager()}>Manage SDKs…</button>
    </div>
  </div>

  <div class="setting-row">
    <div class="text">
      <label class="setting-name" for="set-autoreload">Auto-reload</label>
      <p class="setting-desc">
        Writes game/vnide_autoreload.rpy so the game reloads scripts after a save. Restart the game once to pick it up.
      </p>
    </div>
    <div class="ctl">
      <input id="set-autoreload" type="checkbox" checked={app.autoreload} disabled={!!app.busy} onchange={() => void toggleAutoreload()} />
    </div>
  </div>

  <div class="setting-row stack">
    <div class="text">
      <label class="setting-name" for="set-watch">Watched variables</label>
      <p class="setting-desc">Store names the live game reports back, shown in the Live panel.</p>
    </div>
    <div class="list">
      {#each app.watchVars as name (name)}
        <span class="chip">
          {name}
          <button type="button" class="icon sm ghost" aria-label={`Stop watching ${name}`} onclick={() => void setWatchVars(app.watchVars.filter((n) => n !== name))}>
            <Icon name="close" size={10} />
          </button>
        </span>
      {:else}
        <span class="none">Nothing watched.</span>
      {/each}
    </div>
    <form class="add" onsubmit={(e) => { e.preventDefault(); void addWatch() }}>
      <input id="set-watch" type="text" placeholder="variable_name" bind:value={watchName} />
      <button type="submit" disabled={!watchName.trim()}>Add</button>
    </form>
  </div>

  <div class="setting-row stack">
    <div class="text">
      <label class="setting-name" for="set-word">Spell check words</label>
      <p class="setting-desc">Words this project accepts. They are kept in .vnide/words.txt beside the game folder.</p>
    </div>
    <div class="list">
      {#each words as word (word)}
        <span class="chip">
          {word}
          <button type="button" class="icon sm ghost" aria-label={`Remove ${word}`} onclick={() => void removeWord(word)}>
            <Icon name="close" size={10} />
          </button>
        </span>
      {:else}
        <span class="none">No custom words.</span>
      {/each}
    </div>
    <form class="add" onsubmit={(e) => { e.preventDefault(); void addWord() }}>
      <input id="set-word" type="text" placeholder="Add a word" bind:value={newWord} />
      <button type="submit" disabled={!newWord.trim()}>Add</button>
    </form>
  </div>
{/if}

<style>
  .empty { margin: var(--sp-6) 0; text-align: center; }
  .scope { margin: var(--sp-2) 0 0; color: var(--dim); font-size: var(--fs-md); }
  .ctl { display: flex; align-items: center; gap: var(--sp-3); justify-content: flex-end; }
  .ctl select { min-width: 160px; }
  .list { display: flex; flex-wrap: wrap; gap: var(--sp-2); }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    padding: 1px var(--sp-1) 1px var(--sp-3);
    border: 1px solid var(--line);
    border-radius: var(--r-pill);
    font-size: var(--fs-md);
  }
  .none { color: var(--dim); font-size: var(--fs-md); }
  .add { display: flex; gap: var(--sp-3); max-width: 360px; }
</style>

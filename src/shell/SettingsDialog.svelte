<script lang="ts">
  import { untrack } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import Modal from '../components/Modal.svelte'
  import { api, errorText, pickSettingsOpen, pickSettingsSave } from '../lib/api'
  import { toggleAutoreload } from '../lib/engine.svelte'
  import { setWatchVars } from '../lib/live.svelte'
  import { app } from '../lib/model.svelte'
  import { openSdkManager, refreshSdks, sdk, setProjectEngine } from '../lib/sdk.svelte'
  import {
    CATEGORIES,
    SETTINGS,
    closeSettings,
    exportSettings,
    importSettings,
    isModified,
    resetAll,
    resetLayout,
    resetSetting,
    setSetting,
    settings,
    settingsUi,
    type CategoryId,
    type SettingDef,
    type SettingKey,
  } from '../lib/settings.svelte'

  const LAYOUT_WORDS = 'layout panel panels size sizes reset sidebar explorer splitter width height'
  const PROJECT_WORDS =
    'project engine sdk renpy launcher run autoreload auto-reload reload watch variables spell words dictionary custom'

  let status = $state('')
  let watchName = $state('')
  let newWord = $state('')
  let words = $state<string[]>([])

  const tokens = $derived(
    settingsUi.query
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean),
  )
  const searching = $derived(tokens.length > 0)

  function categoryLabel(id: CategoryId): string {
    return CATEGORIES.find((c) => c.id === id)?.label ?? ''
  }

  function hit(text: string): boolean {
    const hay = text.toLowerCase()
    return tokens.every((t) => hay.includes(t))
  }

  const sections = $derived.by(() => {
    const out: { id: CategoryId; label: string; defs: SettingDef[]; extra: boolean }[] = []
    for (const cat of CATEGORIES) {
      const all = SETTINGS.filter((d) => d.category === cat.id)
      const block = cat.id === 'layout' || cat.id === 'project'
      if (searching) {
        const defs = all.filter((d) => hit(`${d.label} ${d.description} ${d.key} ${cat.label}`))
        const extra =
          (cat.id === 'layout' && hit(`${LAYOUT_WORDS} ${cat.label}`)) ||
          (cat.id === 'project' && !!app.info && hit(`${PROJECT_WORDS} ${cat.label}`))
        if (defs.length || extra) out.push({ id: cat.id, label: cat.label, defs, extra })
      } else if (cat.id === settingsUi.category) {
        out.push({ id: cat.id, label: cat.label, defs: all, extra: block })
      }
    }
    return out
  })

  function pick(id: CategoryId) {
    settingsUi.query = ''
    settingsUi.category = id
    status = ''
  }

  function requestClose() {
    if (settingsUi.query) {
      settingsUi.query = ''
      return
    }
    closeSettings()
  }

  function commit(key: SettingKey, value: unknown) {
    setSetting(key, value as never)
  }

  function disabled(def: SettingDef): boolean {
    return def.key === 'autosaveDelay' && settings.autosave !== 'delay'
  }

  function onNumber(def: SettingDef, e: Event & { currentTarget: HTMLInputElement }) {
    const input = e.currentTarget
    if (input.value.trim() !== '' && Number.isFinite(input.valueAsNumber)) commit(def.key, input.valueAsNumber)
    input.value = String(settings[def.key])
  }

  function onEnum(def: SettingDef, e: Event & { currentTarget: HTMLSelectElement }) {
    const option = def.options?.find((o) => String(o.value) === e.currentTarget.value)
    if (option) commit(def.key, option.value)
  }

  function onText(def: SettingDef, e: Event & { currentTarget: HTMLInputElement }) {
    commit(def.key, e.currentTarget.value)
    e.currentTarget.value = String(settings[def.key])
  }

  async function exportFile() {
    status = ''
    try {
      const path = await pickSettingsSave()
      if (!path) return
      await api.settingsExport(path, exportSettings())
      status = `Exported to ${path}`
    } catch (e) {
      status = errorText(e)
    }
  }

  async function importFile() {
    status = ''
    try {
      const path = await pickSettingsOpen()
      if (!path) return
      const result = importSettings(await api.settingsImport(path))
      const skipped = result.skipped ? `, ${result.skipped} skipped` : ''
      status = `Imported ${result.applied} setting${result.applied === 1 ? '' : 's'}${skipped}`
    } catch (e) {
      status = errorText(e)
    }
  }

  function restoreDefaults() {
    if (!confirm('Reset every setting to its default?')) return
    resetAll()
    status = 'All settings are back to their defaults.'
  }

  function clearSizes() {
    resetLayout()
    status = 'Panel sizes are back to their defaults.'
  }

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

{#snippet settingRow(def: SettingDef)}
  {@const value = settings[def.key]}
  {@const changed = isModified(def.key)}
  {@const off = disabled(def)}
  <div class="row" class:changed class:off>
    <div class="text">
      <label class="name" for={`set-${def.key}`}>{def.label}</label>
      <p class="desc">{def.description}</p>
    </div>
    <div class="ctl">
      {#if def.type === 'bool'}
        <input
          id={`set-${def.key}`}
          type="checkbox"
          checked={value as boolean}
          onchange={(e) => commit(def.key, e.currentTarget.checked)}
        />
      {:else if def.type === 'number'}
        <input
          id={`set-${def.key}`}
          class="num"
          type="number"
          min={def.min}
          max={def.max}
          step={def.step}
          value={value as number}
          disabled={off}
          onchange={(e) => onNumber(def, e)}
        />
        {#if def.unit}<span class="unit">{def.unit}</span>{/if}
      {:else if def.type === 'enum'}
        <select id={`set-${def.key}`} onchange={(e) => onEnum(def, e)}>
          {#each def.options ?? [] as option (option.value)}
            <option value={String(option.value)} selected={option.value === value}>{option.label}</option>
          {/each}
        </select>
      {:else}
        <input
          id={`set-${def.key}`}
          type="text"
          class="wide"
          placeholder={def.placeholder}
          value={value as string}
          onchange={(e) => onText(def, e)}
        />
      {/if}
      <button
        type="button"
        class="icon sm ghost reset"
        aria-label={`Reset ${def.label}`}
        title="Reset to default"
        disabled={!changed}
        onclick={() => resetSetting(def.key)}
      >
        <Icon name="undo" size={12} />
      </button>
    </div>
  </div>
{/snippet}

{#snippet layoutBlock()}
  <div class="row">
    <div class="text">
      <span class="name">Panel sizes</span>
      <p class="desc">
        Put the sidebar, flow, stage, bottom panel, and explorer sections back to their default sizes. Panels
        you dragged wider or taller return to normal.
      </p>
    </div>
    <div class="ctl">
      <button type="button" onclick={clearSizes}>Reset panel sizes</button>
    </div>
  </div>
{/snippet}

{#snippet projectBlock()}
  {#if !app.info}
    <p class="empty">Open a project to change its engine, watched variables, and spell words.</p>
  {:else}
    <p class="scope">Applies to {app.info.name} only.</p>
    <div class="row">
      <div class="text">
        <label class="name" for="set-engine">Ren'Py engine</label>
        <p class="desc">The version used to run, check, and build this project.</p>
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

    <div class="row">
      <div class="text">
        <label class="name" for="set-autoreload">Auto-reload</label>
        <p class="desc">
          Writes game/vnide_autoreload.rpy so the game reloads scripts after a save. Restart the game once to pick
          it up.
        </p>
      </div>
      <div class="ctl">
        <input
          id="set-autoreload"
          type="checkbox"
          checked={app.autoreload}
          disabled={!!app.busy}
          onchange={() => void toggleAutoreload()}
        />
      </div>
    </div>

    <div class="row stack">
      <div class="text">
        <label class="name" for="set-watch">Watched variables</label>
        <p class="desc">Store names the live game reports back, shown in the Live panel.</p>
      </div>
      <div class="list">
        {#each app.watchVars as name (name)}
          <span class="chip">
            {name}
            <button
              type="button"
              class="icon sm ghost"
              aria-label={`Stop watching ${name}`}
              onclick={() => void setWatchVars(app.watchVars.filter((n) => n !== name))}
            >
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

    <div class="row stack">
      <div class="text">
        <label class="name" for="set-word">Spell check words</label>
        <p class="desc">Words this project accepts. They are kept in .vnide/words.txt beside the game folder.</p>
      </div>
      <div class="list">
        {#each words as word (word)}
          <span class="chip">
            {word}
            <button
              type="button"
              class="icon sm ghost"
              aria-label={`Remove ${word}`}
              onclick={() => void removeWord(word)}
            >
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
{/snippet}

{#if settingsUi.open}
  <Modal title="Settings" size="xl" flush onclose={requestClose}>
    <div class="top">
      <Icon name="search" size={14} />
      <input
        type="text"
        class="find"
        placeholder="Search settings"
        aria-label="Search settings"
        bind:value={settingsUi.query}
      />
      {#if settingsUi.query}
        <button type="button" class="icon sm ghost" aria-label="Clear search" onclick={() => (settingsUi.query = '')}>
          <Icon name="close" size={12} />
        </button>
      {/if}
    </div>
    <div class="split">
      <nav class="cats" aria-label="Settings categories">
        {#each CATEGORIES as cat (cat.id)}
          <button
            type="button"
            class="cat"
            class:on={!searching && settingsUi.category === cat.id}
            aria-current={!searching && settingsUi.category === cat.id ? 'page' : undefined}
            onclick={() => pick(cat.id)}
          >
            {cat.label}
          </button>
        {/each}
      </nav>
      <div class="content">
        {#each sections as section (section.id)}
          <section>
            <h4>{section.label}</h4>
            {#each section.defs as def (def.key)}
              {@render settingRow(def)}
            {/each}
            {#if section.extra}
              {#if section.id === 'layout'}
                {@render layoutBlock()}
              {:else}
                {@render projectBlock()}
              {/if}
            {/if}
          </section>
        {:else}
          <p class="empty">No settings match "{settingsUi.query}".</p>
        {/each}
      </div>
    </div>
    {#snippet footer()}
      {#if status}<span class="status" title={status}>{status}</span>{/if}
      <button type="button" onclick={restoreDefaults}>Reset all</button>
      <button type="button" onclick={() => void importFile()}>Import…</button>
      <button type="button" onclick={() => void exportFile()}>Export…</button>
      <button type="button" class="primary" onclick={closeSettings}>Close</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--line-soft);
    color: var(--dim);
    flex: none;
  }
  .find {
    flex: 1;
    border: none;
    background: transparent;
    padding: 4px 0;
  }
  .find:focus {
    outline: none;
  }
  .split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 188px 1fr;
  }
  .cats {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 8px;
    border-right: 1px solid var(--line-soft);
    overflow-y: auto;
  }
  .cat {
    text-align: left;
    background: transparent;
    border-color: transparent;
    color: var(--dim);
    padding: 6px 10px;
  }
  .cat:hover:not(:disabled) {
    background: var(--hover);
    border-color: transparent;
    color: var(--text);
  }
  .cat.on {
    background: var(--active);
    color: var(--text);
  }
  .content {
    overflow-y: auto;
    padding: 6px 18px 18px;
    min-width: 0;
  }
  section {
    display: grid;
  }
  h4 {
    margin: 12px 0 4px;
    font-size: var(--fs-md);
    color: var(--dim);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 16px;
    align-items: center;
    padding: 10px 0 10px 10px;
    border-bottom: 1px solid var(--line-soft);
  }
  .row.changed {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .row.off .text {
    opacity: 0.55;
  }
  .row.stack {
    grid-template-columns: minmax(0, 1fr);
    align-items: start;
    gap: 8px;
  }
  .name {
    font-weight: 600;
    color: var(--text);
  }
  .desc {
    margin: 2px 0 0;
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }
  .ctl select {
    min-width: 160px;
  }
  .num {
    width: 88px;
  }
  .wide {
    width: 240px;
  }
  .unit {
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .reset {
    flex: none;
  }
  .list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px 2px 1px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-pill);
    font-size: var(--fs-md);
  }
  .none {
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .add {
    display: flex;
    gap: 8px;
    max-width: 360px;
  }
  .scope {
    margin: 6px 0 0;
    color: var(--dim);
    font-size: var(--fs-md);
  }
  .empty {
    margin: 24px 0;
    color: var(--dim);
    text-align: center;
  }
  .status {
    margin-right: auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--dim);
    font-size: var(--fs-md);
  }
</style>

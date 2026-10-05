<script lang="ts">
  import Icon from '../components/Icon.svelte'
  import Modal from '../components/Modal.svelte'
  import SettingRow from './SettingRow.svelte'
  import SettingsProject from './SettingsProject.svelte'
  import SettingsPython from './SettingsPython.svelte'
  import { api, errorText, pickSettingsOpen, pickSettingsSave } from '../lib/api'
  import { app } from '../lib/model.svelte'
  import {
    CATEGORIES,
    SETTINGS,
    closeSettings,
    exportSettings,
    importSettings,
    resetAll,
    resetLayout,
    settingsUi,
    type CategoryId,
    type SettingDef,
  } from '../lib/settings.svelte'

  const LAYOUT_WORDS = 'layout panel panels size sizes reset sidebar explorer splitter width height'
  const PROJECT_WORDS =
    'project engine sdk renpy launcher run autoreload auto-reload reload watch variables spell words dictionary custom'

  let status = $state('')
  const tokens = $derived(
    settingsUi.query
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean),
  )
  const searching = $derived(tokens.length > 0)

  function hit(text: string): boolean {
    const hay = text.toLowerCase()
    return tokens.every((t) => hay.includes(t))
  }

  const sections = $derived.by(() => {
    const out: { id: CategoryId; label: string; defs: SettingDef[]; extra: boolean }[] = []
    for (const cat of CATEGORIES) {
      const all = SETTINGS.filter((d) => d.category === cat.id)
      const block = cat.id === 'layout' || cat.id === 'project' || cat.id === 'editor'
      if (searching) {
        const defs = all.filter((d) => hit(`${d.label} ${d.description} ${d.key} ${cat.label}`))
        const extra =
          (cat.id === 'layout' && hit(`${LAYOUT_WORDS} ${cat.label}`)) ||
          (cat.id === 'project' && !!app.info && hit(`${PROJECT_WORDS} ${cat.label}`)) ||
          (cat.id === 'editor' && hit(`python language server ty install ${cat.label}`))
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

</script>

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
            <h4 class="section-label">{section.label}</h4>
            {#each section.defs as def (def.key)}
              <SettingRow {def} />
            {/each}
            {#if section.extra}
              {#if section.id === 'layout'}
                <div class="setting-row">
                  <div class="text">
                    <span class="setting-name">Panel sizes</span>
                    <p class="setting-desc">
                      Put the sidebar, flow, stage, bottom panel, and explorer sections back to their default sizes. Panels
                      you dragged wider or taller return to normal.
                    </p>
                  </div>
                  <div class="ctl">
                    <button type="button" onclick={clearSizes}>Reset panel sizes</button>
                  </div>
                </div>
              {:else if section.id === 'editor'}
                <SettingsPython />
              {:else}
                <SettingsProject />
              {/if}
            {/if}
          </section>
        {:else}
          <p class="dim empty">No settings match "{settingsUi.query}".</p>
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
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-5);
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
    gap: var(--sp-1);
    padding: var(--sp-3);
    border-right: 1px solid var(--line-soft);
    overflow-y: auto;
  }
  .cat {
    text-align: left;
    background: transparent;
    border-color: transparent;
    color: var(--dim);
    padding: var(--sp-2) var(--sp-3);
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
    padding: var(--sp-2) var(--sp-5) var(--sp-5);
    min-width: 0;
  }
  section {
    display: grid;
  }
  h4.section-label {
    margin-bottom: var(--sp-2);
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    justify-content: flex-end;
  }
  .empty {
    margin: var(--sp-6) 0;
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

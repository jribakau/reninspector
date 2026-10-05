<script lang="ts">
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { api, errorText } from '../lib/api'
  import { openContextMenu, placeItems } from '../lib/context.svelte'
  import { app, goTo } from '../lib/store.svelte'
  import { notify } from '../lib/toast.svelte'
  import type { TranslationEntry } from '../lib/types'
  import VirtualList from './VirtualList.svelte'

  let lang = $state('')
  let rows = $state<TranslationEntry[]>([])
  let err = $state('')
  let filter = $state('')
  let saving = $state<Record<string, boolean>>({})

  function rowKey(row: TranslationEntry): string {
    return `${row.path}:${row.line}:${row.kind}`
  }

  const languages = $derived(app.catalog?.languages ?? [])

  $effect(() => {
    const list = languages
    if (!list.length) return
    if (lang && list.some((l) => l.lang === lang)) return
    void load(list[0].lang)
  })

  async function load(next: string) {
    lang = next
    err = ''
    try {
      rows = await api.translations(next)
    } catch (e) {
      err = errorText(e)
    }
  }

  const matched = $derived.by(() => {
    const q = filter.trim().toLowerCase()
    return rows.filter((r) => !q || r.source.toLowerCase().includes(q) || r.translated.toLowerCase().includes(q))
  })
  const shown = $derived(matched.slice(0, 300))
  const capNote = $derived.by(() => {
    const loadedStop = rows.length >= 2000
    if (matched.length > shown.length) {
      const stop = loadedStop ? ' The loaded list stops at 2000.' : ''
      return `Showing ${shown.length.toLocaleString()} of ${matched.length.toLocaleString()}.${stop}`
    }
    if (loadedStop && !filter.trim()) return 'Showing 2000 translations. The list stops there.'
    if (loadedStop) return 'The loaded list stops at 2000.'
    return ''
  })

  async function save(row: TranslationEntry, text: string) {
    if (text === row.translated) return
    const key = rowKey(row)
    saving = { ...saving, [key]: true }
    err = ''
    try {
      await api.updateTranslation(row.path, row.line, text)
      row.translated = text
      rows = [...rows]
      notify(`Updated ${row.path}:${row.line}`, 'ok')
    } catch (e) {
      err = errorText(e)
    } finally {
      const next = { ...saving }
      delete next[key]
      saving = next
    }
  }
</script>

<div class="sb-panel">
  <div class="sb-tools">
    <div class="sb-chips">
      {#each languages as l (l.lang)}
        <button class="sb-chip" class:on={lang === l.lang} onclick={() => load(l.lang)}>
          {l.lang} {l.translated}/{l.total}
        </button>
      {/each}
      {#if !languages.length}<span class="sb-meta">No translate blocks in the readable scripts.</span>{/if}
    </div>
    {#if lang}<FilterInput placeholder="Filter this language…" bind:value={filter} />{/if}
  </div>
  {#if err}<div class="sb-foot">{err}</div>{/if}
  {#if capNote}<div class="sb-foot">{capNote}</div>{/if}
  <div class="sb-list virtual">
    {#if lang && !shown.length}
      <div class="sb-empty">No translations match.</div>
    {:else}
      <VirtualList items={shown} rowHeight={ROW.xxl} itemKey={rowKey}>
        {#snippet row(entry)}
          <div class="pair">
            <button
              class="src"
              onclick={() => goTo(entry.path, entry.line)}
              oncontextmenu={(e) => openContextMenu(e, placeItems(entry.path, entry.line, entry.source || entry.translated))}
            >
              {entry.kind === 'strings' ? entry.source : entry.translated || '(dialogue block)'}
            </button>
            <input
              value={entry.translated}
              disabled={!!saving[rowKey(entry)]}
              onchange={(e) => save(entry, e.currentTarget.value)}
            />
            <span class="sb-meta">{entry.path}:{entry.line}</span>
          </div>
        {/snippet}
      </VirtualList>
    {/if}
  </div>
</div>

<style>
  .pair {
    display: grid;
    gap: 3px;
    padding: 4px 12px;
    box-sizing: border-box;
    height: 100%;
    border-left: 3px solid transparent;
    align-content: center;
  }
  .pair:hover {
    background: var(--hover);
  }
  .pair:focus-within {
    background: var(--sel);
    border-left-color: var(--accent);
  }
  .src {
    text-align: left;
    border: none;
    background: transparent;
    color: var(--dim);
    padding: 0;
    font-size: var(--fs-md);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .src:hover {
    color: var(--text);
    border-color: transparent;
  }
</style>

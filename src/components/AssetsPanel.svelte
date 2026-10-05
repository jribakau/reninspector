<script lang="ts">
  import EmptyState from './EmptyState.svelte'
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { onDestroy } from 'svelte'
  import { api, errorText, readAsset } from '../lib/api'
  import { copyText, openContextMenu, placeItems } from '../lib/context.svelte'
  import { app, goTo } from '../lib/store.svelte'
  import type { AssetReport } from '../lib/types'
  import VirtualList from './VirtualList.svelte'

  let report = $state<AssetReport | null>(null)
  let err = $state('')
  let filter = $state('')
  let only = $state<'all' | 'unused' | 'missing'>('all')
  let preview = $state('')
  let generation = 0
  let seenRoot = ''

  async function load() {
    const root = app.info?.root
    if (!root) {
      report = null
      return
    }
    const seq = ++generation
    err = ''
    try {
      const next = await api.assetReport()
      if (seq !== generation || app.info?.root !== root) return
      report = next
    } catch (e) {
      if (seq !== generation) return
      err = errorText(e)
    }
  }

  $effect(() => {
    const root = app.info?.root ?? ''
    void app.changeSeq
    if (!root) {
      seenRoot = ''
      report = null
      return
    }
    if (root !== seenRoot) seenRoot = root
    void load()
  })

  const rows = $derived.by(() => {
    const files = report?.files ?? []
    const q = filter.trim().toLowerCase()
    return files.filter((f) => {
      if (only === 'unused' && f.used) return false
      if (only === 'missing') return false
      return !q || f.path.toLowerCase().includes(q)
    })
  })

  const missing = $derived.by(() => {
    const list = report?.missing ?? []
    const q = filter.trim().toLowerCase()
    return list.filter((m) => !q || m.path.toLowerCase().includes(q) || m.file.toLowerCase().includes(q) || m.what.toLowerCase().includes(q))
  })

  onDestroy(() => {
    if (preview.startsWith('blob:')) URL.revokeObjectURL(preview)
  })

  async function show(path: string) {
    if (!/\.(png|jpe?g|webp|gif|avif)$/i.test(path)) {
      preview = ''
      return
    }
    try {
      const buf = await readAsset(path)
      if (preview.startsWith('blob:')) URL.revokeObjectURL(preview)
      preview = URL.createObjectURL(new Blob([buf]))
    } catch (e) {
      err = errorText(e)
    }
  }
</script>

<div class="sb-panel">
  <div class="sb-tools">
    <FilterInput placeholder="Filter assets…" bind:value={filter} />
    <div class="sb-chips">
      <button class="sb-chip" class:on={only === 'all'} onclick={() => (only = 'all')}>All {report?.files.length ?? 0}</button>
      <button class="sb-chip" class:on={only === 'unused'} onclick={() => (only = 'unused')}>Unused {report?.unused ?? 0}</button>
      <button class="sb-chip" class:on={only === 'missing'} onclick={() => (only = 'missing')}>Missing {report?.missing.length ?? 0}</button>
      <button class="sb-chip" onclick={load}>Refresh</button>
    </div>
    {#if report && report.filesTotal > report.files.length}
      <div class="cap">Showing {report.files.length.toLocaleString()} of {report.filesTotal.toLocaleString()} files.</div>
    {/if}
    {#if report && report.missingTotal > report.missing.length}
      <div class="cap">Showing {report.missing.length.toLocaleString()} of {report.missingTotal.toLocaleString()} missing.</div>
    {/if}
  </div>
  {#if err}<div class="sb-foot">{err}</div>{/if}
  {#if preview}<img class="preview" src={preview} alt="" />{/if}
  <div class="sb-list virtual">
    {#if !report && !err}
      <div class="sb-empty" role="status">
        Scanning assets…
        <div class="skeleton"></div>
        <div class="skeleton"></div>
        <div class="skeleton"></div>
      </div>
    {:else if only === 'missing' && missing.length === 0 && !err}
      <EmptyState tone="ok" icon="check" title="No missing assets" hint="Every image and sound the scripts name is in the game folder." />
    {:else if only !== 'missing' && rows.length === 0 && !err}
      <EmptyState
        icon="image"
        title="No assets match"
        action={filter.trim() || only !== 'all' ? 'Clear filter' : undefined}
        onaction={() => { filter = ''; only = 'all' }}
      />
    {:else if only === 'missing'}
      <VirtualList items={missing} rowHeight={ROW.lg}>
        {#snippet row(m)}
          <button class="sb-item" onclick={() => goTo(m.file, m.line)} oncontextmenu={(e) => openContextMenu(e, placeItems(m.file, m.line, m.path))}>
            <span class="sb-name">{m.path}</span>
            <span class="sb-meta">{m.what} · {m.file}:{m.line}</span>
          </button>
        {/snippet}
      </VirtualList>
    {:else}
      <VirtualList items={rows} rowHeight={ROW.lg}>
        {#snippet row(f)}
          <button
            class="sb-item"
            onclick={() => show(f.path)}
            oncontextmenu={(e) =>
              openContextMenu(e, [
                { kind: 'item', label: 'Show', run: () => void show(f.path) },
                { kind: 'item', label: 'Copy path', run: () => copyText(f.path) },
              ])}
          >
            <span class="sb-name">{f.path}{#if !f.used}<em class="sb-tag warn">unused</em>{/if}</span>
            <span class="sb-meta">{f.kind} · {f.bytes.toLocaleString()} bytes</span>
          </button>
        {/snippet}
      </VirtualList>
    {/if}
  </div>
</div>

<style>
  .cap {
    font-size: var(--fs-sm);
    color: var(--dim);
    padding: 0 4px;
  }
  .preview {
    max-width: 100%;
    max-height: 140px;
    object-fit: contain;
    background: var(--bg);
    flex: none;
  }
</style>

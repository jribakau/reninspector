<script lang="ts">
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { onDestroy } from 'svelte'
  import VirtualList from './VirtualList.svelte'
  import { api, errorText, readArchiveEntry } from '../lib/api'
  import { copyText, openContextMenu } from '../lib/context.svelte'
  import { app, bakePatch, buildArchive, cancelArchiveJob, extractArchive, removePatch, undoPatch } from '../lib/store.svelte'
  import type { ArchiveEntryInfo, ArchiveInfo } from '../lib/types'

  let selected = $state('')
  let entries = $state<ArchiveEntryInfo[]>([])
  let filter = $state('')
  let picked = $state<string[]>([])
  let listError = $state('')
  let previewName = $state('')
  let previewText = $state('')
  let previewUrl = $state('')

  const archives = $derived(app.info?.archives ?? [])
  const overrides = $derived(app.info?.files.filter((f) => f.origin === 'override') ?? [])
  const packageCount = $derived.by(() => {
    const names = new Set<string>()
    for (const file of overrides) names.add(file.path)
    for (const rel of app.decompiledEdits) names.add(rel)
    return names.size
  })
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase()
    return entries.filter((e) => !q || e.name.toLowerCase().includes(q))
  })

  $effect(() => {
    const path = selected
    if (!path) {
      entries = []
      return
    }
    let cancelled = false
    void api.archiveList(path).then(
      (list) => {
        if (!cancelled) {
          entries = list
          listError = ''
        }
      },
      (e) => {
        if (!cancelled) {
          entries = []
          listError = errorText(e)
        }
      },
    )
    return () => {
      cancelled = true
    }
  })

  function choose(archive: ArchiveInfo) {
    if (selected !== archive.path) picked = []
    selected = archive.path
    clearPreview()
  }

  function togglePick(name: string) {
    picked = picked.includes(name) ? picked.filter((n) => n !== name) : [...picked, name]
  }

  onDestroy(clearPreview)

  function clearPreview() {
    if (previewUrl) URL.revokeObjectURL(previewUrl)
    previewUrl = ''
    previewText = ''
    previewName = ''
  }

  async function preview(entry: ArchiveEntryInfo) {
    if (!selected) return
    clearPreview()
    previewName = entry.name
    try {
      const bytes = await readArchiveEntry(selected, entry.name)
      if (entry.kind === 'image') {
        previewUrl = URL.createObjectURL(new Blob([bytes]))
      } else if (entry.kind === 'script' || entry.kind === 'other') {
        previewText = new TextDecoder('utf-8', { fatal: false }).decode(bytes.slice(0, 6000))
      } else {
        previewText = `${entry.kind} · ${formatBytes(entry.len)}`
      }
    } catch (e) {
      previewText = errorText(e)
    }
  }

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
    return `${(n / (1024 * 1024)).toFixed(1)} MB`
  }

  function badge(archive: ArchiveInfo): string {
    if (archive.error) return 'unreadable'
    if (archive.isPatch) return 'patch'
    if (archive.scripts) return `${archive.scripts} scripts`
    if (archive.compiledOnly) return 'compiled only'
    return `${archive.entries} files`
  }
</script>

<div class="sb-panel arch">
  <div class="sb-tools row">
    <button onclick={buildArchive} disabled={!!app.busy}>Build archive…</button>
    {#if app.busy}
      <button onclick={cancelArchiveJob}>Cancel</button>
    {/if}
  </div>

  <div class="note">
    <div>Package edits (optional)</div>
    {#if packageCount}
      {packageCount} saved edit{packageCount === 1 ? '' : 's'} can be packed into a patch for distributing.
      The game already loads them as loose files.
    {:else}
      Nothing to package. A saved edit is already a loose file the game loads.
    {/if}
    <div class="row">
      <button class="primary" onclick={bakePatch} disabled={!!app.busy || !packageCount}>
        Package edits into a patch
      </button>
      <button onclick={undoPatch} disabled={!!app.busy}>Undo</button>
      <button onclick={removePatch} disabled={!!app.busy || !archives.some((a) => a.isPatch)}>
        Remove patch
      </button>
    </div>
  </div>

  {#if !archives.length}
    <p class="sb-empty">This project has no .rpa archives. Scripts on disk are all there is to edit.</p>
  {/if}

  {#each archives as archive (archive.path)}
    <button class="sb-item arc" class:sel={selected === archive.path} onclick={() => choose(archive)}>
      <span class="sb-name">
        {archive.path}
        {#if archive.isPatch}<em class="sb-tag ok">patch</em>{/if}
        {#if archive.nested}<em class="sb-tag" title="Ren'Py 7 only loads archives at the top of game/">nested</em>{/if}
      </span>
      <span class="sb-meta">
        {archive.error ? archive.error : `${archive.version || 'archive'} · ${badge(archive)} · ${formatBytes(archive.otherBytes)} other`}
        {#if archive.overrides} · {archive.overrides} overriding{/if}
        {#if archive.stale.length} · stale{/if}
      </span>
    </button>
    {#if archive.stale.length && selected === archive.path}
      <p class="warn">{archive.stale[0]}{archive.stale.length > 1 ? ` (+${archive.stale.length - 1} more)` : ''}</p>
    {/if}
  {/each}

  {#if selected && !archives.find((a) => a.path === selected)?.error}
    <div class="sb-tools row">
      <FilterInput placeholder="Filter entries…" bind:value={filter} />
      <button onclick={() => extractArchive(selected, null)} disabled={!!app.busy}>Extract all…</button>
      <button onclick={() => extractArchive(selected, picked)} disabled={!!app.busy || !picked.length}>
        Extract selected{picked.length ? ` (${picked.length})` : ''}…
      </button>
    </div>
    {#if listError}<p class="warn">{listError}</p>{/if}
    <div class="list">
      <VirtualList items={shown} rowHeight={ROW.md}>
        {#snippet row(entry)}
          <div class="sb-item pick">
            <input
              type="checkbox"
              checked={picked.includes(entry.name)}
              onchange={() => togglePick(entry.name)}
              aria-label={`Select ${entry.name}`}
            />
            <button
              class="pick-main"
              onclick={() => preview(entry)}
              title={entry.name}
              oncontextmenu={(e) =>
                openContextMenu(e, [
                  { kind: 'item', label: 'Preview', run: () => preview(entry) },
                  { kind: 'item', label: 'Copy name', run: () => copyText(entry.name) },
                ])}
            >
              <span class="sb-name">{entry.name}</span>
              <span class="sb-meta">{entry.kind} · {formatBytes(entry.len)}</span>
            </button>
          </div>
        {/snippet}
      </VirtualList>
    </div>
    {#if previewName}
      <div class="preview">
        <div class="sb-meta">{previewName}</div>
        {#if previewUrl}<img src={previewUrl} alt="" />{/if}
        {#if previewText}<pre>{previewText}</pre>{/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .arch {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    overflow: auto;
  }
  .row {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .note {
    margin: 0 8px 8px;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    font-size: var(--fs-md);
    display: grid;
    gap: 6px;
  }
  .arc {
    flex: none;
    height: auto;
  }
  .pick {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    height: 100%;
    box-sizing: border-box;
  }
  .pick-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    border: none;
    background: transparent;
    text-align: left;
    padding: 0;
  }
  .list {
    min-height: 140px;
    flex: 1;
  }
  .warn {
    padding: 8px 10px;
    font-size: var(--fs-md);
    color: var(--dim);
  }
  .warn {
    color: var(--warning);
  }
  .preview {
    border-top: 1px solid var(--line);
    padding: 8px;
    max-height: 180px;
    overflow: auto;
  }
  .preview img {
    max-width: 100%;
    max-height: 140px;
  }
  pre {
    margin: 0;
    white-space: pre-wrap;
    font-size: var(--fs-sm);
    font-family: var(--mono);
  }
</style>

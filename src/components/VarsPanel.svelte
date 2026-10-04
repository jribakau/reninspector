<script lang="ts">
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { api, errorText } from '../lib/api'
  import { copyText, labelItems, openContextMenu, placeItems } from '../lib/context.svelte'
  import { app, goTo, openLabelGraph, renameSymbol, searchDialogue } from '../lib/store.svelte'
  import type { Route } from '../lib/types'
  import VirtualList from './VirtualList.svelte'

  let filter = $state('')
  let routes = $state<Route[]>([])
  let err = $state('')

  const rows = $derived.by(() => {
    const q = filter.trim().toLowerCase()
    return (app.catalog?.variables ?? []).filter((v) => !q || v.name.toLowerCase().includes(q))
  })

  const stats = $derived(app.catalog?.dialogue)
  const extra = $derived(
    !!stats || !!app.catalog?.endings.length || !!app.catalog?.deadEnds.length || !!app.selectedLabel,
  )

  async function paths() {
    if (!app.selectedLabel) return
    err = ''
    try {
      routes = await api.labelRoutes(app.selectedLabel)
    } catch (e) {
      err = errorText(e)
    }
  }
</script>

<div class="sb-panel">
  <div class="sb-tools">
    <FilterInput placeholder="Filter variables…" bind:value={filter} />
    {#if stats}
      <div class="sb-meta">{stats.lines.toLocaleString()} dialogue lines · {stats.words.toLocaleString()} words · about {stats.minutes} min</div>
    {/if}
  </div>
  <div class="sb-list virtual">
    <VirtualList items={rows} rowHeight={ROW.lg}>
      {#snippet row(v)}
        <button
          class="sb-item"
          onclick={() => goTo(v.path, v.line)}
          ondblclick={() => renameSymbol('variable', v.name)}
          oncontextmenu={(e) =>
            openContextMenu(e, [
              ...placeItems(v.path, v.line, v.name),
              { kind: 'item', label: 'Rename', run: () => renameSymbol('variable', v.name) },
            ])}
        >
          <span class="sb-name">{v.name}{#if v.persistent}<em class="sb-tag">persistent</em>{/if}{#if v.uses === 0}<em class="sb-tag warn">unused</em>{/if}</span>
          <span class="sb-meta">{v.keyword} · {v.uses} uses · {v.path}:{v.line}</span>
        </button>
      {/snippet}
    </VirtualList>
  </div>
  {#if !app.catalog}
    <div class="sb-empty">Open a project to list its variables.</div>
  {:else if rows.length === 0}
    <div class="sb-empty">No variables match.</div>
  {/if}
  {#if extra}
    <div class="sb-list extra">
      {#if stats}
        <div class="sb-foot">Speakers</div>
        {#each stats.speakers as s (s.name)}
          <button
            class="sb-item"
            onclick={() => searchDialogue(s.name)}
            oncontextmenu={(e) =>
              openContextMenu(e, [
                { kind: 'item', label: 'Find dialogue', run: () => searchDialogue(s.name) },
                { kind: 'item', label: 'Copy name', run: () => copyText(s.name) },
              ])}
          >
            <span class="sb-name">{s.name}</span>
            <span class="sb-meta">{s.lines} lines · {s.words} words</span>
          </button>
        {/each}
      {/if}
      {#if app.catalog?.endings.length}
        <div class="sb-foot">Endings</div>
        {#each app.catalog.endings as name, i (`end:${i}:${name}`)}
          <button class="sb-item" onclick={() => openLabelGraph(name)} oncontextmenu={(e) => openContextMenu(e, labelItems(name))}>
            <span class="sb-name">{name}</span>
          </button>
        {/each}
      {/if}
      {#if app.catalog?.deadEnds.length}
        <div class="sb-foot">Dead ends</div>
        {#each app.catalog.deadEnds as name, i (`dead:${i}:${name}`)}
          <button class="sb-item" onclick={() => openLabelGraph(name)} oncontextmenu={(e) => openContextMenu(e, labelItems(name))}>
            <span class="sb-name">{name}</span>
          </button>
        {/each}
      {/if}
      {#if app.selectedLabel}
        <div class="sb-foot"><button class="sb-link" onclick={paths}>Paths to {app.selectedLabel}</button></div>
        {#each routes as r, i (i)}
          <div class="sb-meta route">
            {#each r.steps as step, j (j)}
              {#if j > 0}<span> → </span>{/if}
              <button class="sb-link" onclick={() => openLabelGraph(step)} oncontextmenu={(e) => openContextMenu(e, labelItems(step))}>{step}</button>
            {/each}
          </div>
        {/each}
      {/if}
      {#if err}<div class="sb-foot">{err}</div>{/if}
    </div>
  {/if}
</div>

<style>
  .route {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    align-items: center;
    padding: 2px 12px;
    white-space: normal;
  }
</style>

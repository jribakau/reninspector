<script lang="ts">
  import FilterInput from './FilterInput.svelte'
  import { ROW } from '../lib/view'
  import { labelItems, openContextMenu } from '../lib/context.svelte'
  import { app, fileOfNode, openLabelGraph, openMapTab, selectLabel } from '../lib/store.svelte'
  import type { MapNode } from '../lib/types'
  import VirtualList from './VirtualList.svelte'

  let filter = $state('')
  let labelFilter = $state<'all' | 'screens' | 'unreachable' | 'duplicate' | 'menus'>('all')

  const labels = $derived.by<MapNode[]>(() => {
    const q = filter.trim().toLowerCase()
    return (app.map?.nodes ?? []).filter((n) => {
      if (n.kind === 'missing') return false
      if (labelFilter === 'unreachable' && n.reachable) return false
      if (labelFilter === 'duplicate' && !n.duplicate) return false
      if (labelFilter === 'menus' && n.kind !== 'menu') return false
      if (labelFilter === 'screens' && n.kind !== 'screen') return false
      return !q || n.id.toLowerCase().includes(q)
    })
  })

  const selectedIndex = $derived(app.selectedLabel ? labels.findIndex((n) => n.id === app.selectedLabel) : -1)

  function fileOf(n: MapNode): string {
    return fileOfNode(n) ?? ''
  }
</script>

<div class="sb-panel">
  <div class="sb-head">
    <h2>Story</h2>
    <button class="mini" onclick={openMapTab}>Project map</button>
  </div>
  <div class="sb-tools">
    <FilterInput placeholder="Search labels…" bind:value={filter} />
    <div class="sb-chips">
      {#each [['all', 'All'], ['screens', 'Screens'], ['unreachable', 'Unreachable'], ['duplicate', 'Duplicates'], ['menus', 'Named menus']] as [id, label] (id)}
        <button class="sb-chip" class:on={labelFilter === id} onclick={() => (labelFilter = id as typeof labelFilter)}>
          {label}
        </button>
      {/each}
    </div>
  </div>
  <div class="sb-list virtual">
    {#if !app.map}
      <div class="sb-empty">Open a project to list its labels.</div>
    {:else if !labels.length}
      <div class="sb-empty">No labels match.</div>
    {:else}
    <VirtualList items={labels} rowHeight={ROW.md} reveal={selectedIndex >= 0 ? selectedIndex : null}>
      {#snippet row(n)}
        <button
          class="sb-item"
          class:sel={app.selectedLabel === n.id}
          onclick={() => selectLabel(n.id)}
          ondblclick={() => openLabelGraph(n.id)}
          oncontextmenu={(e) => {
            app.selectedLabel = n.id
            openContextMenu(e, labelItems(n.id))
          }}
          title="Click: preview source · Double-click: preview flow"
        >
          <span class="sb-name">
            {n.kind === 'screen' ? n.id.slice(7) : n.id}
            {#if n.kind === 'menu'}<em class="sb-tag tag-menu">menu</em>{/if}
            {#if n.kind === 'screen'}<em class="sb-tag screen">screen</em>{/if}
            {#if n.kind === 'compiled'}<em class="sb-tag" title="Known from the engine dump; no readable source">compiled</em>{/if}
            {#if n.indirect}<em class="sb-tag" title="Only reached through a label name kept as data">indirect</em>{/if}
            {#if n.root}<em class="sb-tag ok">entry</em>{/if}
            {#if !n.reachable}<em class="sb-tag">unreachable</em>{/if}
            {#if n.duplicate}<em class="sb-tag warn">duplicate</em>{/if}
          </span>
          <span class="sb-meta">
            {#if n.file >= 0}{fileOf(n)}:{n.line} · {n.says} lines · {n.outDegree} out{:else}{n.outDegree} out · no source{/if}
          </span>
        </button>
      {/snippet}
    </VirtualList>
    {/if}
  </div>
</div>

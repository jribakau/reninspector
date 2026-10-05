<script lang="ts">
  import { labelItems, openContextMenu } from '../lib/context.svelte'
  import { app, goTo, outlineOf, selectLabel, type OutlineEntry } from '../lib/store.svelte'
  import Icon from './Icon.svelte'

  interface Props {
    open: boolean
    grow: boolean
    height?: number
    ontoggle: () => void
    list?: HTMLDivElement | null
  }

  let { open, grow, height, ontoggle, list = $bindable(null) }: Props = $props()

  const outline = $derived(app.loc ? outlineOf(app.loc.file) : [])
  const fileName = $derived(app.loc?.file.split('/').pop() ?? '')

  function openOutline(n: OutlineEntry) {
    if (n.kind === 'transform' || n.kind === 'style') {
      const file = app.loc?.file
      if (file) goTo(file, n.line, n.endLine)
      app.selectedLabel = n.id
      return
    }
    selectLabel(n.id)
  }
</script>

<div class="outline" class:grow style={height !== undefined ? `height:${height}px` : undefined}>
  <button class="sb-section" aria-expanded={open} onclick={ontoggle}>
    <span class="twist" class:open><Icon name="chevron-right" size={12} /></span>
    <span class="sb-section-name">Outline{fileName ? ` · ${fileName}` : ''}</span>
    {#if outline.length}<span class="badge">{outline.length}</span>{/if}
  </button>
  {#if open}
    <div class="sb-list" bind:this={list}>
      {#each outline as n (n.id)}
        <button
          class="sb-item"
          data-id={n.id}
          class:sel={app.selectedLabel === n.id}
          onclick={() => openOutline(n)}
          oncontextmenu={(e) => {
            app.selectedLabel = n.id
            if (n.kind === 'transform' || n.kind === 'style') return
            openContextMenu(e, labelItems(n.id))
          }}
        >
          <span class="sb-name">
            {n.name}
            {#if n.kind !== 'label'}<em class="sb-tag" class:screen={n.kind === 'screen'} class:tag-menu={n.kind === 'menu'}>{n.kind}</em>{/if}
          </span>
          <span class="sb-meta">line {n.line}</span>
        </button>
      {:else}
        <div class="sb-empty">Open a script to list its labels, screens, transforms, and styles.</div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .outline {
    flex: none;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .outline.grow {
    flex: 1 1 0;
  }
</style>

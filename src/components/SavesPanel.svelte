<script lang="ts">
  import { api, errorText } from '../lib/api'
  import { app } from '../lib/store.svelte'
  import type { SaveDetail, SaveNode, SaveSlot } from '../lib/types'

  let slots = $state<SaveSlot[]>([])
  let errorMsg = $state('')
  let loading = $state(false)
  let selected = $state('')
  let detail = $state<SaveDetail | null>(null)
  let deep = $state(false)

  $effect(() => {
    const root = app.info?.root
    slots = []
    detail = null
    selected = ''
    errorMsg = ''
    if (!root) return
    let cancel = false
    loading = true
    void api.saveList().then(
      (list) => {
        if (!cancel) {
          slots = list
          loading = false
        }
      },
      (e) => {
        if (!cancel) {
          errorMsg = errorText(e)
          loading = false
        }
      },
    )
    return () => {
      cancel = true
    }
  })

  async function open(slot: SaveSlot, withTree: boolean) {
    selected = slot.path
    deep = withTree
    errorMsg = ''
    try {
      detail = await api.saveInspect(slot.path, withTree)
    } catch (e) {
      detail = null
      errorMsg = errorText(e)
    }
  }
</script>

{#snippet node(n: SaveNode, depth: number)}
  <div class="node" style:padding-left="{depth * 12}px">
    <span class="sb-name">{n.name}</span>
    <span class="sb-meta">{n.kind} · {n.repr}</span>
  </div>
  {#each n.children as child (`${child.name}:${child.kind}:${child.repr}`)}
    {@render node(child, depth + 1)}
  {/each}
{/snippet}

<div class="sb-panel saves">
  {#if loading}<span class="sb-meta">Reading saves…</span>{/if}
  {#if errorMsg}<p class="warn">{errorMsg}</p>{/if}
  {#if !loading && !slots.length}
    <p class="sb-empty">No saves in game/saves or the user save folder.</p>
  {/if}
  {#each slots as slot (slot.path)}
    <button class="sb-item" class:sel={selected === slot.path} onclick={() => open(slot, deep)}>
      <span class="sb-name">{slot.name}</span>
      <span class="sb-meta">
        {slot.persistent ? 'persistent' : slot.extra || 'save'}
        {#if slot.version} · Ren'Py {slot.version}{/if}
        {#if slot.modified} · {slot.modified}{/if}
      </span>
    </button>
  {/each}

  {#if detail}
    <div class="note">
      <div class="row">
        <span>{detail.slot.name}</span>
        <button type="button" onclick={() => open(detail!.slot, !deep)}>
          {deep ? 'Hide store' : 'Read store'}
        </button>
      </div>
      {#if detail.slot.extra}<span class="sb-meta">{detail.slot.extra}</span>{/if}
      {#if detail.screenshotBase64}
        <img alt="Save screenshot" src="data:image/png;base64,{detail.screenshotBase64}" />
      {/if}
      {#if detail.note}<p class="warn">{detail.note}</p>{/if}
      {#if detail.tree}
        {@render node(detail.tree, 0)}
      {/if}
    </div>
  {/if}
</div>

<style>
  .saves {
    overflow: auto;
    padding: 4px 0 12px;
  }
  .note {
    margin: 8px;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-md);
    display: grid;
    gap: 6px;
    font-size: var(--fs-md);
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  img {
    max-width: 100%;
    height: auto;
    border-radius: var(--r-sm);
  }
  .node {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .warn {
    margin: 0 8px 8px;
    color: var(--warning);
    font-size: var(--fs-md);
  }
</style>

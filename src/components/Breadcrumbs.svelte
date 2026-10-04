<script lang="ts">
  import Icon from './Icon.svelte'

  interface Crumb {
    part: string
    path: string
    ellipsis: boolean
  }

  interface Props {
    path: string
    label?: string | null
    onsegment: (path: string) => void
    onlabel?: () => void
  }

  let { path, label = null, onsegment, onlabel }: Props = $props()

  let expanded = $state(false)

  // A different file starts collapsed again.
  $effect(() => {
    void path
    expanded = false
  })

  const parts = $derived(path.split('/').filter(Boolean))
  const crumbs = $derived.by(() => {
    const all: Crumb[] = parts.map((part, i) => ({
      part,
      path: parts.slice(0, i + 1).join('/'),
      ellipsis: false,
    }))
    if (all.length <= 3 || expanded) return all
    return [all[0], { part: '…', path: '', ellipsis: true }, all[all.length - 1]]
  })
</script>

<div class="crumb" title={path}>
  {#each crumbs as seg, i (seg.path || `gap-${i}`)}
    {#if i > 0}<span class="sep"><Icon name="chevron-right" size={11} /></span>{/if}
    {#if seg.ellipsis}
      <button class="seg more" title="Show the full path" aria-label="Show the full path" onclick={() => (expanded = true)}>…</button>
    {:else}
      <button class="seg" class:last={i === crumbs.length - 1 && !label} onclick={() => onsegment(seg.path)}>
        {#if i === crumbs.length - 1}<Icon name="file" size={12} />{/if}
        <span class="txt">{seg.part}</span>
      </button>
    {/if}
  {/each}
  {#if label}
    <span class="sep"><Icon name="chevron-right" size={11} /></span>
    <button class="label" onclick={() => onlabel?.()}>
      <Icon name="flow" size={12} />
      <span class="txt">{label}</span>
    </button>
  {/if}
</div>

<style>
  .crumb {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 26px;
    padding: 0 var(--sp-4);
    border-bottom: 1px solid var(--line-soft);
    background: var(--panel);
    color: var(--dim);
    font-size: var(--fs-md);
    flex: none;
    overflow: hidden;
    white-space: nowrap;
    min-width: 0;
  }
  .sep {
    opacity: 0.55;
    flex: none;
    display: inline-grid;
  }
  .seg,
  .label {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: none;
    background: transparent;
    color: var(--dim);
    padding: 1px 4px;
    border-radius: var(--r-sm);
    min-width: 0;
  }
  .txt {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .seg:hover,
  .label:hover {
    color: var(--text);
    background: var(--hover);
    border-color: transparent;
  }
  .seg.last {
    color: var(--text);
  }
  .label {
    color: var(--text);
    flex: none;
  }
</style>

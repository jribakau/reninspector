<script lang="ts">
  import { applog, clearApplog, markEventsSeen } from '../lib/applog.svelte'
  import { api, errorText } from '../lib/api'
  import { copyText, openContextMenu } from '../lib/context.svelte'
  import { app } from '../lib/store.svelte'
  import EmptyState from './EmptyState.svelte'

  let level = $state('all')
  let query = $state('')
  let err = $state('')
  let box: HTMLDivElement | undefined = $state()
  let near = $state(true)

  const levels = ['all', 'info', 'warn', 'error']

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase()
    return applog.items.filter((entry) => {
      if (level !== 'all' && entry.level !== level) return false
      if (!q) return true
      return entry.message.toLowerCase().includes(q) || entry.source.toLowerCase().includes(q)
    })
  })

  function clock(ts: number): string {
    const d = new Date(ts)
    const p = (n: number) => String(n).padStart(2, '0')
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
  }

  function lineOf(entry: (typeof shown)[number]): string {
    const times = entry.count > 1 ? ` ×${entry.count}` : ''
    return `${clock(entry.tsMs)} ${entry.level} ${entry.source} ${entry.message}${times}`
  }

  function onScroll() {
    if (!box) return
    near = box.scrollHeight - box.scrollTop - box.clientHeight < 48
  }

  async function openFolder() {
    err = ''
    try {
      const path = await api.applogPath()
      if (path) await api.revealPath(path)
    } catch (e) {
      err = errorText(e)
    }
  }

  $effect(() => {
    if (app.bottomOpen && app.bottomTab === 'events') markEventsSeen()
  })

  $effect(() => {
    void applog.items.length
    void shown.length
    if (!near || !box) return
    queueMicrotask(() => {
      if (box) box.scrollTop = box.scrollHeight
    })
  })
</script>

<div class="sb-panel">
  <div class="sb-tools row">
    <button onclick={() => void copyText(shown.map(lineOf).join('\n'))} disabled={!shown.length}>Copy all</button>
    <button onclick={() => void clearApplog()} disabled={!applog.items.length}>Clear</button>
    <button onclick={() => void openFolder()}>Open log folder</button>
    {#each levels as name (name)}
      <button class="sb-chip" class:on={level === name} onclick={() => (level = name)}>{name}</button>
    {/each}
    <input class="filter" placeholder="Filter" bind:value={query} />
  </div>
  {#if err}<div class="sb-foot">{err}</div>{/if}
  <div class="sb-list" bind:this={box} onscroll={onScroll}>
    {#if !shown.length}
      <EmptyState
        icon="history"
        title={applog.items.length ? 'Nothing matches' : 'No events yet'}
        hint={applog.items.length ? 'Try another level or filter.' : "Opens, saves, builds and errors in Ren'Inspector."}
      />
    {/if}
    {#each shown as entry (entry.seq)}
      <div
        class="sb-item log"
        role="group"
        oncontextmenu={(e) => openContextMenu(e, [{ kind: 'item', label: 'Copy', run: () => copyText(lineOf(entry)) }])}
      >
        <span class="text">{entry.message}</span>
        <span class="sb-meta">
          <span class="when">{clock(entry.tsMs)}</span>
          <span class="lvl {entry.level}">{entry.level}</span>
          {entry.source}
          {#if entry.count > 1}<span>×{entry.count}</span>{/if}
        </span>
      </div>
    {/each}
  </div>
</div>

<style>
  .log {
    height: auto;
    padding: 4px 12px;
    user-select: text;
  }
  .text {
    font-family: var(--mono);
    font-size: var(--fs-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    cursor: text;
  }
  .when {
    font-variant-numeric: tabular-nums;
  }
  .lvl {
    text-transform: uppercase;
    font-size: var(--fs-xs);
  }
  .lvl.error { color: var(--error); }
  .lvl.warn { color: var(--warning); }
  .filter {
    flex: 1;
    min-width: 8em;
    margin-left: 4px;
  }
</style>

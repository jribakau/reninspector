<script lang="ts">
  import { onMount } from 'svelte'
  import { api, errorText } from '../lib/api'
  import { copyText, openContextMenu, placeItems } from '../lib/context.svelte'
  import { goTo, runGame } from '../lib/store.svelte'
  import EmptyState from './EmptyState.svelte'
  import type { LogLine } from '../lib/types'

  let lines = $state<LogLine[]>([])
  let err = $state('')
  let source = $state('all')
  let box: HTMLDivElement | undefined = $state()
  let near = $state(true)
  let timer: ReturnType<typeof setInterval> | null = null

  const sources = $derived([...new Set(lines.map((line) => line.source))])
  const shown = $derived(source === 'all' ? lines : lines.filter((line) => line.source === source))

  function plain(text: string): string {
    return text.replace(/\u001b\[[0-9;]*m/g, '')
  }

  function onScroll() {
    if (!box) return
    near = box.scrollHeight - box.scrollTop - box.clientHeight < 48
  }

  async function load() {
    try {
      lines = (await api.readLogs()).map((line) => ({ ...line, text: plain(line.text) }))
      err = ''
      if (near) queueMicrotask(() => { if (box) box.scrollTop = box.scrollHeight })
    } catch (e) {
      err = errorText(e)
    }
  }

  onMount(() => {
    void load()
    timer = setInterval(() => void load(), 2000)
    return () => {
      if (timer) clearInterval(timer)
    }
  })
</script>

<div class="sb-panel">
  <div class="sb-tools row">
    <button onclick={load}>Refresh logs</button>
    <button onclick={() => void copyText(shown.map((line) => line.text).join('\n'))} disabled={!shown.length}>Copy all</button>
    {#each ['all', ...sources] as name (name)}
      <button class="sb-chip" class:on={source === name} onclick={() => (source = name)}>{name}</button>
    {/each}
  </div>
  {#if err}<div class="sb-foot">{err}</div>{/if}
  <div class="sb-list" bind:this={box} onscroll={onScroll}>
    {#if !shown.length && !err}
      <EmptyState icon="history" title="No log output" hint="Run the game to fill log.txt." action="Run game" onaction={() => void runGame()} />
    {/if}
    {#each shown as line, i (`${line.source}:${i}`)}
      {#if line.path && line.line}
        <div
          class="sb-item log"
          role="group"
          oncontextmenu={(e) => openContextMenu(e, placeItems(line.path!, line.line!, line.text))}
        >
          <span class="text">{line.text}</span>
          <span class="sb-meta">
            {line.source} ·
            <button class="sb-link" onclick={() => goTo(line.path!, line.line!)}>{line.path}:{line.line}</button>
          </span>
        </div>
      {:else}
        <div class="sb-item log" role="group" oncontextmenu={(e) => openContextMenu(e, [{ kind: 'item', label: 'Copy', run: () => copyText(line.text) }])}>
          <span class="text">{line.text}</span>
          <span class="sb-meta">{line.source}</span>
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .log {
    height: auto;
    padding: 4px 12px;
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
  .log {
    user-select: text;
  }
</style>

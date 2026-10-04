<script lang="ts">
  import { buildUi, clearBuildLog, showBuildFolder, stopBuild } from '../lib/build.svelte'

  let box: HTMLDivElement | undefined = $state()
  let near = $state(true)

  const shown = $derived(buildUi.lines.length > 800 ? buildUi.lines.slice(-800) : buildUi.lines)
  const hidden = $derived(buildUi.lines.length - shown.length)
  const percent = $derived(
    buildUi.progress && buildUi.progress.total > 0
      ? Math.min(100, Math.round((buildUi.progress.cur / buildUi.progress.total) * 100))
      : null,
  )

  function onScroll() {
    if (!box) return
    near = box.scrollHeight - box.scrollTop - box.clientHeight < 48
  }

  $effect(() => {
    void buildUi.rev
    if (!near || !box) return
    queueMicrotask(() => {
      if (box) box.scrollTop = box.scrollHeight
    })
  })
</script>

<div class="sb-panel">
  <div class="sb-tools row">
    <button onclick={() => void stopBuild()} disabled={!buildUi.running}>{buildUi.status === 'Stopping…' ? 'Stopping…' : 'Stop'}</button>
    <button onclick={() => void showBuildFolder()} title="Show the destination folder">Show folder</button>
    <button onclick={clearBuildLog} disabled={buildUi.running}>Clear</button>
    <span class="sb-meta">{buildUi.status}</span>
  </div>
  {#if buildUi.running && percent !== null}
    <div class="bar" role="progressbar" aria-valuenow={percent} aria-valuemin="0" aria-valuemax="100">
      <span style={`width:${percent}%`}></span>
    </div>
  {/if}
  <div class="log" bind:this={box} onscroll={onScroll}>
    {#if !shown.length}
      <div class="sb-empty">Build output shows up here.</div>
    {/if}
    {#if hidden > 0}<div class="sb-foot">{hidden} earlier lines are hidden.</div>{/if}
    {#each shown as line, i (hidden + i)}
      <div class="line" class:err={line.stream === 'err'}>{line.text}</div>
    {/each}
  </div>
</div>

<style>
  .log {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 6px 12px 10px;
    font-family: var(--mono);
    font-size: var(--fs-sm);
    line-height: 1.5;
    user-select: text;
  }
  .line { white-space: pre-wrap; overflow-wrap: anywhere; }
  .err { color: var(--error); }
  .bar {
    height: 4px;
    background: var(--panel-2);
    flex: none;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width var(--dur) var(--ease);
  }
</style>

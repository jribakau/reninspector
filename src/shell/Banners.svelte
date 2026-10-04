<script lang="ts">
  import { slide } from 'svelte/transition'
  import Icon from '../components/Icon.svelte'
  import { app, goTo, impactSummary, openRenpy, selectLabel } from '../lib/store.svelte'
  import type { Diagnostic } from '../lib/types'

  const impact = $derived(app.impact)
  const impactBad = $derived(!!impact?.added.some((d) => d.severity === 'error'))

  let hidden = $state<string[]>([])
  let hiddenFor = ''

  $effect(() => {
    const root = app.info?.root ?? ''
    if (root === hiddenFor) return
    hiddenFor = root
    hidden = []
  })

  function hide(id: string) {
    if (!hidden.includes(id)) hidden = [...hidden, id]
  }

  function showImpact(d: Diagnostic) {
    if (d.path) goTo(d.path, d.line)
    if (d.label) selectLabel(d.label, { reveal: false })
  }

  const archiveErrors = $derived(app.info?.archives.filter((a) => a.error).length ?? 0)
</script>

{#snippet dismiss(run: () => void)}
  <button type="button" class="icon sm ghost" aria-label="Dismiss" onclick={run}><Icon name="close" size={13} /></button>
{/snippet}

<div class="stack">
  {#if impact && !impactSummary(impact).startsWith('Nothing else')}
    <div class="banner" class:err={impactBad} class:warn={!impactBad} role={impactBad ? 'alert' : 'status'} transition:slide={{ duration: 140 }}>
      <Icon name={impactBad ? 'error' : 'warning'} size={14} />
      <span class="msg">{impactSummary(impact)}</span>
      {#if impact.added[0]?.path}
        <button class="sm" onclick={() => showImpact(impact.added[0])}>Show</button>
      {/if}
      {@render dismiss(() => (app.impact = null))}
    </div>
  {/if}

  {#if app.error}
    <div class="banner err" role="alert" transition:slide={{ duration: 140 }}>
      <Icon name="error" size={14} />
      <span class="msg">{app.error}</span>
      {@render dismiss(() => (app.error = ''))}
    </div>
  {/if}

  {#if app.info && app.info.compiledOnly.length && !hidden.includes('compiled')}
    <div class="banner warn" role="status" transition:slide={{ duration: 140 }}>
      <Icon name="warning" size={14} />
      <span class="msg">
        {app.info.compiledOnly.length} compiled script{app.info.compiledOnly.length === 1 ? '' : 's'} could not be
        decompiled, so {app.info.compiledOnly.length === 1 ? 'it is' : 'they are'} not on the map.
      </span>
      {@render dismiss(() => hide('compiled'))}
    </div>
  {/if}
  {#if archiveErrors && !hidden.includes('archives')}
    <div class="banner warn" role="status" transition:slide={{ duration: 140 }}>
      <Icon name="warning" size={14} />
      <span class="msg">
        {archiveErrors} archive{archiveErrors === 1 ? '' : 's'} could not be read.
        <button class="link" onclick={() => openRenpy('archives')}>Show</button>
      </span>
      {@render dismiss(() => hide('archives'))}
    </div>
  {/if}
  {#if app.info?.engine?.stale && !hidden.includes('stale')}
    <div class="banner warn" role="status" transition:slide={{ duration: 140 }}>
      <Icon name="info" size={14} />
      <span class="msg">Scripts changed since the engine check; re-run it to refresh.</span>
      {@render dismiss(() => hide('stale'))}
    </div>
  {/if}
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    flex: none;
    max-height: 30vh;
    overflow: auto;
  }
  .stack:empty {
    display: none;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: 4px 8px 4px 12px;
    flex: none;
    font-size: var(--fs-md);
  }
  .msg {
    flex: 1;
    min-width: 0;
  }
  .banner.err {
    background: color-mix(in srgb, var(--error) 16%, var(--bg));
    border-bottom: 1px solid color-mix(in srgb, var(--error) 55%, transparent);
    color: var(--text);
  }
  .banner.err > :global(svg:first-child) {
    color: var(--error);
  }
  .banner.warn {
    background: color-mix(in srgb, var(--warning) 12%, var(--bg));
    border-bottom: 1px solid color-mix(in srgb, var(--warning) 50%, transparent);
  }
  .banner.warn > :global(svg:first-child) {
    color: var(--warning);
  }
  .link {
    background: none;
    border: none;
    padding: 0 4px;
    color: var(--accent);
    cursor: pointer;
    text-align: left;
  }
  .link:hover {
    text-decoration: underline;
    border-color: transparent;
  }
</style>

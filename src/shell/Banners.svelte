<script lang="ts">
  import { slide } from 'svelte/transition'
  import Icon from '../components/Icon.svelte'
  import { copyText } from '../lib/context.svelte'
  import { app, checkWithEngine, goTo, impactSummary, openRenpy, selectLabel } from '../lib/store.svelte'
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
    <div class="alert" class:err={impactBad} class:warn={!impactBad} role={impactBad ? 'alert' : 'status'} transition:slide={{ duration: 140 }}>
      <Icon name={impactBad ? 'error' : 'warning'} size={14} />
      <span class="msg">{impactSummary(impact)}</span>
      {#if impact.added[0]?.path}
        <button class="sm" onclick={() => showImpact(impact.added[0])}>Show</button>
      {/if}
      {@render dismiss(() => (app.impact = null))}
    </div>
  {/if}

  {#if app.error}
    <div class="alert err" role="alert" transition:slide={{ duration: 140 }}>
      <Icon name="error" size={14} />
      <span class="msg">{app.error}</span>
      <button type="button" class="sm" onclick={() => copyText(app.error)}>Copy</button>
      {@render dismiss(() => (app.error = ''))}
    </div>
  {/if}

  {#if app.info && app.info.compiledOnly.length && !hidden.includes('compiled')}
    <div class="alert warn" role="status" transition:slide={{ duration: 140 }}>
      <Icon name="warning" size={14} />
      <span class="msg">
        {app.info.compiledOnly.length} compiled script{app.info.compiledOnly.length === 1 ? '' : 's'} could not be
        decompiled, so {app.info.compiledOnly.length === 1 ? 'it is' : 'they are'} not on the map.
      </span>
      <button type="button" class="sm" onclick={() => openRenpy('game')}>Show</button>
      {@render dismiss(() => hide('compiled'))}
    </div>
  {/if}
  {#if archiveErrors && !hidden.includes('archives')}
    <div class="alert warn" role="status" transition:slide={{ duration: 140 }}>
      <Icon name="warning" size={14} />
      <span class="msg">{archiveErrors} archive{archiveErrors === 1 ? '' : 's'} could not be read.</span>
      <button type="button" class="sm" onclick={() => openRenpy('archives')}>Show</button>
      {@render dismiss(() => hide('archives'))}
    </div>
  {/if}
  {#if app.info?.engine?.stale && !hidden.includes('stale')}
    <div class="alert info" role="status" transition:slide={{ duration: 140 }}>
      <Icon name="info" size={14} />
      <span class="msg">Scripts changed since the engine check; re-run it to refresh.</span>
      <button type="button" class="sm" onclick={() => void checkWithEngine()}>Re-check</button>
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
</style>

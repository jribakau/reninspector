<script lang="ts">
  import Icon from '../components/Icon.svelte'
  import { sdk, installProgressText, openSdkManager } from '../lib/sdk.svelte'
  import { app, openBottom } from '../lib/store.svelte'

  const errors = $derived(app.diag?.errors ?? 0)
  const warnings = $derived(app.diag?.warnings ?? 0)
</script>

<footer class="status" class:busy-bar={!!app.busy || !!(sdk.installing && sdk.progress)} class:has-errors={errors > 0} aria-busy={!!app.busy || !!(sdk.installing && sdk.progress)}>
  {#if app.info}
    <button
      class="bare"
      class:bad={errors > 0}
      class:warn={errors === 0 && warnings > 0}
      aria-label={`Problems: ${errors} errors, ${warnings} warnings`}
      title="Problems"
      onclick={() => openBottom('problems')}
    >
      <Icon name="error" size={12} />
      <span class="badge" class:bad={errors > 0}>{errors}</span>
      <Icon name="warning" size={12} />
      <span class="badge" class:warn={warnings > 0}>{warnings}</span>
    </button>
  {/if}
  {#if app.busy}
    <span class="busy"><i class="spinner"></i>{app.busy}</span>
  {:else if sdk.installing && sdk.progress}
    <span class="busy"><i class="spinner"></i>{installProgressText(sdk.progress)}</span>
  {:else}
    <span class="dim notice" aria-live="polite">{app.notice}</span>
    {#if app.noticeAction && app.noticeAction.text === app.notice}
      <button class="bare" type="button" onclick={() => app.noticeAction?.run()}>{app.noticeAction.label}</button>
    {/if}
  {/if}
  <span class="spacer"></span>
  {#if app.live.running}
    <button class="bare live" aria-label="Live session" onclick={() => openBottom('live')} title={app.live.file ? `${app.live.file}:${app.live.line}` : 'Waiting for the game'}>
      <Icon name="live" size={12} />
      {app.live.label || '…'}{app.live.line ? `:${app.live.line}` : ''}
    </button>
  {/if}
  {#if app.cursor}
    <span class="dim cursor" title={`${app.cursor.file}:${app.cursor.line}`}>Ln {app.cursor.line}</span>
  {/if}
  {#if app.info && sdk.label}
    <button
      class="bare"
      class:warn={sdk.mismatch || sdk.kind === 'missing'}
      aria-label="Ren'Py SDK"
      title={sdk.mismatch
        ? 'This SDK is a different Ren\'Py release than the project. A newer SDK recompiles scripts and can change save compatibility.'
        : 'Choose the Ren\'Py version'}
      onclick={() => void openSdkManager()}
    >
      {#if sdk.mismatch || sdk.kind === 'missing'}<Icon name="warning" size={12} />{/if}
      {sdk.label}
    </button>
  {/if}
  {#if app.info}
    <span class="dim path wide-only" title={app.info.root}>{app.info.root}</span>
  {/if}
</footer>

<style>
  .status {
    display: flex;
    gap: var(--sp-3);
    align-items: center;
    padding: 0 var(--sp-3) 0 var(--sp-4);
    background: var(--panel);
    border-top: 1px solid var(--line);
    box-shadow: inset 0 1px 0 transparent;
    font-size: var(--fs-sm);
    flex: none;
    min-height: var(--h-status);
    container-type: inline-size;
  }
  .status.busy-bar {
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .status.has-errors {
    box-shadow: inset 2px 0 0 var(--error);
  }
  .status.busy-bar.has-errors {
    box-shadow: inset 0 2px 0 var(--accent), inset 2px 0 0 var(--error);
  }
  @container (max-width: 860px) {
    .wide-only {
      display: none;
    }
  }
  .notice,
  .cursor {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cursor {
    max-width: 28%;
    padding: 0 var(--sp-2);
  }
  .status .path {
    max-width: 28%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .busy {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
  }
  .busy .spinner {
    width: 10px;
    height: 10px;
    border-width: 1.5px;
  }
  .bare {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 18px;
    background: none;
    border: none;
    padding: 0 var(--sp-2);
    color: var(--dim);
    border-radius: var(--r-sm);
    font-size: var(--fs-sm);
  }
  .bare:hover:not(:disabled) {
    color: var(--text);
    background: var(--hover);
    border-color: transparent;
  }
  .bare.bad {
    color: var(--error);
  }
  .bare.warn {
    color: var(--warning);
  }
  .bare .badge {
    min-width: 14px;
    line-height: 14px;
  }
  .bare.live {
    color: var(--ok);
  }
</style>

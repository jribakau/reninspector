<script lang="ts">
  import Icon from '../components/Icon.svelte'
  import { dismissToast, pauseToast, resumeToast, toasts, type ToastKind } from '../lib/toast.svelte'

  const icon: Record<ToastKind, 'info' | 'check' | 'warning' | 'error'> = {
    info: 'info',
    ok: 'check',
    warn: 'warning',
    error: 'error',
  }
</script>

{#if toasts.items.length}
  <div class="toasts" aria-live="polite">
    {#each toasts.items as toast (toast.id)}
      <div
        class="toast {toast.kind} anim-pop"
        role={toast.kind === 'error' ? 'alert' : 'status'}
        onmouseenter={() => pauseToast(toast.id)}
        onmouseleave={() => resumeToast(toast.id)}
      >
        <Icon name={icon[toast.kind]} size={14} />
        <span class="text">{toast.text}</span>
        {#if toast.action}
          <button type="button" class="sm" onclick={() => { toast.action?.run(); dismissToast(toast.id) }}>
            {toast.action.label}
          </button>
        {/if}
        <button type="button" class="icon sm ghost" aria-label="Dismiss" onclick={() => dismissToast(toast.id)}>
          <Icon name="close" size={13} />
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toasts {
    position: fixed;
    right: var(--sp-4);
    bottom: calc(var(--h-status) + var(--sp-3));
    z-index: var(--z-context);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: min(380px, calc(100vw - var(--sp-6)));
    pointer-events: none;
  }

  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: var(--sp-3);
    padding: var(--sp-3);
    background: var(--panel);
    border: 1px solid var(--line);
    border-left-width: 3px;
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    font-size: var(--fs-md);
  }

  .toast.info { border-left-color: var(--accent); }
  .toast.ok { border-left-color: var(--ok); }
  .toast.warn { border-left-color: var(--warning); }
  .toast.error { border-left-color: var(--error); }

  .toast.info :global(svg:first-child) { color: var(--accent); }
  .toast.ok :global(svg:first-child) { color: var(--ok); }
  .toast.warn :global(svg:first-child) { color: var(--warning); }
  .toast.error :global(svg:first-child) { color: var(--error); }

  .text {
    flex: 1;
    min-width: 0;
    padding-top: 1px;
  }

  .toast :global(button.sm) {
    flex: none;
  }
</style>

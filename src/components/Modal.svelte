<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte'
  import { fade } from 'svelte/transition'
  import { dismissTransient, pushOverlay } from '../lib/overlay.svelte'
  import Icon from './Icon.svelte'

  interface Props {
    title: string
    wide?: boolean
    /** `xl` is a tall, fixed-height sheet for dialogs that lay out their own panes. */
    size?: 'normal' | 'wide' | 'xl'
    /** Drops the body padding and scrolling so the content can manage its own layout. */
    flush?: boolean
    /** While busy the dialog cannot be dismissed from the header or backdrop. */
    busy?: boolean
    onclose: () => void
    children: Snippet
    /** Actions row. Put secondary actions first and the primary one last. */
    footer?: Snippet
  }

  let { title, wide = false, size = 'normal', flush = false, busy = false, onclose, children, footer }: Props = $props()
  const sizeClass = $derived(size !== 'normal' ? size : wide ? 'wide' : '')
  let sheet: HTMLDivElement | undefined = $state()
  const titleId = `modal-title-${Math.random().toString(36).slice(2, 8)}`
  const motion = typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 140
  // The overlay handler is registered once, so it reads this instead of the prop from mount time.
  let dismissBlocked = false
  $effect(() => {
    dismissBlocked = busy
  })

  onMount(() => {
    dismissTransient()
    const pop = pushOverlay({
      kind: 'modal',
      el: () => sheet ?? null,
      onEscape: () => {
        if (!dismissBlocked) onclose()
      },
    })
    void tick().then(() => {
      const first =
        sheet?.querySelector<HTMLElement>('.body input, .body select, .body textarea, .body button') ??
        sheet?.querySelector<HTMLElement>('.dialog-foot button.primary')
      first?.focus()
      if (first instanceof HTMLInputElement && first.type === 'text') first.select()
    })
    return pop
  })

</script>

<div class="modal" transition:fade|global={{ duration: motion }}>
  <button type="button" class="backdrop" aria-label="Close dialog" disabled={busy} onclick={onclose}></button>
  <div class="sheet {sizeClass}" bind:this={sheet} role="dialog" aria-modal="true" aria-labelledby={titleId}>
    <header class="dialog-head">
      <h3 id={titleId}>{title}</h3>
      <button type="button" class="icon sm ghost" aria-label="Close" disabled={busy} onclick={onclose}>
        <Icon name="close" size={14} />
      </button>
    </header>
    <div class="body" class:flush>
      {@render children()}
    </div>
    {#if footer}
      <footer class="dialog-foot">
        {@render footer()}
      </footer>
    {/if}
  </div>
</div>

<style>
  .modal {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    z-index: var(--z-modal);
    padding: var(--sp-6);
  }
  .backdrop {
    position: absolute;
    inset: 0;
    border: none;
    border-radius: 0;
    padding: 0;
    background: var(--scrim);
  }
  .backdrop:hover:not(:disabled) {
    border-color: transparent;
    background: var(--scrim);
  }
  .sheet {
    position: relative;
    z-index: var(--z-base);
    width: min(480px, 100%);
    max-height: min(86vh, 760px);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-lg);
    animation: pop-in var(--dur) var(--ease);
    overflow: hidden;
  }
  .sheet.wide {
    width: min(680px, 100%);
  }
  .sheet.xl {
    width: min(940px, 100%);
    height: min(86vh, 660px);
  }
  .body.flush {
    flex: 1;
    padding: 0;
    gap: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  h3 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: var(--fs-lg);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .body {
    padding: var(--sp-4) var(--sp-5);
    overflow: auto;
    min-height: 0;
    display: grid;
    gap: var(--sp-3);
    align-content: start;
  }
</style>

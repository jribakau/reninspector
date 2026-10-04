<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte'
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

  onMount(() => {
    dismissTransient()
    const pop = pushOverlay({
      kind: 'modal',
      el: () => sheet ?? null,
      onEscape: onclose,
    })
    void tick().then(() => {
      const first =
        sheet?.querySelector<HTMLElement>('.body input, .body select, .body textarea, .body button') ??
        sheet?.querySelector<HTMLElement>('.foot button.primary')
      first?.focus()
      if (first instanceof HTMLInputElement && first.type === 'text') first.select()
    })
    return pop
  })

  function backdrop(e: MouseEvent) {
    if (e.target === e.currentTarget && !busy) onclose()
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="modal" onclick={backdrop}>
  <div class="sheet {sizeClass}" bind:this={sheet} role="dialog" aria-modal="true" aria-labelledby={titleId}>
    <header class="head">
      <h3 id={titleId}>{title}</h3>
      <button type="button" class="icon sm ghost" aria-label="Close" disabled={busy} onclick={onclose}>
        <Icon name="close" size={14} />
      </button>
    </header>
    <div class="body" class:flush>
      {@render children()}
    </div>
    {#if footer}
      <footer class="foot">
        {@render footer()}
      </footer>
    {/if}
  </div>
</div>

<style>
  .modal {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    display: grid;
    place-items: center;
    z-index: var(--z-modal);
    padding: 24px;
    animation: fade-in var(--dur) var(--ease);
  }
  .sheet {
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
  .head {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: 8px 8px 8px 16px;
    border-bottom: 1px solid var(--line-soft);
    flex: none;
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
    padding: 14px 16px;
    overflow: auto;
    min-height: 0;
    display: grid;
    gap: 10px;
    align-content: start;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-3);
    padding: 10px 16px;
    border-top: 1px solid var(--line-soft);
    background: color-mix(in srgb, var(--bg) 40%, var(--panel));
    flex: none;
  }
</style>

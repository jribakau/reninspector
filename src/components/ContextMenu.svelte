<script lang="ts">
  import { closeContextMenu, contextMenu } from '../lib/context.svelte'
  import { overlayBus, pushOverlay } from '../lib/overlay.svelte'

  let el = $state<HTMLDivElement | null>(null)

  $effect(() => {
    if (!contextMenu.open) return
    const node = el
    if (node) {
      const pad = 8
      const width = node.offsetWidth
      const height = node.offsetHeight
      let x = contextMenu.rightEdge ? contextMenu.x - width : contextMenu.x
      let y = contextMenu.y
      if (x < pad) x = pad
      if (x + width > window.innerWidth - pad) x = Math.max(pad, window.innerWidth - width - pad)
      if (y + height > window.innerHeight - pad) y = Math.max(pad, window.innerHeight - height - pad)
      node.style.left = `${x}px`
      node.style.top = `${y}px`
    }
    const down = (e: PointerEvent) => {
      const target = e.target
      if (target instanceof Node && node?.contains(target)) return
      closeContextMenu()
    }
    const key = (e: KeyboardEvent) => {
      if (!node) return
      const buttons = [...node.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')]
      if (!buttons.length) return
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        e.stopImmediatePropagation()
        buttons[(index + 1) % buttons.length]?.focus()
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        e.stopImmediatePropagation()
        buttons[(index - 1 + buttons.length) % buttons.length]?.focus()
      }
    }
    const scroll = () => closeContextMenu()
    const pop = pushOverlay({
      kind: 'context',
      el: () => node,
      onEscape: closeContextMenu,
    })
    node?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
    window.addEventListener('pointerdown', down, true)
    window.addEventListener('keydown', key, true)
    window.addEventListener('scroll', scroll, true)
    return () => {
      pop()
      window.removeEventListener('pointerdown', down, true)
      window.removeEventListener('keydown', key, true)
      window.removeEventListener('scroll', scroll, true)
    }
  })

  let seenDismiss = 0
  $effect(() => {
    const n = overlayBus.dismiss
    if (n === seenDismiss) return
    seenDismiss = n
    if (contextMenu.open) closeContextMenu()
  })

  function run(item: { run: () => void }) {
    closeContextMenu()
    item.run()
  }
</script>

{#if contextMenu.open}
  <div class="menu ctx anim-pop" bind:this={el} style={`left:${contextMenu.x}px;top:${contextMenu.y}px`} role="menu" tabindex="-1" oncontextmenu={(e) => e.preventDefault()}>
    {#each contextMenu.items as item, i (i)}
      {#if item.kind === 'sep'}
        <div class="menu-sep" role="separator"></div>
      {:else}
        <button type="button" class="menu-item" class:current={item.current} role="menuitem" disabled={item.enabled === false} title={item.hint} onclick={() => run(item)}>
          {item.label}
          {#if item.key}<span class="menu-key">{item.key}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .ctx {
    position: fixed;
    z-index: var(--z-context);
    min-width: 180px;
    max-height: calc(100vh - 16px);
  }
  .ctx :global(.menu-item.current) {
    color: var(--accent);
    font-weight: 600;
  }
  .ctx :global(.menu-item:focus-visible) {
    background: var(--sel);
    outline: none;
  }
</style>

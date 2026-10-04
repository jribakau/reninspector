<script lang="ts" generics="T">
  import type { Snippet } from 'svelte'

  interface Props {
    items: T[]
    rowHeight: number
    row: Snippet<[T, number]>
    /** Scroll this index into view when it changes. */
    reveal?: number | null
    /** Bumped so the same index scrolls again. */
    revealSeq?: number
    /** Stable identity for a row. Defaults to the index. */
    itemKey?: (item: T, index: number) => string | number
    role?: 'listbox' | 'tree'
    /** Left/Right on the active row, for trees. */
    onhorizontal?: (dir: 'left' | 'right', index: number) => void
    /** Sees every key first. Call preventDefault to keep the list from acting on it. */
    onkey?: (e: KeyboardEvent, index: number) => void
    /** Index of the keyboard-focused row. */
    active?: number
  }

  let { items, rowHeight, row, reveal = null, revealSeq = 0, itemKey, role = 'listbox', onhorizontal, onkey, active = $bindable(0) }: Props = $props()

  let focused = $state(false)
  let typed = ''
  let typedTimer: ReturnType<typeof setTimeout> | null = null

  /** Type-ahead matches the visible text of rendered rows, or the item's string form when off screen. */
  function rowText(index: number): string {
    const item = items[index] as unknown
    if (typeof item === 'string') return item.toLowerCase()
    if (item && typeof item === 'object') {
      const o = item as Record<string, unknown>
      for (const k of ['name', 'label', 'id', 'path', 'message']) {
        if (typeof o[k] === 'string') return (o[k] as string).toLowerCase()
      }
      const entry = o.entry as Record<string, unknown> | undefined
      if (entry && typeof entry.name === 'string') return entry.name.toLowerCase()
    }
    return ''
  }

  $effect(() => {
    if (active >= items.length) active = Math.max(0, items.length - 1)
  })

  let el: HTMLDivElement
  let scrollTop = $state(0)
  let height = $state(0)

  const start = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - 6))
  const end = $derived(Math.min(items.length, Math.ceil((scrollTop + height) / rowHeight) + 6))
  const slice = $derived(items.slice(start, end))

  $effect(() => {
    const index = reveal
    const seq = revealSeq
    if (index === null || index < 0 || seq < 0 || !el) return
    const top = index * rowHeight
    const viewTop = el.scrollTop
    const viewH = el.clientHeight
    if (top < viewTop) el.scrollTop = Math.max(0, top - rowHeight * 2)
    else if (top + rowHeight > viewTop + viewH) el.scrollTop = top - viewH / 2
  })
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="vl"
  {role}
  tabindex="0"
  bind:this={el}
  bind:clientHeight={height}
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  onkeydown={(e) => {
    onkey?.(e, active)
    if (e.defaultPrevented || !items.length) return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Home' || e.key === 'End') {
      e.preventDefault()
      if (e.key === 'Home') active = 0
      else if (e.key === 'End') active = items.length - 1
      else active = Math.min(items.length - 1, Math.max(0, active + (e.key === 'ArrowDown' ? 1 : -1)))
      const top = active * rowHeight
      if (top < el.scrollTop) el.scrollTop = top
      else if (top + rowHeight > el.scrollTop + el.clientHeight) el.scrollTop = top - el.clientHeight + rowHeight
    } else if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && onhorizontal) {
      e.preventDefault()
      onhorizontal(e.key === 'ArrowLeft' ? 'left' : 'right', active)
    } else if (e.key === 'Enter') {
      const row = el.querySelectorAll('.r')[active - start]
      // A row either is the target or holds one: a button, or a div with role="button".
      const node = row?.querySelector<HTMLElement>('button, [role="button"], [role="treeitem"]')
      // The focused list already has the key; a row that handles Enter itself would fire twice.
      if (node && e.target === el) node.click()
    } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      typed += e.key.toLowerCase()
      if (typedTimer) clearTimeout(typedTimer)
      typedTimer = setTimeout(() => (typed = ''), 700)
      const from = typed.length > 1 ? active : active + 1
      for (let n = 0; n < items.length; n++) {
        const i = (from + n) % items.length
        const text = rowText(i)
        if (text.startsWith(typed)) {
          active = i
          const top = i * rowHeight
          if (top < el.scrollTop || top + rowHeight > el.scrollTop + el.clientHeight) el.scrollTop = Math.max(0, top - el.clientHeight / 2)
          break
        }
      }
    }
  }}
  onfocus={() => (focused = true)}
  onblur={() => (focused = false)}
>
  <div class="space" style={`height:${items.length * rowHeight}px`}>
    <div class="win" style={`top:${start * rowHeight}px`}>
      {#each slice as item, i (itemKey ? itemKey(item, start + i) : start + i)}
        <div class="r" role="presentation" class:active={focused && start + i === active} style={`height:${rowHeight}px`} onpointerdown={() => (active = start + i)}>
          {@render row(item, start + i)}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .vl {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    outline: none;
  }
  .space {
    position: relative;
  }
  .win {
    position: absolute;
    left: 0;
    right: 0;
  }
  .r.active {
    outline: 1px solid var(--focus-ring);
    outline-offset: -1px;
    background: var(--hover);
  }
  .r {
    overflow: hidden;
  }
</style>

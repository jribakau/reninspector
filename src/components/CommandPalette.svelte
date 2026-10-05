<script lang="ts">
  import PaletteShell from './PaletteShell.svelte'
  import { paletteActions } from '../lib/menus.svelte'
  import { overlayBus, pushOverlay } from '../lib/overlay.svelte'
  import { app, goTo, nodeByName, selectLabel } from '../lib/store.svelte'
  import type { Symbol } from '../lib/types'

  interface Item {
    id: string
    label: string
    shortcut?: string
    enabled: boolean
    run: () => void
  }

  let query = $state('')
  let input = $state<HTMLInputElement | null>(null)
  let listEl: HTMLDivElement | undefined = $state()
  let picked = $state(0)

  const commands = $derived(paletteActions())

  const SYMBOL_KINDS = new Set(['label', 'screen', 'character', 'image', 'transform'])
  const PALETTE_CAP = 80

  function atBoundary(text: string, index: number): boolean {
    if (index <= 0) return true
    const prev = text[index - 1]
    if (prev === '_' || prev === '.' || prev === '/' || prev === ' ') return true
    const cur = text[index]
    return prev === prev.toLowerCase() && cur !== cur.toLowerCase()
  }

  /** Lower is better. Exact, prefix, boundary, then a plain substring. */
  function rankIn(text: string, q: string): number | null {
    const lower = text.toLowerCase()
    if (lower === q) return 0
    if (lower.startsWith(q)) return 1
    let from = 1
    while (from <= lower.length - q.length) {
      const at = lower.indexOf(q, from)
      if (at < 0) break
      if (atBoundary(text, at)) return 2
      from = at + 1
    }
    if (lower.includes(q)) return 3
    return null
  }

  function takeRanked<T>(
    list: T[],
    q: string,
    rank: (item: T) => number | null,
    tie: (a: T, b: T) => number,
  ): T[] {
    const scored: { item: T; rank: number; index: number }[] = []
    list.forEach((item, index) => {
      const score = rank(item)
      if (score !== null) scored.push({ item, rank: score, index })
    })
    scored.sort((a, b) => a.rank - b.rank || (q ? tie(a.item, b.item) : a.index - b.index))
    return scored.slice(0, PALETTE_CAP).map((s) => s.item)
  }

  function byName(a: { name: string; path: string }, b: { name: string; path: string }): number {
    if (a.name.length !== b.name.length) return a.name.length - b.name.length
    const byName = a.name.localeCompare(b.name)
    return byName || a.path.localeCompare(b.path)
  }

  function openSymbol(sym: Symbol) {
    const id = sym.kind === 'screen' ? `screen:${sym.name}` : sym.name
    if ((sym.kind === 'label' || sym.kind === 'screen') && nodeByName(id)) selectLabel(id)
    else if (sym.path) goTo(sym.path, sym.line)
  }

  const items = $derived.by(() => {
    const q = query.trim().toLowerCase()
    const openFile = app.loc?.file ?? ''
    if (app.palette === 'files') {
      return takeRanked(
        app.info?.files ?? [],
        q,
        (f) => {
          if (!q) return f.path === openFile ? 0 : 1
          const base = f.path.split('/').pop() ?? f.path
          return rankIn(base, q) ?? (f.path.toLowerCase().includes(q) ? 4 : null)
        },
        (a, b) => {
          const an = a.path.split('/').pop() ?? a.path
          const bn = b.path.split('/').pop() ?? b.path
          return byName({ name: an, path: a.path }, { name: bn, path: b.path })
        },
      ).map((f) => ({ id: f.path, label: f.path, shortcut: '', enabled: true, run: () => {} }))
    }
    if (app.palette === 'symbols') {
      return takeRanked(
        (app.catalog?.symbols ?? []).filter((s) => SYMBOL_KINDS.has(s.kind)),
        q,
        (s) => {
          if (!q) return s.path === openFile ? 0 : 1
          return rankIn(s.name, q) ?? (s.kind.toLowerCase().includes(q) ? 4 : null)
        },
        byName,
      ).map((s) => ({
        id: `${s.kind}:${s.name}:${s.path}:${s.line}`,
        label: `${s.name} · ${s.kind} · ${s.path}`,
        shortcut: '',
        enabled: true,
        run: () => openSymbol(s),
      }))
    }
    return commands
      .filter((c) => !q || c.label.toLowerCase().includes(q))
      .map((c) => ({ id: c.id, label: c.label, shortcut: c.shortcut, enabled: c.enabled, run: c.run }))
  })

  const paletteLabel = $derived(
    app.palette === 'files' ? 'Go to file' : app.palette === 'symbols' ? 'Go to symbol' : 'Commands',
  )

  let seenDismiss = 0
  let box = $state<HTMLDivElement | null>(null)

  $effect(() => {
    const n = overlayBus.dismiss
    if (n !== seenDismiss) {
      seenDismiss = n
      app.palette = null
    }
  })

  $effect(() => {
    if (!app.palette) return
    query = ''
    picked = 0
    const pop = pushOverlay({
      kind: 'palette',
      el: () => box ?? null,
      onEscape: close,
    })
    queueMicrotask(() => input?.focus())
    return pop
  })

  $effect(() => {
    const index = picked
    if (!app.palette || !listEl) return
    listEl.querySelectorAll('button')[index]?.scrollIntoView({ block: 'nearest' })
  })

  function close() {
    app.palette = null
  }

  function choose(index: number) {
    const item = items[index]
    if (!item || !item.enabled) return
    const mode = app.palette
    if (mode === 'files') goTo(item.id, 1, 1, { flow: true, open: true })
    else item.run()
    if (app.palette === mode) close()
  }

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault()
      close()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      picked = Math.min(items.length - 1, picked + 1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      picked = Math.max(0, picked - 1)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      choose(picked)
    }
  }
</script>

{#if app.palette}
  <PaletteShell
    label={paletteLabel}
    placeholder={app.palette === 'files' ? 'Go to file…' : app.palette === 'symbols' ? 'Go to symbol…' : 'Type a command…'}
    bind:query
    bind:box
    bind:input
    onclose={close}
    onkey={key}
    oninput={() => (picked = 0)}
  >
    <div class="pal-list" bind:this={listEl}>
      {#each items as item, i (item.id)}
        <button class="pal-item" class:on={i === picked} title={item.label} disabled={!item.enabled} onclick={() => choose(i)}>
          <span class="pal-lab">{item.label}</span>
          {#if item.shortcut}<span class="pal-key">{item.shortcut}</span>{/if}
        </button>
      {/each}
      {#if !items.length}<div class="pal-empty">Nothing matches.</div>{/if}
    </div>
  </PaletteShell>
{/if}

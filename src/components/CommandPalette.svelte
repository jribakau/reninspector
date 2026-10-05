<script lang="ts">
  import PaletteShell from './PaletteShell.svelte'
  import { highlightPieces, rankMatches, scoreFile, scoreLabel, scoreSymbol, symbolLabel } from '../lib/fuzzy'
  import { paletteActions } from '../lib/menus.svelte'
  import { overlayBus, pushOverlay } from '../lib/overlay.svelte'
  import { app, goTo, nodeByName, selectLabel } from '../lib/store.svelte'
  import type { Symbol } from '../lib/types'

  interface Piece {
    text: string
    hit: boolean
  }

  interface Item {
    id: string
    label: string
    pieces: Piece[]
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

  function plain(text: string): Piece[] {
    return [{ text, hit: false }]
  }

  /** The open file first, then everything else in catalog order. */
  function openFirst<T>(list: readonly T[], isOpen: (item: T) => boolean): T[] {
    const first: T[] = []
    const rest: T[] = []
    for (const item of list) (isOpen(item) ? first : rest).push(item)
    return first.concat(rest).slice(0, PALETTE_CAP)
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
    const q = query.trim()
    const openFile = app.loc?.file ?? ''
    if (app.palette === 'files') {
      const files = app.info?.files ?? []
      if (!q) {
        return openFirst(files, (f) => f.path === openFile).map((f) => ({
          id: f.path, label: f.path, pieces: plain(f.path), shortcut: '', enabled: true, run: () => {},
        }))
      }
      return rankMatches(
        files,
        (f) => scoreFile(q, f.path),
        (a, b) => byName({ name: a.path.split('/').pop() ?? a.path, path: a.path }, { name: b.path.split('/').pop() ?? b.path, path: b.path }),
        PALETTE_CAP,
      ).map(({ item, hit }) => ({
        id: item.path, label: item.path, pieces: highlightPieces(item.path, hit.matches), shortcut: '', enabled: true, run: () => {},
      }))
    }
    if (app.palette === 'symbols') {
      const symbols = (app.catalog?.symbols ?? []).filter((s) => SYMBOL_KINDS.has(s.kind))
      if (!q) {
        return openFirst(symbols, (s) => s.path === openFile).map((s) => {
          const label = symbolLabel(s.name, s.kind, s.path)
          return { id: `${s.kind}:${s.name}:${s.path}:${s.line}`, label, pieces: plain(label), shortcut: '', enabled: true, run: () => openSymbol(s) }
        })
      }
      return rankMatches(symbols, (s) => scoreSymbol(q, s.name, s.kind, s.path), byName, PALETTE_CAP).map(({ item, hit }) => {
        const label = symbolLabel(item.name, item.kind, item.path)
        return {
          id: `${item.kind}:${item.name}:${item.path}:${item.line}`,
          label,
          pieces: highlightPieces(label, hit.matches),
          shortcut: '',
          enabled: true,
          run: () => openSymbol(item),
        }
      })
    }
    if (!q) return commands.map((c) => ({ id: c.id, label: c.label, pieces: plain(c.label), shortcut: c.shortcut, enabled: c.enabled, run: c.run }))
    return rankMatches(commands, (c) => scoreLabel(q, c.label), () => 0, PALETTE_CAP).map(({ item, hit }) => ({
      id: item.id, label: item.label, pieces: highlightPieces(item.label, hit.matches), shortcut: item.shortcut, enabled: item.enabled, run: item.run,
    }))
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
          <span class="pal-lab">
            {#each item.pieces as piece, p (p)}
              {#if piece.hit}<span class="pal-hit">{piece.text}</span>{:else}{piece.text}{/if}
            {/each}
          </span>
          {#if item.shortcut}<span class="pal-key">{item.shortcut}</span>{/if}
        </button>
      {/each}
      {#if !items.length}<div class="pal-empty">Nothing matches.</div>{/if}
    </div>
  </PaletteShell>
{/if}

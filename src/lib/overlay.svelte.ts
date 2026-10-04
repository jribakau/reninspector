export type OverlayKind = 'modal' | 'palette' | 'menu' | 'context'

interface Entry {
  id: number
  kind: OverlayKind
  el: () => HTMLElement | null
  onEscape: () => void
  restore: HTMLElement | null
}

const stack: Entry[] = []
let seq = 0

/** Bumped when a modal opens so menus, the palette, and context menus close. */
export const overlayBus = $state({ dismiss: 0 })

export const overlays = $state({ depth: 0, modal: false })

function sync() {
  overlays.depth = stack.length
  overlays.modal = stack.some((entry) => entry.kind === 'modal')
}

export function anyModal(): boolean {
  return overlays.modal
}

export function dismissTransient() {
  overlayBus.dismiss += 1
}

const FOCUSABLE =
  'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])'

function focusables(el: HTMLElement): HTMLElement[] {
  return [...el.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((node) => {
    if (node.getAttribute('aria-hidden') === 'true') return false
    return node.tabIndex >= 0
  })
}

function trapTab(el: HTMLElement, e: KeyboardEvent) {
  const nodes = focusables(el)
  if (!nodes.length) {
    e.preventDefault()
    return
  }
  const first = nodes[0]
  const last = nodes[nodes.length - 1]
  const active = document.activeElement
  if (e.shiftKey) {
    if (active === first || !(active instanceof Node) || !el.contains(active)) {
      e.preventDefault()
      last.focus()
    }
  } else if (active === last || !(active instanceof Node) || !el.contains(active)) {
    e.preventDefault()
    first.focus()
  }
}

/** Registers an overlay. The returned function removes it and restores focus if it was on top. */
export function pushOverlay(opts: {
  kind: OverlayKind
  el: () => HTMLElement | null
  onEscape: () => void
}): () => void {
  const restore = document.activeElement instanceof HTMLElement ? document.activeElement : null
  const entry: Entry = { id: ++seq, kind: opts.kind, el: opts.el, onEscape: opts.onEscape, restore }
  stack.push(entry)
  sync()
  return () => {
    const index = stack.findIndex((item) => item.id === entry.id)
    if (index < 0) return
    const wasTop = index === stack.length - 1
    stack.splice(index, 1)
    sync()
    if (wasTop && entry.restore?.isConnected) entry.restore.focus()
  }
}

/** Capture-phase handler. Only the top overlay sees Escape, and Tab stays inside it. */
export function handleOverlayKey(e: KeyboardEvent): boolean {
  const top = stack[stack.length - 1]
  if (!top) return false
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopImmediatePropagation()
    top.onEscape()
    return true
  }
  if (e.key === 'Tab') {
    const el = top.el()
    if (el) {
      trapTab(el, e)
      e.stopImmediatePropagation()
    }
    return true
  }
  return false
}

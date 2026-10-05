export interface TabDrag {
  id: string
  x: number
  y: number
  /** Where the tab would land: before the tab now at this index. Null when released here would cancel. */
  slot: number | null
  /** Pixel offset of the drop marker inside the tab strip. */
  mark: number
}

export interface TabDragHost {
  strip(): HTMLElement | null | undefined
  indexOf(id: string): number
  activate(id: string): void
  move(id: string, slot: number): void
}

/** Pointer-drag reordering. The window swallows native drag events used for OS file drops. */
export function createTabDrag(host: TabDragHost) {
  let drag = $state<TabDrag | null>(null)
  let dragFrom: { x: number; y: number; id: string } | null = null
  let scrollDir = 0
  let scroller: ReturnType<typeof setInterval> | null = null

  function slotAt(x: number, y: number): { slot: number | null; mark: number } {
    const strip = host.strip()
    if (!strip) return { slot: null, mark: 0 }
    const bar = strip.parentElement?.getBoundingClientRect()
    if (bar && (y < bar.top - 24 || y > bar.bottom + 24)) return { slot: null, mark: 0 }
    const rects = [...strip.querySelectorAll<HTMLElement>('.tab')].map((el) => el.getBoundingClientRect())
    const origin = strip.getBoundingClientRect().left - strip.scrollLeft
    let slot = rects.length
    for (let i = 0; i < rects.length; i++) {
      if (x < rects[i].left + rects[i].width / 2) {
        slot = i
        break
      }
    }
    const edge = slot < rects.length ? rects[slot].left : (rects[rects.length - 1]?.right ?? origin)
    return { slot, mark: edge - origin }
  }

  function onMove(e: PointerEvent) {
    const from = dragFrom
    if (!from) return
    if (!drag && Math.hypot(e.clientX - from.x, e.clientY - from.y) < 5) return
    drag = { id: from.id, x: e.clientX, y: e.clientY, ...slotAt(e.clientX, e.clientY) }
    const strip = host.strip()
    const box = strip?.getBoundingClientRect()
    scrollDir = !box ? 0 : e.clientX < box.left + 32 ? -1 : e.clientX > box.right - 32 ? 1 : 0
    if (scrollDir && !scroller) {
      scroller = setInterval(() => {
        const node = host.strip()
        if (!scrollDir || !drag || !node) return
        node.scrollLeft += scrollDir * 14
        drag = { ...drag, ...slotAt(drag.x, drag.y) }
      }, 16)
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Escape') return
    e.stopPropagation()
    stop()
  }

  function stop() {
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onDrop)
    window.removeEventListener('keydown', onKey, true)
    if (scroller) clearInterval(scroller)
    scroller = null
    scrollDir = 0
    dragFrom = null
    drag = null
  }

  function onDrop() {
    const from = dragFrom
    const done = drag
    stop()
    if (!from) return
    const index = host.indexOf(from.id)
    // Dropping back on the same tab is a click. The drag used to swallow that click,
    // so the small pinned icon looked like it did nothing.
    const sameSlot = !!done && (done.slot === index || done.slot === index + 1)
    if (!done || sameSlot) {
      host.activate(from.id)
      return
    }
    if (done.slot === null) return
    const swallow = (ev: MouseEvent) => ev.stopPropagation()
    window.addEventListener('click', swallow, { capture: true, once: true })
    setTimeout(() => window.removeEventListener('click', swallow, true), 0)
    host.move(from.id, done.slot)
  }

  function begin(e: PointerEvent, id: string) {
    if (e.button !== 0 || (e.target instanceof Element && e.target.closest('.x'))) return
    dragFrom = { x: e.clientX, y: e.clientY, id }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onDrop)
    window.addEventListener('keydown', onKey, true)
  }

  return {
    get drag() {
      return drag
    },
    begin,
    stop,
  }
}

/** A vertical wheel over the strip scrolls the tabs sideways. */
export function horizontalWheel(node: HTMLElement) {
  const onWheel = (e: WheelEvent) => {
    if (node.scrollWidth <= node.clientWidth + 1) return
    const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY
    if (!delta) return
    e.preventDefault()
    node.scrollLeft += delta
  }
  node.addEventListener('wheel', onWheel, { passive: false })
  return {
    destroy() {
      node.removeEventListener('wheel', onWheel)
    },
  }
}

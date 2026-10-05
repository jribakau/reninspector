<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { ViewRect } from '../lib/view'

  interface Props {
    contentWidth: number
    contentHeight: number
    /** Refit when this changes (e.g. a new layout). */
    fitKey: string | number
    minK?: number
    maxK?: number
    children: Snippet<[ViewRect]>
    hud?: Snippet<[ViewRect]>
    onview?: (rect: ViewRect) => void
    /** Bumps when the layout is replaced, so the drawing fades in. */
    fadeKey?: number
  }

  let {
    contentWidth,
    contentHeight,
    fitKey,
    minK = 0.03,
    maxK = 2.5,
    children,
    hud,
    onview,
    fadeKey = 0,
  }: Props = $props()

  // Zoom bands shared by the project map and label graph. The culling view
  // keeps one k per band so labels and level-of-detail do not change on every tick.
  const ZOOM_STEPS = [0.14, 0.35, 0.4, 0.8]

  let el: HTMLDivElement
  let cw = $state(0)
  let ch = $state(0)
  // Authoritative camera. Pointer and wheel handlers write these immediately
  // and publish them to `frame` at most once per animation frame.
  let tx = 0
  let ty = 0
  let k = 1
  let frame = $state({ tx: 0, ty: 0, k: 1 })
  let cull = $state.raw<ViewRect | null>(null)
  let cullBand = -1
  let panning = $state(false)
  let moved = false
  let start = { x: 0, y: 0, tx: 0, ty: 0, id: -1 }
  let fittedFor: string | number | null = null
  let rafId = 0

  const live: ViewRect = $derived({
    x0: -frame.tx / frame.k,
    y0: -frame.ty / frame.k,
    x1: (cw - frame.tx) / frame.k,
    y1: (ch - frame.ty) / frame.k,
    k: frame.k,
  })

  function zoomBand(zoom: number): number {
    let band = 0
    for (let i = 0; i < ZOOM_STEPS.length; i++) {
      const step = ZOOM_STEPS[i]
      // Stay in the current band until the zoom moves clearly past the edge.
      let edge = step
      if (cullBand === i) edge = step * 1.08
      else if (cullBand > i) edge = step * 0.92
      if (zoom >= edge) band++
      else break
    }
    return band
  }

  function liveNow(): ViewRect {
    return {
      x0: -tx / k,
      y0: -ty / k,
      x1: (cw - tx) / k,
      y1: (ch - ty) / k,
      k,
    }
  }

  /** Recompute the padded culling rect when the viewport leaves it or the zoom band changes. */
  function considerCull() {
    if (!cw || !ch || !(k > 0)) return
    const now = liveNow()
    const band = zoomBand(k)
    if (
      cull &&
      cullBand === band &&
      now.x0 >= cull.x0 &&
      now.y0 >= cull.y0 &&
      now.x1 <= cull.x1 &&
      now.y1 <= cull.y1
    ) {
      return
    }
    const padX = (now.x1 - now.x0) * 0.5
    const padY = (now.y1 - now.y0) * 0.5
    cull = {
      x0: now.x0 - padX,
      y0: now.y0 - padY,
      x1: now.x1 + padX,
      y1: now.y1 + padY,
      k: now.k,
    }
    cullBand = band
  }

  function publishNow() {
    if (rafId) {
      cancelAnimationFrame(rafId)
      rafId = 0
    }
    frame = { tx, ty, k }
    considerCull()
  }

  function schedulePublish() {
    if (rafId) return
    rafId = requestAnimationFrame(() => {
      rafId = 0
      frame = { tx, ty, k }
      considerCull()
    })
  }

  $effect(() => {
    onview?.(live)
  })

  $effect(() => {
    return () => {
      if (rafId) cancelAnimationFrame(rafId)
    }
  })

  export function fit(): void {
    if (!cw || !ch || !contentWidth || !contentHeight) return
    k = Math.max(minK, Math.min(maxK, Math.min(cw / contentWidth, ch / contentHeight) * 0.96))
    tx = (cw - contentWidth * k) / 2
    ty = Math.max(8, (ch - contentHeight * k) / 2)
    publishNow()
  }

  /** Center the viewport on a world point, optionally changing zoom. */
  export function centerOn(x: number, y: number, zoom?: number): void {
    k = Math.max(minK, Math.min(maxK, zoom ?? k))
    tx = cw / 2 - x * k
    ty = ch / 2 - y * k
    publishNow()
  }

  export function zoomBy(factor: number): void {
    zoomAt(cw / 2, ch / 2, factor, true)
  }

  export function currentRect(): ViewRect {
    return liveNow()
  }

  /** The world point under a window position. */
  export function toContent(clientX: number, clientY: number): { x: number; y: number } {
    const box = el.getBoundingClientRect()
    return { x: (clientX - box.left - tx) / k, y: (clientY - box.top - ty) / k }
  }

  function zoomAt(px: number, py: number, factor: number, immediate = false) {
    const nk = Math.max(minK, Math.min(maxK, k * factor))
    const ratio = nk / k
    tx = px - (px - tx) * ratio
    ty = py - (py - ty) * ratio
    k = nk
    if (immediate) publishNow()
    else schedulePublish()
  }

  $effect(() => {
    const key = fitKey
    if (cw > 0 && ch > 0 && contentWidth > 0 && fittedFor !== key) {
      fittedFor = key
      fit()
    }
  })

  // Resize moves the viewport in world space even when the camera is unchanged.
  $effect(() => {
    void cw
    void ch
    considerCull()
  })

  $effect(() => {
    const node = el
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      const box = node.getBoundingClientRect()
      const px = e.clientX - box.left
      const py = e.clientY - box.top
      if (e.shiftKey) {
        tx -= e.deltaX || (Math.abs(e.deltaY) > Math.abs(e.deltaX) ? 0 : e.deltaX)
        ty -= Math.abs(e.deltaY) >= Math.abs(e.deltaX) ? e.deltaY : e.deltaX
        schedulePublish()
      } else if (e.ctrlKey || e.metaKey || Math.abs(e.deltaY) >= Math.abs(e.deltaX)) {
        zoomAt(px, py, Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0016)))
      } else {
        tx -= e.deltaX
        schedulePublish()
      }
    }
    node.addEventListener('wheel', onWheel, { passive: false })
    return () => node.removeEventListener('wheel', onWheel)
  })

  function down(e: PointerEvent) {
    if (e.button !== 0) return
    start = { x: e.clientX, y: e.clientY, tx, ty, id: e.pointerId }
    moved = false
  }

  function move(e: PointerEvent) {
    if (start.id !== e.pointerId) return
    const dx = e.clientX - start.x
    const dy = e.clientY - start.y
    if (!moved && Math.hypot(dx, dy) > 4) {
      moved = true
      panning = true
      el.setPointerCapture(e.pointerId)
    }
    if (moved) {
      tx = start.tx + dx
      ty = start.ty + dy
      schedulePublish()
    }
  }

  function up(e: PointerEvent) {
    if (start.id !== e.pointerId) return
    start.id = -1
    panning = false
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId)
  }

  function keyNav(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey) return
    const target = e.target
    if (target instanceof HTMLElement && target.closest('input, textarea, select, button')) return
    if (e.key === '+' || e.key === '=') {
      e.preventDefault()
      zoomBy(1.15)
    } else if (e.key === '-' || e.key === '_') {
      e.preventDefault()
      zoomBy(1 / 1.15)
    } else if (e.key === '0') {
      e.preventDefault()
      fit()
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault()
      tx += e.shiftKey ? 120 : 48
      schedulePublish()
    } else if (e.key === 'ArrowRight') {
      e.preventDefault()
      tx -= e.shiftKey ? 120 : 48
      schedulePublish()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      ty += e.shiftKey ? 120 : 48
      schedulePublish()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      ty -= e.shiftKey ? 120 : 48
      schedulePublish()
    }
  }

  function suppressClick(e: MouseEvent) {
    if (moved) {
      e.stopPropagation()
      e.preventDefault()
      moved = false
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="panzoom"
  class:panning
  bind:this={el}
  bind:clientWidth={cw}
  bind:clientHeight={ch}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onclickcapture={suppressClick}
  role="application"
  tabindex="0"
  aria-label="Graph. Plus and minus zoom, zero fits, arrows pan, shift and wheel pans."
  onkeydown={keyNav}
>
  <svg width={cw} height={ch}>
    {#key fadeKey}
      <g class="fade" transform={`translate(${frame.tx} ${frame.ty}) scale(${frame.k})`}>
        {#if cull}
          {@render children(cull)}
        {/if}
      </g>
    {/key}
  </svg>
  {#if hud}
    {@render hud(live)}
  {/if}
</div>

<style>
  .panzoom {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    cursor: grab;
    background: var(--bg-canvas);
    touch-action: none;
  }
  .panzoom:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .fade {
    animation: graph-in var(--dur-slow) var(--ease);
  }
  @keyframes graph-in {
    from {
      opacity: 0.35;
    }
  }
  .panzoom.panning {
    cursor: grabbing;
  }
  /* Hover strokes and drop shadows repaint the whole graph while dragging. */
  .panzoom.panning :global(.node) {
    pointer-events: none;
  }
  .panzoom.panning :global(.node.selected rect) {
    filter: none;
  }
  svg {
    display: block;
    user-select: none;
  }
</style>

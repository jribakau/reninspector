<script lang="ts">
  import { untrack } from 'svelte'

  interface Props {
    axis: 'x' | 'y'
    /** 1 if moving the pointer in the positive direction grows the pane. */
    grow: 1 | -1
    value: number
    min: number
    hardMax: number
    /** Largest share of the parent the pane may take. */
    fraction?: number
    /** Pixels that must remain for the rest of the parent. */
    reserve?: number
    reset: number
    onchange: (next: number) => void
  }

  let {
    axis,
    grow,
    value,
    min,
    hardMax,
    fraction = 0.45,
    reserve = 240,
    reset,
    onchange,
  }: Props = $props()

  let el: HTMLDivElement | undefined = $state()

  function limit(next: number): number {
    const parent = el?.parentElement
    const container = parent ? (axis === 'x' ? parent.clientWidth : parent.clientHeight) : 0
    if (!parent || container < 50) return Number.isFinite(next) && next > 0 ? next : reset
    const max = Math.max(min, Math.min(hardMax, Math.floor(container * fraction), container - reserve))
    if (!Number.isFinite(next)) return Math.min(max, Math.max(min, reset))
    return Math.min(max, Math.max(min, next))
  }

  function apply(next: number) {
    const clamped = limit(next)
    if (clamped !== value) onchange(clamped)
  }

  function down(e: PointerEvent) {
    if (e.button !== 0) return
    e.preventDefault()
    const target = e.currentTarget
    if (!(target instanceof HTMLElement)) return
    target.setPointerCapture(e.pointerId)
    const start = axis === 'x' ? e.clientX : e.clientY
    const base = value
    const move = (ev: PointerEvent) => {
      const delta = (axis === 'x' ? ev.clientX : ev.clientY) - start
      apply(base + grow * delta)
    }
    const up = (ev: PointerEvent) => {
      if (target.hasPointerCapture(ev.pointerId)) target.releasePointerCapture(ev.pointerId)
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      target.removeEventListener('pointercancel', up)
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
    target.addEventListener('pointercancel', up)
  }

  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 48 : 16
    if (axis === 'x' && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      e.preventDefault()
      const dir = e.key === 'ArrowRight' ? 1 : -1
      apply(value + grow * dir * step)
    } else if (axis === 'y' && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault()
      const dir = e.key === 'ArrowDown' ? 1 : -1
      apply(value + grow * dir * step)
    } else if (e.key === 'Home') {
      e.preventDefault()
      apply(min)
    } else if (e.key === 'End') {
      e.preventDefault()
      apply(hardMax)
    }
  }

  $effect(() => {
    const node = el
    if (!node) return
    const onResize = () => apply(untrack(() => value))
    window.addEventListener('resize', onResize)
    const frame = requestAnimationFrame(onResize)
    return () => {
      cancelAnimationFrame(frame)
      window.removeEventListener('resize', onResize)
    }
  })
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={el}
  class="split"
  class:h={axis === 'y'}
  role="separator"
  aria-orientation={axis === 'x' ? 'vertical' : 'horizontal'}
  aria-valuemin={min}
  aria-valuemax={hardMax}
  aria-valuenow={Math.round(value)}
  tabindex="0"
  onpointerdown={down}
  ondblclick={() => apply(reset)}
  onkeydown={key}
></div>

<style>
  .split {
    flex: none;
    width: 5px;
    margin: 0 -2px;
    cursor: col-resize;
    z-index: var(--z-split);
  }
  .split.h {
    width: auto;
    height: 5px;
    margin: -2px 0;
    cursor: row-resize;
  }
  .split:hover,
  .split:focus-visible {
    background: color-mix(in srgb, var(--accent) 50%, transparent);
  }
</style>

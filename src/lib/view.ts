/**
 * Row heights for virtual lists. One-line rows are `sm`, a name over a meta line is `md`,
 * rows with a preview or second meta line are `lg`, and rows that carry an input are `xl`/`xxl`.
 */
export const ROW = { tree: 24, sm: 28, md: 40, lg: 52, xl: 64, xxl: 78 } as const

/** Visible region in world coordinates plus the current zoom factor. */
export interface ViewRect {
  x0: number
  y0: number
  x1: number
  y1: number
  k: number
}

export function intersects(
  v: ViewRect,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  margin = 0,
): boolean {
  return x1 >= v.x0 - margin && x0 <= v.x1 + margin && y1 >= v.y0 - margin && y0 <= v.y1 + margin
}

export function truncate(text: string, max: number): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat
}

/** Labels stay on while the screen holds this many nodes. */
export const NODE_BUDGET = 400
/** Every on-screen node is still drawn, as a rectangle, up to this count. */
export const NODE_RECT_BUDGET = 800
/** Above this, the label graph draws its cheap level. */
export const EDGE_BUDGET = 800
/** Above this, the project map joins edges of the same kind into one path. */
export const DETAIL_EDGES = 160

export interface Box {
  x0: number
  y0: number
  x1: number
  y1: number
  /** Item this box belongs to, when several boxes share one edge. Defaults to the array index. */
  id?: number
}

/** One box per item, or each segment box when a cached layout stored them. */
export function indexBoxes(
  items: readonly { x0: number; y0: number; x1: number; y1: number; segs?: readonly Box[] }[],
): Box[] {
  const out: Box[] = []
  for (let i = 0; i < items.length; i++) {
    const segs = items[i].segs
    if (segs && segs.length) {
      for (const s of segs) out.push({ x0: s.x0, y0: s.y0, x1: s.x1, y1: s.y1, id: i })
    } else {
      const b = items[i]
      out.push({ x0: b.x0, y0: b.y0, x1: b.x1, y1: b.y1, id: i })
    }
  }
  return out
}

/** Join path data of the same kind. `skip` leaves those edges for a separate highlight path. */
export function batchPaths<T extends { d: string; kind: string }>(
  edges: readonly T[],
  skip: (edge: T) => boolean,
): { kind: string; d: string }[] {
  const parts = new Map<string, string[]>()
  for (const e of edges) {
    if (skip(e)) continue
    const bucket = parts.get(e.kind)
    if (bucket) bucket.push(e.d)
    else parts.set(e.kind, [e.d])
  }
  const out: { kind: string; d: string }[] = []
  for (const [kind, ds] of parts) out.push({ kind, d: ds.join(' ') })
  return out
}

export interface SpatialIndex {
  /** Indices of boxes that overlap the query rectangle. */
  query(x0: number, y0: number, x1: number, y1: number): number[]
}

/**
 * Uniform grid over world-space boxes. Boxes that cover many cells are kept in
 * a side list so one long edge cannot fill the grid. A query that covers more
 * cells than there are boxes falls back to a linear scan.
 */
export function spatialIndex(boxes: readonly Box[], cell = 512): SpatialIndex {
  let owners = boxes.length
  for (let i = 0; i < boxes.length; i++) {
    const id = boxes[i].id
    if (id !== undefined && id + 1 > owners) owners = id + 1
  }
  const buckets = new Map<string, number[]>()
  const always: number[] = []
  const maxSpan = 8
  for (let i = 0; i < boxes.length; i++) {
    const b = boxes[i]
    const x0 = Math.floor(b.x0 / cell)
    const x1 = Math.floor(b.x1 / cell)
    const y0 = Math.floor(b.y0 / cell)
    const y1 = Math.floor(b.y1 / cell)
    if (x1 - x0 > maxSpan || y1 - y0 > maxSpan) {
      always.push(i)
      continue
    }
    for (let x = x0; x <= x1; x++) {
      for (let y = y0; y <= y1; y++) {
        const key = `${x},${y}`
        const list = buckets.get(key)
        if (list) list.push(i)
        else buckets.set(key, [i])
      }
    }
  }

  const hit = (i: number, x0: number, y0: number, x1: number, y1: number) => {
    const b = boxes[i]
    return b.x1 >= x0 && b.x0 <= x1 && b.y1 >= y0 && b.y0 <= y1
  }

  const owner = (i: number) => boxes[i].id ?? i

  return {
    query(qx0, qy0, qx1, qy1) {
      const x0 = Math.floor(qx0 / cell)
      const x1 = Math.floor(qx1 / cell)
      const y0 = Math.floor(qy0 / cell)
      const y1 = Math.floor(qy1 / cell)
      const nx = x1 - x0 + 1
      const ny = y1 - y0 + 1
      const seen = new Uint8Array(owners)
      const out: number[] = []
      const take = (i: number) => {
        const id = owner(i)
        if (seen[id]) return
        if (!hit(i, qx0, qy0, qx1, qy1)) return
        seen[id] = 1
        out.push(id)
      }
      if (nx > 64 || ny > 64 || nx * ny > boxes.length) {
        for (let i = 0; i < boxes.length; i++) take(i)
        return out
      }
      for (let x = x0; x <= x1; x++) {
        for (let y = y0; y <= y1; y++) {
          const list = buckets.get(`${x},${y}`)
          if (!list) continue
          for (const i of list) take(i)
        }
      }
      for (const i of always) take(i)
      return out
    },
  }
}

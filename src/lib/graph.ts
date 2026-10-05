import { NODE_BUDGET, NODE_RECT_BUDGET } from './view'

export function wrapText(text: string, chars: number): string[] {
  const words = text.replace(/\s+/g, ' ').trim().split(' ').filter(Boolean)
  const lines: string[] = []
  let cur = ''
  for (const word of words) {
    const next = cur ? `${cur} ${word}` : word
    if (cur && next.length > chars) {
      lines.push(cur)
      cur = word
    } else cur = next
  }
  if (cur) lines.push(cur)
  return lines.slice(0, 6)
}

export function rangeText(line: number, endLine: number): string {
  return endLine > line ? `lines ${line}–${endLine}` : `line ${line}`
}

export interface EdgePath {
  d: string
  index: number
}

export interface EdgeKind {
  from: number
  to: number
  kind: string
}

/** Group edge paths by kind, and pull edges that touch the selection into one hot path. */
export function packEdges(
  ids: readonly number[],
  selected: number | null,
  layoutEdges: readonly EdgePath[],
  graphEdges: readonly EdgeKind[],
): { batches: { kind: string; d: string }[]; hot: string } {
  const batches: { kind: string; d: string }[] = []
  const parts = new Map<string, string[]>()
  let hot = ''
  for (const i of ids) {
    const e = layoutEdges[i]
    const ge = e ? graphEdges[e.index] : undefined
    if (!e || !ge) continue
    if (selected !== null && (ge.from === selected || ge.to === selected)) {
      hot = hot ? `${hot} ${e.d}` : e.d
      continue
    }
    const bucket = parts.get(ge.kind)
    if (bucket) bucket.push(e.d)
    else parts.set(ge.kind, [e.d])
  }
  for (const [kind, ds] of parts) batches.push({ kind, d: ds.join(' ') })
  return { batches, hot }
}

export interface HitBox {
  id: number
  x: number
  y: number
  w: number
  h: number
}

/** First node box under the point, skipping `skip`. `accept` drops boxes that cannot be the target. */
export function nodeAt(
  nodes: readonly HitBox[],
  x: number,
  y: number,
  skip: number,
  accept?: (box: HitBox) => boolean,
): HitBox | null {
  for (const p of nodes) {
    if (p.id === skip || x < p.x || x > p.x + p.w || y < p.y || y > p.y + p.h) continue
    if (accept && !accept(p)) continue
    return p
  }
  return null
}

/** Zoom at which the map stops drawing nodes and shows group boxes alone. */
export const LOD_NODES_K = 0.14
/** Zoom at which node titles are drawn. Below it a title would be smaller than the eye can read. */
export const LOD_TITLES_K = 0.4
/** Zoom at which nodes also get their second line. */
export const LOD_DETAIL_K = 0.85

export interface MapLevel {
  /** False means group boxes only. */
  show: boolean
  /** 0 is a rectangle, 1 adds the title, 2 adds the second line. */
  tier: 0 | 1 | 2
}

/**
 * One detail level for everything on the screen. Zoom picks it, and a crowded
 * screen steps down so labels never fight for space.
 */
export function mapLevel(k: number, count: number): MapLevel {
  const show = k >= LOD_NODES_K && count <= NODE_RECT_BUDGET
  const tier = k >= LOD_DETAIL_K && count <= 160 ? 2 : k >= LOD_TITLES_K && count <= NODE_BUDGET ? 1 : 0
  return { show, tier }
}

/** Which look a node gets on the mark layer. Start scenes win, then missing and unreachable ones. */
export function markState(n: { kind: string; root: boolean; reachable: boolean } | undefined): string {
  if (!n) return 'label'
  if (n.root) return 'root'
  if (n.kind === 'missing') return 'missing'
  if (!n.reachable) return 'unreachable'
  return n.kind
}

/** A node as path data, rounded to a tenth so a thousand of them stay short. */
export function markRect(x: number, y: number, w: number, h: number): string {
  const r = (v: number) => String(Math.round(v * 10) / 10)
  return `M${r(x)} ${r(y)}h${r(w)}v${r(h)}h${r(-w)}z`
}

/** The box a point falls in, or the nearest one within `reach`. Marks are fatter than their nodes, so a click beside a node still counts. */
export function nearestBox<T extends { x: number; y: number; w: number; h: number }>(
  boxes: Iterable<T>,
  px: number,
  py: number,
  reach: number,
): T | null {
  let best: T | null = null
  let bestDist = Infinity
  for (const b of boxes) {
    const dist = Math.hypot(Math.max(b.x - px, 0, px - (b.x + b.w)), Math.max(b.y - py, 0, py - (b.y + b.h)))
    if (dist <= reach && dist < bestDist) {
      best = b
      bestDist = dist
    }
  }
  return best
}

/**
 * Title size in world units. It grows as the map shrinks so the text keeps a
 * readable size on screen, and stops at the node height.
 */
export function titleFont(k: number, base = 13, max = 24, screenPx = 9.5): number {
  if (!(k > 0)) return max
  return Math.min(max, Math.max(base, screenPx / k))
}

/** Multiplier that keeps outlines and lines about a pixel wide when the map is zoomed out. */
export function hairline(k: number): number {
  if (!(k > 0)) return 6
  return Math.min(6, Math.max(1, 1.1 / k))
}

/** Characters of a title that fit a node. 0.58 em is the average advance of the UI font. */
export function fitChars(width: number, font: number, reserve: number): number {
  return Math.max(3, Math.floor((width - reserve) / (font * 0.58)))
}

export interface RoutePoint {
  x: number
  y: number
}

function routeNum(n: number): string {
  return String(Math.round(n * 10) / 10)
}

/**
 * Orthogonal polyline with quadratic corners. The radius shrinks to half of the
 * shorter side of a corner, so neighbouring bends meet but do not cross.
 */
export function roundedPath(points: readonly RoutePoint[], radius: number): string {
  if (points.length === 0) return ''
  const start = `M${routeNum(points[0].x)} ${routeNum(points[0].y)}`
  if (points.length === 1 || radius <= 0) {
    return points.slice(1).reduce((d, p) => `${d} L${routeNum(p.x)} ${routeNum(p.y)}`, start)
  }
  let d = start
  for (let i = 1; i < points.length - 1; i++) {
    const prev = points[i - 1]
    const cur = points[i]
    const next = points[i + 1]
    const dx1 = cur.x - prev.x
    const dy1 = cur.y - prev.y
    const dx2 = next.x - cur.x
    const dy2 = next.y - cur.y
    const lenIn = Math.hypot(dx1, dy1)
    const lenOut = Math.hypot(dx2, dy2)
    const cross = dx1 * dy2 - dy1 * dx2
    const turn = lenIn >= 0.5 && lenOut >= 0.5 && Math.abs(cross) > 0.01 * lenIn * lenOut
    if (!turn) {
      d += ` L${routeNum(cur.x)} ${routeNum(cur.y)}`
      continue
    }
    const rad = Math.min(radius, lenIn / 2, lenOut / 2)
    const inX = cur.x - (dx1 / lenIn) * rad
    const inY = cur.y - (dy1 / lenIn) * rad
    const outX = cur.x + (dx2 / lenOut) * rad
    const outY = cur.y + (dy2 / lenOut) * rad
    d += ` L${routeNum(inX)} ${routeNum(inY)} Q${routeNum(cur.x)} ${routeNum(cur.y)} ${routeNum(outX)} ${routeNum(outY)}`
  }
  const last = points[points.length - 1]
  return `${d} L${routeNum(last.x)} ${routeNum(last.y)}`
}

export interface StoryNode {
  id: string
  root: boolean
  /** Missing targets sort after every other node. */
  missing?: boolean
}

export interface StoryLink {
  from: string
  to: string
}

/**
 * Rank from the nearest root, following links forward. Unreached nodes share the
 * rank after the deepest reached node. Missing nodes share the rank after that.
 */
export function storyDepth(nodes: readonly StoryNode[], edges: readonly StoryLink[]): Map<string, number> {
  const missing = new Set<string>()
  const known = new Set<string>()
  for (const n of nodes) {
    known.add(n.id)
    if (n.missing) missing.add(n.id)
  }
  const next = new Map<string, string[]>()
  for (const id of known) next.set(id, [])
  for (const e of edges) {
    if (e.from === e.to || !known.has(e.from) || !known.has(e.to)) continue
    next.get(e.from)!.push(e.to)
  }
  const depth = new Map<string, number>()
  const queue: string[] = []
  for (const n of nodes) {
    if (!n.root || missing.has(n.id) || depth.has(n.id)) continue
    depth.set(n.id, 0)
    queue.push(n.id)
  }
  for (let i = 0; i < queue.length; i++) {
    const id = queue[i]
    const d = depth.get(id)!
    for (const to of next.get(id) ?? []) {
      if (missing.has(to) || depth.has(to)) continue
      depth.set(to, d + 1)
      queue.push(to)
    }
  }
  let maxReached = -1
  for (const d of depth.values()) if (d > maxReached) maxReached = d
  const unreached = maxReached + 1
  const last = unreached + 1
  for (const n of nodes) {
    if (depth.has(n.id)) continue
    depth.set(n.id, n.missing ? last : unreached)
  }
  return depth
}

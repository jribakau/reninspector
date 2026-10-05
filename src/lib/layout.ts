import ELK from 'elkjs/lib/elk-api'
import type { ElkNode, ElkExtendedEdge } from 'elkjs/lib/elk-api'
import elkWorkerUrl from 'elkjs/lib/elk-worker.min.js?url'
import { api } from './api'
import { roundedPath, storyDepth, type RoutePoint } from './graph'
import { HELPERS_KEY, MISSING_KEY, UNLINKED_KEY, assignGroups, findHelpers, isTileGroup, type GroupMode } from './mapgroups'
import type { FileInfo, GNode, LabelGraph, MapEdge, MapNode, ProjectMap } from './types'

// ---------------------------------------------------------------- worker

// ELK runs its layered algorithm in a Web Worker (elk-worker.min.js); elk-api wraps it in promises.
let elk: InstanceType<typeof ELK> | null = null

export function elkLayout(graph: ElkNode): Promise<ElkNode> {
  elk ??= new ELK({ workerUrl: elkWorkerUrl })
  return elk.layout(graph)
}

// ---------------------------------------------------------------- shared

function pathOf(points: { x: number; y: number }[]): string {
  return points.map((p, i) => `${i === 0 ? 'M' : 'L'}${round(p.x)} ${round(p.y)}`).join(' ')
}

function round(n: number): number {
  return Math.round(n * 10) / 10
}

export interface SegBox {
  x0: number
  y0: number
  x1: number
  y1: number
}

/** Boxes along a polyline, split so a long stroke does not cover the whole map. */
function segmentBoxes(pts: { x: number; y: number }[], maxLen = 2048): SegBox[] {
  const out: SegBox[] = []
  for (let i = 1; i < pts.length; i++) {
    const a = pts[i - 1]
    const b = pts[i]
    const dx = b.x - a.x
    const dy = b.y - a.y
    const pieces = Math.max(1, Math.ceil(Math.hypot(dx, dy) / maxLen))
    for (let s = 0; s < pieces; s++) {
      const t0 = s / pieces
      const t1 = (s + 1) / pieces
      const x0 = a.x + dx * t0
      const y0 = a.y + dy * t0
      const x1 = a.x + dx * t1
      const y1 = a.y + dy * t1
      out.push({
        x0: Math.min(x0, x1) - 2,
        y0: Math.min(y0, y1) - 2,
        x1: Math.max(x0, x1) + 2,
        y1: Math.max(y0, y1) + 2,
      })
    }
  }
  return out
}

function sectionPoints(edge: ElkExtendedEdge, dx: number, dy: number): { x: number; y: number }[] {
  const s = edge.sections?.[0]
  if (!s) return []
  const pts = [s.startPoint, ...(s.bendPoints ?? []), s.endPoint]
  return pts.map((p) => ({ x: p.x + dx, y: p.y + dy }))
}

function hashString(s: string): number {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

export function clusterHue(key: string): number {
  return hashString(key) % 360
}

// ---------------------------------------------------------------- project map

export type ClusterMode = GroupMode

export interface LNode {
  id: string
  x: number
  y: number
  w: number
  h: number
  c: number
}

export interface LCluster {
  key: string
  x: number
  y: number
  w: number
  h: number
  count: number
}

export interface LEdge {
  from: string
  to: string
  kind: MapEdge['kind']
  count: number
  d: string
  /** Fan from the source node to the shared trunk, then from the trunk to the target. Absent inside a group. */
  fan?: string
  /** A link to or from a side group (helpers or unlinked scenes). Drawn only for the selected scene. */
  helper?: boolean
  /** Overall box. Used when a cached layout has no segment boxes. */
  x0: number
  y0: number
  x1: number
  y1: number
  /** Polyline or curve pieces. Absent on layouts cached before segment indexing. */
  segs?: SegBox[]
  intra: boolean
}

export interface LClusterEdge {
  from: number
  to: number
  weight: number
  d: string
  x0: number
  y0: number
  x1: number
  y1: number
  segs?: SegBox[]
}

export interface MapLayout {
  nodes: LNode[]
  clusters: LCluster[]
  edges: LEdge[]
  clusterEdges: LClusterEdge[]
  width: number
  height: number
}

/** Tall enough for a title at the largest zoomed-out size, or a title over a second line. */
export const NODE_H = 44
const CLUSTER_PAD = 26
/** Room for the group name, which is drawn at 16px when the group is open. */
const CLUSTER_HEADER = 44

export function mapNodeWidth(n: { id: string }): number {
  return Math.max(112, Math.min(260, n.id.length * 7.6 + 40))
}

export function mapCacheKey(root: string, map: ProjectMap, mode: ClusterMode, helpers = true): string {
  let h = 2166136261
  for (const n of map.nodes) {
    // Helper detection reads these, so a change to them must not reuse a stored layout.
    h = Math.imul(h ^ hashString(`${n.id}|${n.kind}|${n.returns ? 1 : 0}|${n.root ? 1 : 0}|${n.endsScript ? 1 : 0}`), 16777619)
  }
  for (const e of map.edges) h = Math.imul(h ^ hashString(`${e.from}>${e.to}:${e.kind}`), 16777619)
  return `map|v6|${root}|${mode}|${helpers ? 'h' : 'n'}|${map.nodes.length}|${map.edges.length}|${h >>> 0}`
}

const INNER_OPTIONS = {
  'elk.algorithm': 'layered',
  'elk.direction': 'DOWN',
  'elk.edgeRouting': 'ORTHOGONAL',
  'elk.spacing.nodeNode': '32',
  'elk.layered.spacing.nodeNodeBetweenLayers': '60',
  'elk.spacing.edgeNode': '20',
  'elk.spacing.edgeEdge': '14',
  'elk.layered.spacing.edgeNodeBetweenLayers': '20',
  'elk.layered.spacing.edgeEdgeBetweenLayers': '14',
  'elk.layered.thoroughness': '3',
  'elk.layered.cycleBreaking.strategy': 'MODEL_ORDER',
  'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
  'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
}

const OUTER_OPTIONS = {
  'elk.algorithm': 'layered',
  'elk.direction': 'RIGHT',
  'elk.edgeRouting': 'ORTHOGONAL',
  'elk.spacing.nodeNode': '110',
  'elk.layered.spacing.nodeNodeBetweenLayers': '220',
  'elk.spacing.edgeNode': '40',
  'elk.spacing.edgeEdge': '24',
  'elk.layered.spacing.edgeNodeBetweenLayers': '40',
  'elk.layered.spacing.edgeEdgeBetweenLayers': '24',
  'elk.layered.cycleBreaking.strategy': 'MODEL_ORDER',
  'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
}

const ROUTE_RADIUS = 8

/**
 * Ids ELK can pin to the first layer. A FIRST node with an incoming edge makes
 * the layout throw, so a root or entry that something links to stays unpinned
 * and story order is the only hint.
 */
export function firstLayerIds(
  orderedIds: readonly string[],
  incoming: ReadonlySet<string>,
  roots: ReadonlySet<string>,
): string[] {
  const open = orderedIds.filter((id) => !incoming.has(id))
  const rooted = open.filter((id) => roots.has(id))
  if (rooted.length) return rooted
  return open.length ? [open[0]] : []
}

function samePoint(a: RoutePoint, b: RoutePoint): boolean {
  return Math.abs(a.x - b.x) < 0.5 && Math.abs(a.y - b.y) < 0.5
}

function dedupePoints(pts: RoutePoint[]): RoutePoint[] {
  const out: RoutePoint[] = []
  for (const p of pts) {
    const last = out[out.length - 1]
    if (last && samePoint(last, p)) continue
    out.push(p)
  }
  return out
}

/** Horizontal-then-vertical, or vertical-then-horizontal, elbow. */
function elbow(from: RoutePoint, to: RoutePoint, horizontalFirst: boolean): RoutePoint[] {
  if (Math.abs(from.x - to.x) < 0.5 || Math.abs(from.y - to.y) < 0.5) return dedupePoints([from, to])
  const mid = horizontalFirst ? { x: to.x, y: from.y } : { x: from.x, y: to.y }
  return [from, mid, to]
}

function sidePort(n: { x: number; y: number; w: number; h: number }, towardX: number): RoutePoint {
  return {
    x: towardX >= n.x + n.w / 2 ? n.x + n.w : n.x,
    y: n.y + n.h / 2,
  }
}

function boundsOf(pts: RoutePoint[]): { x0: number; y0: number; x1: number; y1: number } {
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -Infinity
  let y1 = -Infinity
  for (const p of pts) {
    if (p.x < x0) x0 = p.x
    if (p.y < y0) y0 = p.y
    if (p.x > x1) x1 = p.x
    if (p.y > y1) y1 = p.y
  }
  return { x0, y0, x1, y1 }
}

export interface MapLayoutProgress {
  (done: number, total: number): void
}

export interface MapLayoutOptions {
  /** Pull shared subroutines out of the story into a lane of their own. On by default. */
  helpers?: boolean
}

/** Space between the story and the column of helpers beside it, and between boxes in that column. */
const ASIDE_GAP = 160
const ASIDE_ROW_GAP = 60
const TILE_GAP_X = 24
const TILE_GAP_Y = 18

/** Tiles in rows, about as wide as they are tall. For groups with no flow to show. */
function tileLayout(members: readonly MapNode[]): {
  pos: Map<string, { x: number; y: number; w: number; h: number }>
  w: number
  h: number
} {
  const pos = new Map<string, { x: number; y: number; w: number; h: number }>()
  if (!members.length) return { pos, w: 0, h: 0 }
  const widths = members.map(mapNodeWidth)
  const cellW = Math.max(...widths) + TILE_GAP_X
  const cellH = NODE_H + TILE_GAP_Y
  const cols = Math.max(1, Math.round(Math.sqrt((members.length * cellH) / cellW)))
  const rows = Math.ceil(members.length / cols)
  members.forEach((m, i) => {
    pos.set(m.id, { x: (i % cols) * cellW, y: Math.floor(i / cols) * cellH, w: widths[i], h: NODE_H })
  })
  return { pos, w: cols * cellW - TILE_GAP_X, h: rows * cellH - TILE_GAP_Y }
}

/** Horizontal, vertical, horizontal. A readable route between two nodes in different groups. */
function zRoute(from: RoutePoint, to: RoutePoint): RoutePoint[] {
  const mid = (from.x + to.x) / 2
  return dedupePoints([from, { x: mid, y: from.y }, { x: mid, y: to.y }, to])
}

export async function layoutProjectMap(
  map: ProjectMap,
  files: FileInfo[],
  mode: ClusterMode,
  key: string,
  progress?: MapLayoutProgress,
  options: MapLayoutOptions = {},
): Promise<MapLayout> {
  try {
    const cached = await api.layoutCacheGet(key)
    if (cached) return JSON.parse(cached) as MapLayout
  } catch {
    /* cache is best effort */
  }

  const helperSet = options.helpers === false ? new Set<string>() : findHelpers(map.nodes, map.edges)
  const clusterOf = assignGroups(map.nodes, map.edges, files, mode, helperSet)
  // Helpers are reached from everywhere, so they would flatten the story's depth.
  const depth = storyDepth(
    map.nodes.map((n) => ({ id: n.id, root: n.root, missing: n.kind === 'missing' })),
    map.edges.filter((e) => !helperSet.has(e.from) && !helperSet.has(e.to)),
  )
  const byDepth = (a: { id: string }, b: { id: string }) => {
    const d = (depth.get(a.id) ?? 0) - (depth.get(b.id) ?? 0)
    if (d) return d
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0
  }
  const keys = [...new Set(clusterOf.values())].sort()
  const clusterIndex = new Map(keys.map((k, i) => [k, i]))
  const byCluster: MapNode[][] = keys.map(() => [])
  for (const n of map.nodes) byCluster[clusterIndex.get(clusterOf.get(n.id)!)!].push(n)
  for (const group of byCluster) group.sort(byDepth)

  // 1. lay out each cluster on its own
  interface Inner {
    key: string
    members: MapNode[]
    pos: Map<string, { x: number; y: number; w: number; h: number }>
    intra: { edge: MapEdge; points: { x: number; y: number }[] }[]
    w: number
    h: number
  }
  const inners: Inner[] = []
  for (let ci = 0; ci < keys.length; ci++) {
    const members = byCluster[ci]
    const ids = new Set(members.map((m) => m.id))
    const intraEdges = map.edges
      .filter((e) => ids.has(e.from) && ids.has(e.to) && e.from !== e.to)
      .sort((a, b) => {
        const from = (depth.get(a.from) ?? 0) - (depth.get(b.from) ?? 0)
        if (from) return from
        const to = (depth.get(a.to) ?? 0) - (depth.get(b.to) ?? 0)
        if (to) return to
        if (a.kind !== b.kind) return a.kind < b.kind ? -1 : 1
        return a.to < b.to ? -1 : a.to > b.to ? 1 : 0
      })
    if (isTileGroup(keys[ci])) {
      const tiles = tileLayout([...members].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)))
      inners.push({
        key: keys[ci],
        members,
        pos: tiles.pos,
        intra: [],
        w: tiles.w + CLUSTER_PAD * 2,
        h: tiles.h + CLUSTER_PAD * 2 + CLUSTER_HEADER,
      })
      progress?.(ci + 1, keys.length + 1)
      continue
    }
    const incoming = new Set(intraEdges.map((e) => e.to))
    const pinned = new Set(
      keys[ci] === MISSING_KEY
        ? []
        : firstLayerIds(
            members.map((m) => m.id),
            incoming,
            new Set(members.filter((m) => m.root).map((m) => m.id)),
          ),
    )
    const graph: ElkNode = {
      id: `c${ci}`,
      layoutOptions: INNER_OPTIONS,
      children: members.map((m) => ({
        id: m.id,
        width: mapNodeWidth(m),
        height: NODE_H,
        layoutOptions: pinned.has(m.id) ? { 'elk.layered.layering.layerConstraint': 'FIRST' } : undefined,
      })),
      edges: intraEdges.map((e, i) => ({ id: `e${i}`, sources: [e.from], targets: [e.to] })),
    }
    const res = await elkLayout(graph)
    const pos = new Map<string, { x: number; y: number; w: number; h: number }>()
    for (const c of res.children ?? []) {
      pos.set(c.id, { x: c.x ?? 0, y: c.y ?? 0, w: c.width ?? 0, h: c.height ?? 0 })
    }
    const routed = new Map<number, RoutePoint[]>()
    for (let i = 0; i < (res.edges ?? []).length; i++) {
      const edge = res.edges![i]
      const parsed = Number(edge.id?.slice(1))
      routed.set(Number.isNaN(parsed) ? i : parsed, sectionPoints(edge as ElkExtendedEdge, 0, 0))
    }
    const intra = intraEdges.map((e, i) => ({
      edge: e,
      points: routed.get(i) ?? [],
    }))
    inners.push({
      key: keys[ci],
      members,
      pos,
      intra,
      w: (res.width ?? 0) + CLUSTER_PAD * 2,
      h: (res.height ?? 0) + CLUSTER_PAD * 2 + CLUSTER_HEADER,
    })
    progress?.(ci + 1, keys.length + 1)
  }

  // 2. lay out the clusters relative to each other
  /** Helpers and unlinked scenes have no flow to follow, so they are set down beside the story. */
  const aside = new Set(inners.flatMap((inner, i) => (isTileGroup(inner.key) ? [i] : [])))
  const weights = new Map<string, number>()
  for (const e of map.edges) {
    const a = clusterIndex.get(clusterOf.get(e.from)!)!
    const b = clusterIndex.get(clusterOf.get(e.to)!)!
    // Those groups take no part in placing the story's groups.
    if (a === b || aside.has(a) || aside.has(b)) continue
    const k = `${a}>${b}`
    weights.set(k, (weights.get(k) ?? 0) + e.count)
  }
  /** Groups with no story flow sit after the story: unlinked, then missing targets, then helpers. */
  const tail = (key: string) => (key === UNLINKED_KEY ? 1 : key === MISSING_KEY ? 2 : key === HELPERS_KEY ? 3 : 0)
  const clusterDepth = inners.map((inner) => {
    let d = Infinity
    for (const m of inner.members) d = Math.min(d, depth.get(m.id) ?? Infinity)
    return d === Infinity ? 0 : d
  })
  const outerOrder = inners.map((_, i) => i).sort((a, b) => {
    const ta = tail(inners[a].key)
    const tb = tail(inners[b].key)
    if (ta !== tb) return ta - tb
    if (clusterDepth[a] !== clusterDepth[b]) return clusterDepth[a] - clusterDepth[b]
    return inners[a].key < inners[b].key ? -1 : inners[a].key > inners[b].key ? 1 : 0
  })
  const outerKeys = [...weights.keys()].sort((a, b) => {
    const [aFrom, aTo] = a.split('>').map(Number)
    const [bFrom, bTo] = b.split('>').map(Number)
    if (clusterDepth[aFrom] !== clusterDepth[bFrom]) return clusterDepth[aFrom] - clusterDepth[bFrom]
    if (clusterDepth[aTo] !== clusterDepth[bTo]) return clusterDepth[aTo] - clusterDepth[bTo]
    return a < b ? -1 : a > b ? 1 : 0
  })
  const clusterIn = new Set<number>()
  const clusterOut = new Set<number>()
  for (const k of weights.keys()) {
    const [a, b] = k.split('>').map(Number)
    clusterOut.add(a)
    clusterIn.add(b)
  }
  const outerGraph: ElkNode = {
    id: 'root',
    layoutOptions: OUTER_OPTIONS,
    children: outerOrder.filter((i) => !aside.has(i)).map((i) => {
      const inner = inners[i]
      // Same ELK rule as inside a group: FIRST forbids incoming edges, LAST forbids outgoing ones.
      const constraint =
        tail(inner.key) > 0 && !clusterOut.has(i)
          ? 'LAST'
          : inner.members.some((m) => m.root) && !clusterIn.has(i)
            ? 'FIRST'
            : ''
      return {
        id: `c${i}`,
        width: inner.w,
        height: inner.h,
        layoutOptions: constraint ? { 'elk.layered.layering.layerConstraint': constraint } : undefined,
      }
    }),
    edges: outerKeys.map((k, i) => {
      const [a, b] = k.split('>')
      return { id: `o${i}`, sources: [`c${a}`], targets: [`c${b}`] }
    }),
  }
  const outer = await elkLayout(outerGraph)
  progress?.(keys.length + 1, keys.length + 1)

  const placed = new Map<number, { x: number; y: number }>()
  for (const c of outer.children ?? []) placed.set(Number(c.id.slice(1)), { x: c.x ?? 0, y: c.y ?? 0 })
  let storyRight = 0
  for (const [i, p] of placed) storyRight = Math.max(storyRight, p.x + inners[i].w)
  const asideX = storyRight + (placed.size > 0 ? ASIDE_GAP : 0)
  let asideY = 0
  for (const i of [...aside].sort((a, b) => tail(inners[a].key) - tail(inners[b].key))) {
    placed.set(i, { x: asideX, y: asideY })
    asideY += inners[i].h + ASIDE_ROW_GAP
  }

  // 3. compose absolute coordinates
  const layoutNodes: LNode[] = []
  const layoutClusters: LCluster[] = []
  const nodePos = new Map<string, LNode>()
  const origin: { x: number; y: number }[] = []
  const margin = 60
  inners.forEach((inner, i) => {
    const oc = placed.get(i)
    const ox = (oc?.x ?? 0) + margin
    const oy = (oc?.y ?? 0) + margin
    origin.push({ x: ox, y: oy })
    layoutClusters.push({
      key: inner.key,
      x: ox,
      y: oy,
      w: inner.w,
      h: inner.h,
      count: inner.members.length,
    })
    for (const m of inner.members) {
      const p = inner.pos.get(m.id)!
      const ln: LNode = {
        id: m.id,
        x: ox + CLUSTER_PAD + p.x,
        y: oy + CLUSTER_HEADER + CLUSTER_PAD + p.y,
        w: p.w,
        h: p.h,
        c: i,
      }
      layoutNodes.push(ln)
      nodePos.set(m.id, ln)
    }
  })

  const trunks = new Map<string, RoutePoint[]>()
  for (let i = 0; i < (outer.edges ?? []).length; i++) {
    const edge = outer.edges![i]
    const parsed = Number(edge.id?.slice(1))
    const pair = outerKeys[Number.isNaN(parsed) ? i : parsed]
    if (!pair) continue
    const pts = dedupePoints(sectionPoints(edge as ElkExtendedEdge, margin, margin))
    if (pts.length >= 2) trunks.set(pair, pts)
  }
  const clusterTrunk = (a: number, b: number): RoutePoint[] => {
    const routed = trunks.get(`${a}>${b}`)
    if (routed && routed.length >= 2) return routed
    const ca = layoutClusters[a]
    const cb = layoutClusters[b]
    return elbow(sidePort(ca, cb.x + cb.w / 2), sidePort(cb, ca.x + ca.w / 2), true)
  }

  const edges: LEdge[] = []
  inners.forEach((inner, i) => {
    const dx = origin[i].x + CLUSTER_PAD
    const dy = origin[i].y + CLUSTER_HEADER + CLUSTER_PAD
    for (const { edge, points } of inner.intra) {
      const pts = dedupePoints(points.map((p) => ({ x: p.x + dx, y: p.y + dy })))
      if (pts.length < 2) continue
      const box = boundsOf(pts)
      edges.push({
        from: edge.from,
        to: edge.to,
        kind: edge.kind,
        count: edge.count,
        d: roundedPath(pts, ROUTE_RADIUS),
        x0: box.x0,
        y0: box.y0,
        x1: box.x1,
        y1: box.y1,
        segs: segmentBoxes(pts),
        intra: true,
      })
    }
  })
  for (const e of map.edges) {
    const a = nodePos.get(e.from)
    const b = nodePos.get(e.to)
    if (!a || !b || a.c === b.c) continue
    if (aside.has(a.c) || aside.has(b.c)) {
      const route = zRoute(sidePort(a, b.x + b.w / 2), sidePort(b, a.x + a.w / 2))
      if (route.length < 2) continue
      const hb = boundsOf(route)
      edges.push({
        from: e.from,
        to: e.to,
        kind: e.kind,
        count: e.count,
        d: roundedPath(route, ROUTE_RADIUS),
        helper: true,
        x0: hb.x0,
        y0: hb.y0,
        x1: hb.x1,
        y1: hb.y1,
        segs: segmentBoxes(route),
        intra: false,
      })
      continue
    }
    const trunk = clusterTrunk(a.c, b.c)
    const start = trunk[0]
    const end = trunk[trunk.length - 1]
    const fanOut = elbow(sidePort(a, start.x), start, true)
    const fanIn = elbow(end, sidePort(b, end.x), false)
    const full = dedupePoints([...fanOut, ...trunk.slice(1), ...fanIn.slice(1)])
    if (full.length < 2) continue
    const box = boundsOf(full)
    edges.push({
      from: e.from,
      to: e.to,
      kind: e.kind,
      count: e.count,
      d: roundedPath(full, ROUTE_RADIUS),
      fan: `${roundedPath(fanOut, ROUTE_RADIUS)} ${roundedPath(fanIn, ROUTE_RADIUS)}`.trim(),
      x0: box.x0,
      y0: box.y0,
      x1: box.x1,
      y1: box.y1,
      segs: segmentBoxes(full),
      intra: false,
    })
  }

  const clusterEdges: LClusterEdge[] = []
  for (const [k, weight] of weights) {
    const [a, b] = k.split('>').map(Number)
    const pts = clusterTrunk(a, b)
    if (pts.length < 2) continue
    const box = boundsOf(pts)
    clusterEdges.push({
      from: a,
      to: b,
      weight,
      d: roundedPath(pts, ROUTE_RADIUS),
      x0: box.x0,
      y0: box.y0,
      x1: box.x1,
      y1: box.y1,
      segs: segmentBoxes(pts),
    })
  }

  const layout: MapLayout = {
    nodes: layoutNodes,
    clusters: layoutClusters,
    edges,
    clusterEdges,
    width: Math.max(1, ...layoutClusters.map((c) => c.x + c.w)) + margin,
    height: Math.max(1, ...layoutClusters.map((c) => c.y + c.h)) + margin,
  }
  try {
    await api.layoutCachePut(key, JSON.stringify(layout))
  } catch {
    /* ignore */
  }
  return layout
}

// ---------------------------------------------------------------- label graph

export interface GLNode {
  id: number
  x: number
  y: number
  w: number
  h: number
}

export interface GLEdge {
  index: number
  d: string
  label: string | null
  cond: string | null
  /** Chip width reserved by the layout. Zero when the edge has no label. */
  lw: number
  lx: number
  ly: number
  x0: number
  y0: number
  x1: number
  y1: number
  segs?: SegBox[]
}

export interface GraphLayout {
  nodes: GLNode[]
  edges: GLEdge[]
  width: number
  height: number
}

/** Matches the 12px pill font closely enough that a label is not cut while it still fits. */
const PILL_CHAR = 6.4
const PILL_PAD = 36

export function graphPillText(n: GNode): string {
  switch (n.kind) {
    case 'jump':
      return n.dynamic ? `jump expression ${n.title}` : `jump ${n.title}`
    case 'fall':
      return `falls into ${n.title}`
    case 'call':
      return n.dynamic ? `call expression ${n.title}` : `call ${n.title}`
    case 'cond':
      return n.title.startsWith('while ') ? n.title : `if ${n.title}`
    case 'choice':
      return n.body || n.title
    case 'menu':
      return n.title === 'menu' ? 'menu' : `menu: ${n.title}`
    case 'return':
      return 'return'
    default:
      return n.title
  }
}

export function pillCharLimit(w: number): number {
  return Math.max(1, Math.floor((w - 24) / PILL_CHAR))
}

const CHIP_PAD = 16
const CHIP_MAX = 420

export function chipWidthFor(text: string): number {
  return Math.min(CHIP_MAX, Math.ceil(text.length * PILL_CHAR + CHIP_PAD))
}

export function chipCharLimit(w: number): number {
  return Math.max(1, Math.floor((w - CHIP_PAD) / PILL_CHAR))
}

function edgeText(label: string | null, cond: string | null): string {
  if (label && cond && !label.includes(cond)) return `${label} · ${cond}`
  return label || cond || ''
}

function pillWidth(text: string): number {
  return Math.max(130, Math.min(360, Math.ceil(text.length * PILL_CHAR + PILL_PAD)))
}

function cardLines(text: string): number {
  const flat = text.replace(/\s+/g, ' ').trim()
  if (!flat) return 1
  return Math.min(6, Math.ceil(flat.length / 38))
}

export function graphNodeSize(n: GNode): { w: number; h: number } {
  switch (n.kind) {
    case 'dialogue':
      if (n.body) return { w: 280, h: 44 + cardLines(n.body) * 16 }
      if (n.beats.length) return { w: 280, h: 16 + n.beats.length * 18 }
      if (n.says === 0 && n.title) return { w: Math.max(140, Math.min(280, pillWidth(n.title))), h: 36 }
      return { w: 280, h: 52 + n.preview.length * 15 + (n.variants || n.conds ? 18 : 0) }
    case 'choice':
      return { w: 280, h: 28 + cardLines(n.body || n.title) * 16 + (n.target ? 18 : 0) }
    case 'menu':
      return { w: Math.max(160, pillWidth(graphPillText(n))), h: 46 }
    case 'cond':
      return { w: 220, h: 42 }
    case 'jump':
    case 'fall':
    case 'call':
      return { w: pillWidth(graphPillText(n)), h: 32 }
    default:
      return { w: 120, h: 30 }
  }
}

function polylineMidpoint(pts: { x: number; y: number }[]): { x: number; y: number } {
  if (pts.length === 0) return { x: 0, y: 0 }
  let total = 0
  const segs: number[] = []
  for (let i = 1; i < pts.length; i++) {
    const len = Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y)
    segs.push(len)
    total += len
  }
  let target = total * 0.5
  for (let i = 0; i < segs.length; i++) {
    if (target <= segs[i] || i === segs.length - 1) {
      const t = segs[i] === 0 ? 0 : Math.min(1, target / segs[i])
      return {
        x: pts[i].x + (pts[i + 1].x - pts[i].x) * t,
        y: pts[i].y + (pts[i + 1].y - pts[i].y) * t,
      }
    }
    target -= segs[i]
  }
  return pts[0]
}

export async function layoutLabelGraph(g: LabelGraph): Promise<GraphLayout> {
  const graph: ElkNode = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'DOWN',
      'elk.edgeRouting': 'POLYLINE',
      'elk.spacing.nodeNode': '36',
      'elk.layered.spacing.nodeNodeBetweenLayers': '56',
      'elk.layered.cycleBreaking.strategy': 'GREEDY',
      'elk.layered.thoroughness': '5',
    },
    children: g.nodes.map((n) => {
      const { w, h } = graphNodeSize(n)
      return { id: `n${n.id}`, width: w, height: h }
    }),
    edges: g.edges.map((e, i) => ({ id: `e${i}`, sources: [`n${e.from}`], targets: [`n${e.to}`] })),
  }
  const res = await elkLayout(graph)
  const margin = 30
  const nodes: GLNode[] = (res.children ?? []).map((c) => ({
    id: Number(c.id.slice(1)),
    x: (c.x ?? 0) + margin,
    y: (c.y ?? 0) + margin,
    w: c.width ?? 0,
    h: c.height ?? 0,
  }))
  const byId = new Map(nodes.map((n) => [n.id, n]))
  const drafted: { index: number; d: string; text: string; cond: string | null; pts: { x: number; y: number }[]; ax: number; ay: number; want: number }[] = []
  ;(res.edges ?? []).forEach((e, i) => {
    const pts = sectionPoints(e as ElkExtendedEdge, margin, margin)
    if (pts.length < 2) return
    const ge = g.edges[Number(e.id?.slice(1))] ?? g.edges[i]
    const text = edgeText(ge?.label ?? null, ge?.cond ?? null)
    const mid = polylineMidpoint(pts)
    const target = ge ? byId.get(ge.to) : undefined
    const source = ge ? byId.get(ge.from) : undefined
    let ax = mid.x
    let ay = mid.y
    if (target) {
      ax = target.x + target.w / 2
      ay = target.y - 20
      if (source && ay < source.y + source.h + 14) ay = (source.y + source.h + target.y) / 2
    }
    drafted.push({
      index: Number(e.id?.slice(1) ?? i),
      d: pathOf(pts),
      text,
      cond: ge?.cond ?? null,
      pts,
      ax,
      ay,
      want: text ? chipWidthFor(text) : 0,
    })
  })
  // Keep each caption inside the gap above its own branch. The layout stays as
  // wide as the nodes; a caption shrinks only when the next branch's caption
  // would otherwise meet it.
  for (const chip of drafted) {
    if (!chip.want) continue
    let half = chip.want / 2
    for (const other of drafted) {
      if (other === chip || !other.want) continue
      if (Math.abs(other.ay - chip.ay) > 24) continue
      const dist = Math.abs(other.ax - chip.ax)
      if (dist < 8) continue
      half = Math.min(half, dist / 2 - 8)
    }
    chip.want = Math.max(48, half * 2)
  }
  const edges: GLEdge[] = drafted.map((chip) => {
    const xs = chip.pts.map((p) => p.x)
    const ys = chip.pts.map((p) => p.y)
    if (chip.want) {
      xs.push(chip.ax - chip.want / 2, chip.ax + chip.want / 2)
      ys.push(chip.ay - 9, chip.ay + 9)
    }
    return {
      index: chip.index,
      d: chip.d,
      label: chip.text || null,
      cond: chip.cond,
      lw: chip.want,
      lx: chip.ax,
      ly: chip.ay,
      x0: Math.min(...xs),
      y0: Math.min(...ys),
      x1: Math.max(...xs),
      y1: Math.max(...ys),
      segs: segmentBoxes(chip.pts),
    }
  })
  let width = (res.width ?? 0) + margin * 2
  let height = (res.height ?? 0) + margin * 2
  for (const e of edges) {
    if (!e.lw) continue
    width = Math.max(width, e.lx + e.lw / 2 + 12)
    height = Math.max(height, e.ly + 12)
  }
  return { nodes, edges, width, height }
}

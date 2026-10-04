import ELK from 'elkjs/lib/elk-api'
import type { ElkNode, ElkExtendedEdge } from 'elkjs/lib/elk-api'
import elkWorkerUrl from 'elkjs/lib/elk-worker.min.js?url'
import { api } from './api'
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

/** Samples of a cubic so a long curve is indexed in pieces. The drawn `d` stays one command. */
function cubicPoints(
  x0: number,
  y0: number,
  c1x: number,
  c1y: number,
  c2x: number,
  c2y: number,
  x1: number,
  y1: number,
): { x: number; y: number }[] {
  const steps = Math.max(4, Math.ceil(Math.hypot(x1 - x0, y1 - y0) / 2048))
  const pts: { x: number; y: number }[] = []
  for (let i = 0; i <= steps; i++) {
    const t = i / steps
    const u = 1 - t
    const uu = u * u
    const tt = t * t
    pts.push({
      x: uu * u * x0 + 3 * uu * t * c1x + 3 * u * tt * c2x + tt * t * x1,
      y: uu * u * y0 + 3 * uu * t * c1y + 3 * u * tt * c2y + tt * t * y1,
    })
  }
  return pts
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

export type ClusterMode = 'prefix' | 'file'

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

const NODE_H = 38
const CLUSTER_PAD = 18
const CLUSTER_HEADER = 34

export function mapNodeWidth(n: { id: string }): number {
  return Math.max(96, Math.min(240, n.id.length * 7.4 + 32))
}

/** Assign every node to a cluster. `prefix` mode groups `v18...` style names and
 *  lets unprefixed labels join the cluster they are most connected to. */
export function clusterNodes(
  map: ProjectMap,
  files: FileInfo[],
  mode: ClusterMode,
): Map<string, string> {
  const out = new Map<string, string>()
  if (mode === 'file') {
    for (const n of map.nodes) {
      out.set(n.id, n.kind === 'missing' ? '(missing)' : (files[n.file]?.path ?? '(unknown)'))
    }
    return out
  }
  const initial = new Map<string, string>()
  const sizes = new Map<string, number>()
  for (const n of map.nodes) {
    if (n.kind === 'missing') continue
    const m = /^([a-z]+\d+)/i.exec(n.id) ?? /^([a-z]+)_/i.exec(n.id)
    if (m) {
      const key = m[1].toLowerCase()
      initial.set(n.id, key)
      sizes.set(key, (sizes.get(key) ?? 0) + 1)
    }
  }
  for (const [id, key] of [...initial]) {
    if ((sizes.get(key) ?? 0) < 3) initial.delete(id)
  }
  const assigned = new Map(initial)
  const neighbours = new Map<string, string[]>()
  const link = (a: string, b: string) => {
    if (!neighbours.has(a)) neighbours.set(a, [])
    neighbours.get(a)!.push(b)
  }
  for (const e of map.edges) {
    link(e.from, e.to)
    link(e.to, e.from)
  }
  for (let round = 0; round < 8; round++) {
    const next: [string, string][] = []
    for (const n of map.nodes) {
      if (n.kind === 'missing' || assigned.has(n.id)) continue
      const votes = new Map<string, number>()
      for (const nb of neighbours.get(n.id) ?? []) {
        const k = assigned.get(nb)
        if (k) votes.set(k, (votes.get(k) ?? 0) + 1)
      }
      let best = ''
      let bestVotes = 0
      for (const [k, v] of votes) {
        if (v > bestVotes) {
          best = k
          bestVotes = v
        }
      }
      if (best) next.push([n.id, best])
    }
    if (!next.length) break
    for (const [id, k] of next) assigned.set(id, k)
  }
  for (const n of map.nodes) {
    if (n.kind === 'missing') out.set(n.id, '(missing)')
    else out.set(n.id, assigned.get(n.id) ?? 'misc')
  }
  return out
}

export function mapCacheKey(root: string, map: ProjectMap, mode: ClusterMode): string {
  let h = 2166136261
  for (const n of map.nodes) h = Math.imul(h ^ hashString(n.id), 16777619)
  for (const e of map.edges) h = Math.imul(h ^ hashString(`${e.from}>${e.to}:${e.kind}`), 16777619)
  return `map|v3|${root}|${mode}|${map.nodes.length}|${map.edges.length}|${h >>> 0}`
}

const INNER_OPTIONS = {
  'elk.algorithm': 'layered',
  'elk.direction': 'DOWN',
  'elk.edgeRouting': 'POLYLINE',
  'elk.spacing.nodeNode': '22',
  'elk.layered.spacing.nodeNodeBetweenLayers': '38',
  'elk.layered.thoroughness': '3',
  'elk.layered.cycleBreaking.strategy': 'GREEDY',
  'elk.layered.nodePlacement.strategy': 'SIMPLE',
}

const OUTER_OPTIONS = {
  'elk.algorithm': 'layered',
  'elk.direction': 'RIGHT',
  'elk.edgeRouting': 'POLYLINE',
  'elk.spacing.nodeNode': '60',
  'elk.layered.spacing.nodeNodeBetweenLayers': '140',
  'elk.layered.cycleBreaking.strategy': 'GREEDY',
}

export interface MapLayoutProgress {
  (done: number, total: number): void
}

export async function layoutProjectMap(
  map: ProjectMap,
  files: FileInfo[],
  mode: ClusterMode,
  key: string,
  progress?: MapLayoutProgress,
): Promise<MapLayout> {
  try {
    const cached = await api.layoutCacheGet(key)
    if (cached) return JSON.parse(cached) as MapLayout
  } catch {
    /* cache is best effort */
  }

  const clusterOf = clusterNodes(map, files, mode)
  const keys = [...new Set(clusterOf.values())].sort()
  const clusterIndex = new Map(keys.map((k, i) => [k, i]))
  const byCluster: MapNode[][] = keys.map(() => [])
  for (const n of map.nodes) byCluster[clusterIndex.get(clusterOf.get(n.id)!)!].push(n)

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
    const intraEdges = map.edges.filter((e) => ids.has(e.from) && ids.has(e.to) && e.from !== e.to)
    const graph: ElkNode = {
      id: `c${ci}`,
      layoutOptions: INNER_OPTIONS,
      children: members.map((m) => ({ id: m.id, width: mapNodeWidth(m), height: NODE_H })),
      edges: intraEdges.map((e, i) => ({ id: `e${i}`, sources: [e.from], targets: [e.to] })),
    }
    const res = await elkLayout(graph)
    const pos = new Map<string, { x: number; y: number; w: number; h: number }>()
    for (const c of res.children ?? []) {
      pos.set(c.id, { x: c.x ?? 0, y: c.y ?? 0, w: c.width ?? 0, h: c.height ?? 0 })
    }
    const intra = intraEdges.map((e, i) => ({
      edge: e,
      points: sectionPoints((res.edges?.[i] as ElkExtendedEdge) ?? ({} as ElkExtendedEdge), 0, 0),
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
  const weights = new Map<string, number>()
  for (const e of map.edges) {
    const a = clusterIndex.get(clusterOf.get(e.from)!)!
    const b = clusterIndex.get(clusterOf.get(e.to)!)!
    if (a === b) continue
    const k = `${a}>${b}`
    weights.set(k, (weights.get(k) ?? 0) + e.count)
  }
  const outerGraph: ElkNode = {
    id: 'root',
    layoutOptions: OUTER_OPTIONS,
    children: inners.map((c, i) => ({ id: `c${i}`, width: c.w, height: c.h })),
    edges: [...weights.keys()].map((k, i) => {
      const [a, b] = k.split('>')
      return { id: `o${i}`, sources: [`c${a}`], targets: [`c${b}`] }
    }),
  }
  const outer = await elkLayout(outerGraph)
  progress?.(keys.length + 1, keys.length + 1)

  // 3. compose absolute coordinates
  const layoutNodes: LNode[] = []
  const layoutClusters: LCluster[] = []
  const nodePos = new Map<string, LNode>()
  const origin: { x: number; y: number }[] = []
  const margin = 40
  inners.forEach((inner, i) => {
    const oc = outer.children?.find((c) => c.id === `c${i}`)
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

  const edges: LEdge[] = []
  inners.forEach((inner, i) => {
    const dx = origin[i].x + CLUSTER_PAD
    const dy = origin[i].y + CLUSTER_HEADER + CLUSTER_PAD
    for (const { edge, points } of inner.intra) {
      const pts = points.map((p) => ({ x: p.x + dx, y: p.y + dy }))
      if (pts.length < 2) continue
      edges.push({
        from: edge.from,
        to: edge.to,
        kind: edge.kind,
        count: edge.count,
        d: pathOf(pts),
        x0: Math.min(...pts.map((p) => p.x)),
        y0: Math.min(...pts.map((p) => p.y)),
        x1: Math.max(...pts.map((p) => p.x)),
        y1: Math.max(...pts.map((p) => p.y)),
        segs: segmentBoxes(pts),
        intra: true,
      })
    }
  })
  for (const e of map.edges) {
    const a = nodePos.get(e.from)
    const b = nodePos.get(e.to)
    if (!a || !b || a.c === b.c) continue
    const forward = b.x + b.w / 2 >= a.x + a.w / 2
    const x0 = forward ? a.x + a.w : a.x
    const y0 = a.y + a.h / 2
    const x1 = forward ? b.x : b.x + b.w
    const y1 = b.y + b.h / 2
    const dx = Math.min(400, Math.max(40, Math.abs(x1 - x0) / 2)) * (forward ? 1 : -1)
    const samples = cubicPoints(x0, y0, x0 + dx, y0, x1 - dx, y1, x1, y1)
    edges.push({
      from: e.from,
      to: e.to,
      kind: e.kind,
      count: e.count,
      d: `M${round(x0)} ${round(y0)} C${round(x0 + dx)} ${round(y0)} ${round(x1 - dx)} ${round(y1)} ${round(x1)} ${round(y1)}`,
      x0: Math.min(x0, x1),
      y0: Math.min(y0, y1),
      x1: Math.max(x0, x1),
      y1: Math.max(y0, y1),
      segs: segmentBoxes(samples),
      intra: false,
    })
  }

  const clusterEdges: LClusterEdge[] = []
  for (const [k, weight] of weights) {
    const [a, b] = k.split('>').map(Number)
    const ca = layoutClusters[a]
    const cb = layoutClusters[b]
    const forward = cb.x + cb.w / 2 >= ca.x + ca.w / 2
    const x0 = forward ? ca.x + ca.w : ca.x
    const y0 = ca.y + ca.h / 2
    const x1 = forward ? cb.x : cb.x + cb.w
    const y1 = cb.y + cb.h / 2
    const dx = Math.min(600, Math.max(60, Math.abs(x1 - x0) / 2)) * (forward ? 1 : -1)
    const samples = cubicPoints(x0, y0, x0 + dx, y0, x1 - dx, y1, x1, y1)
    clusterEdges.push({
      from: a,
      to: b,
      weight,
      d: `M${round(x0)} ${round(y0)} C${round(x0 + dx)} ${round(y0)} ${round(x1 - dx)} ${round(y1)} ${round(x1)} ${round(y1)}`,
      x0: Math.min(x0, x1),
      y0: Math.min(y0, y1),
      x1: Math.max(x0, x1),
      y1: Math.max(y0, y1),
      segs: segmentBoxes(samples),
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

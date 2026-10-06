import type { FileInfo, MapEdge, MapNode } from './types'

/** Group keys that are not scene names. The map titles them with `groupTitle`. */
export const MISSING_KEY = '(missing)'
export const HELPERS_KEY = '(helpers)'
export const UNLINKED_KEY = '(unlinked)'

/** Groups laid out as a grid of tiles instead of a flow. They have no meaningful order. */
export function isTileGroup(key: string): boolean {
  return key === HELPERS_KEY || key === UNLINKED_KEY
}

export function groupTitle(key: string): string {
  switch (key) {
    case HELPERS_KEY:
      return 'Helpers'
    case UNLINKED_KEY:
      return 'Unlinked'
    case MISSING_KEY:
      return 'Missing targets'
    default:
      return key
  }
}

export type GroupMode = 'auto' | 'flow' | 'prefix' | 'file'

type Link = Pick<MapEdge, 'from' | 'to'> & Partial<Pick<MapEdge, 'kind' | 'count'>>

// ---------------------------------------------------------------- helpers

/** A helper is called from several places, like `call play_sound` or a shared scene that jumps away. */
const HELPER_CALL_SHARE = 0.8
const HELPER_MIN_CALLERS = 3
const HELPER_LEAF_MIN_CALLERS = 2
/** More than this share of the scenes being helpers means the rule is misreading the project. */
const HELPER_MAX_SHARE = 0.6

/**
 * Labels that act as shared subroutines. They are reached by `call`, and either
 * return to the caller or are called from enough places that a jump-exit would
 * still tie the story in knots. A leaf that returns can qualify with two callers.
 * A project that is mostly helpers by this rule keeps none.
 */
export function findHelpers(nodes: readonly MapNode[], edges: readonly MapEdge[]): Set<string> {
  const known = new Map(nodes.map((n) => [n.id, n]))
  const incoming = new Map<string, MapEdge[]>()
  const outgoing = new Map<string, number>()
  for (const e of edges) {
    if (e.from === e.to || !known.has(e.from) || !known.has(e.to)) continue
    const list = incoming.get(e.to)
    if (list) list.push(e)
    else incoming.set(e.to, [e])
    outgoing.set(e.from, (outgoing.get(e.from) ?? 0) + 1)
  }
  const out = new Set<string>()
  let eligible = 0
  for (const n of nodes) {
    if (n.kind !== 'label') continue
    eligible++
    if (n.root || n.endsScript) continue
    const ins = incoming.get(n.id)
    if (!ins || !ins.length) continue
    const viaCall = ins.filter((e) => e.kind === 'call').length
    if (viaCall / ins.length < HELPER_CALL_SHARE) continue
    const callers = new Set(ins.map((e) => e.from)).size
    const leaf = !outgoing.get(n.id)
    const shared = callers >= HELPER_MIN_CALLERS
    const smallLeaf = n.returns && leaf && callers >= HELPER_LEAF_MIN_CALLERS
    if (shared || smallLeaf) out.add(n.id)
  }
  if (eligible === 0 || out.size > eligible * HELPER_MAX_SHARE) return new Set()
  return out
}

// ---------------------------------------------------------------- name prefixes

export interface PrefixResult {
  /** Every scene that ended up in a prefix group. The rest are not in the map. */
  assigned: Map<string, string>
  /** Scenes whose own name carried a prefix shared by enough others. */
  seeded: number
}

/**
 * Groups `v18...` and `intro_...` style names. Scenes without such a prefix join
 * the group they are linked to most, over a few rounds.
 */
export function prefixGroups(nodes: readonly MapNode[], edges: readonly Link[]): PrefixResult {
  const initial = new Map<string, string>()
  const sizes = new Map<string, number>()
  for (const n of nodes) {
    const m = /^([a-z]+\d+)/i.exec(n.id) ?? /^([a-z]+)_/i.exec(n.id)
    if (!m) continue
    const key = m[1].toLowerCase()
    initial.set(n.id, key)
    sizes.set(key, (sizes.get(key) ?? 0) + 1)
  }
  for (const [id, key] of [...initial]) {
    if ((sizes.get(key) ?? 0) < 3) initial.delete(id)
  }
  const seeded = initial.size
  const assigned = new Map(initial)
  const neighbours = new Map<string, string[]>()
  const link = (a: string, b: string) => {
    const list = neighbours.get(a)
    if (list) list.push(b)
    else neighbours.set(a, [b])
  }
  for (const e of edges) {
    link(e.from, e.to)
    link(e.to, e.from)
  }
  for (let round = 0; round < 8; round++) {
    const next: [string, string][] = []
    for (const n of nodes) {
      if (assigned.has(n.id)) continue
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
  return { assigned, seeded }
}

// ---------------------------------------------------------------- story flow

const MIN_GROUP = 3
const MAX_PASSES = 50
const MAX_LEVELS = 12
const EPS = 1e-9

/**
 * Louvain community detection on an undirected weighted graph. Nodes move to the
 * neighbouring group that raises modularity the most, then groups are merged
 * into single nodes and the process repeats. Visit order is fixed, so the result
 * is the same on every run. Returns a group number for every node.
 */
export function louvain(n: number, links: readonly (readonly [number, number, number])[]): number[] {
  let adj: Map<number, number>[] = Array.from({ length: n }, () => new Map())
  for (const [a, b, w] of links) {
    if (a === b || w <= 0) continue
    adj[a].set(b, (adj[a].get(b) ?? 0) + w)
    adj[b].set(a, (adj[b].get(a) ?? 0) + w)
  }
  // `inner` is the weight of links inside a merged node. `deg` counts it twice.
  let inner = new Array<number>(n).fill(0)
  let deg = adj.map((m) => {
    let s = 0
    for (const w of m.values()) s += w
    return s
  })
  let of = Array.from({ length: n }, (_, i) => i)

  for (let level = 0; level < MAX_LEVELS; level++) {
    const size = adj.length
    let m2 = 0
    for (const d of deg) m2 += d
    if (m2 === 0) break
    const comm = Array.from({ length: size }, (_, i) => i)
    const tot = deg.slice()
    let moved = false
    for (let pass = 0; pass < MAX_PASSES; pass++) {
      let passMoved = false
      for (let i = 0; i < size; i++) {
        const home = comm[i]
        const toComm = new Map<number, number>()
        for (const [j, w] of adj[i]) toComm.set(comm[j], (toComm.get(comm[j]) ?? 0) + w)
        tot[home] -= deg[i]
        let best = home
        let bestGain = (toComm.get(home) ?? 0) - (tot[home] * deg[i]) / m2
        for (const [c, w] of toComm) {
          const gain = w - (tot[c] * deg[i]) / m2
          if (gain > bestGain + EPS) {
            best = c
            bestGain = gain
          }
        }
        tot[best] += deg[i]
        if (best !== home) {
          comm[i] = best
          passMoved = true
          moved = true
        }
      }
      if (!passMoved) break
    }
    if (!moved) break

    const renumber = new Map<number, number>()
    for (const c of comm) if (!renumber.has(c)) renumber.set(c, renumber.size)
    const next: Map<number, number>[] = Array.from({ length: renumber.size }, () => new Map())
    const nextInner = new Array<number>(renumber.size).fill(0)
    const nextDeg = new Array<number>(renumber.size).fill(0)
    for (let i = 0; i < size; i++) {
      const a = renumber.get(comm[i])!
      nextInner[a] += inner[i]
      nextDeg[a] += deg[i]
      for (const [j, w] of adj[i]) {
        const b = renumber.get(comm[j])!
        if (a === b) nextInner[a] += w / 2
        else next[a].set(b, (next[a].get(b) ?? 0) + w)
      }
    }
    of = of.map((g) => renumber.get(comm[g])!)
    adj = next
    inner = nextInner
    deg = nextDeg
    if (renumber.size === size) break
  }
  return of
}

/**
 * Groups scenes by how they link. Groups under three scenes fold into the
 * neighbour they link to most; scenes with no links go to the unlinked group. A
 * group is named after its most linked scene.
 */
export function flowGroups(ids: readonly string[], edges: readonly Link[]): Map<string, string> {
  const order = new Map(ids.map((id, i) => [id, i]))
  const adj = new Map<string, Map<string, number>>(ids.map((id) => [id, new Map()]))
  for (const e of edges) {
    if (e.from === e.to || !order.has(e.from) || !order.has(e.to)) continue
    const a = adj.get(e.from)!
    const b = adj.get(e.to)!
    a.set(e.to, (a.get(e.to) ?? 0) + 1)
    b.set(e.from, (b.get(e.from) ?? 0) + 1)
  }

  const links: [number, number, number][] = []
  for (const [id, m] of adj) {
    const a = order.get(id)!
    for (const [other, w] of m) {
      const b = order.get(other)!
      if (a < b) links.push([a, b, w])
    }
  }
  const found = louvain(ids.length, links)
  // Each group is keyed by its earliest scene, so the order of scenes decides ties later.
  const first = new Map<number, string>()
  ids.forEach((id, i) => {
    if (!first.has(found[i])) first.set(found[i], id)
  })
  const comm = new Map(ids.map((id, i) => [id, first.get(found[i])!]))

  const members = new Map<string, string[]>()
  for (const id of ids) {
    const c = comm.get(id)!
    const list = members.get(c)
    if (list) list.push(id)
    else members.set(c, [id])
  }

  const result = new Map<string, string>()
  const small: string[][] = []
  const big = new Map<string, string[]>()
  for (const [c, list] of members) {
    if (list.length >= MIN_GROUP) big.set(c, list)
    else small.push(list)
  }
  const home = new Map<string, string>()
  for (const [c, list] of big) for (const id of list) home.set(id, c)
  for (const list of small) {
    const pull = new Map<string, number>()
    for (const id of list) {
      for (const [nb, w] of adj.get(id)!) {
        const c = home.get(nb)
        if (c) pull.set(c, (pull.get(c) ?? 0) + w)
      }
    }
    let target = ''
    let weight = 0
    for (const [c, w] of pull) {
      if (w > weight || (w === weight && target && order.get(c)! < order.get(target)!)) {
        target = c
        weight = w
      }
    }
    if (target) {
      for (const id of list) big.get(target)!.push(id)
    } else {
      for (const id of list) result.set(id, UNLINKED_KEY)
    }
  }
  for (const list of big.values()) {
    let hub = list[0]
    let hubW = -1
    for (const id of list) {
      let w = 0
      for (const x of adj.get(id)!.values()) w += x
      if (w > hubW || (w === hubW && order.get(id)! < order.get(hub)!)) {
        hub = id
        hubW = w
      }
    }
    for (const id of list) result.set(id, hub)
  }
  return result
}

// ---------------------------------------------------------------- assignment

/** Share of scenes whose names must carry a prefix before prefix grouping beats story flow. */
const PREFIX_COVERAGE = 0.5

/** Which of `prefix` or `flow` `auto` resolves to. */
export function resolveMode(
  mode: GroupMode,
  nodes: readonly MapNode[],
  edges: readonly Link[],
): Exclude<GroupMode, 'auto'> {
  if (mode !== 'auto') return mode
  if (!nodes.length) return 'flow'
  const { seeded } = prefixGroups(nodes, edges)
  return seeded / nodes.length >= PREFIX_COVERAGE ? 'prefix' : 'flow'
}

/**
 * Group key for every node. Missing targets and helpers get their own groups
 * whatever the mode. The rest follow the mode.
 */
export function assignGroups(
  nodes: readonly MapNode[],
  edges: readonly MapEdge[],
  files: readonly FileInfo[],
  mode: GroupMode,
  helpers: ReadonlySet<string>,
): Map<string, string> {
  const out = new Map<string, string>()
  const rest: MapNode[] = []
  for (const n of nodes) {
    if (n.kind === 'missing') out.set(n.id, MISSING_KEY)
    else if (helpers.has(n.id)) out.set(n.id, HELPERS_KEY)
    else rest.push(n)
  }
  const inRest = new Set(rest.map((n) => n.id))
  const story = edges.filter((e) => e.from !== e.to && inRest.has(e.from) && inRest.has(e.to))
  const resolved = resolveMode(mode, rest, story)
  if (resolved === 'file') {
    for (const n of rest) out.set(n.id, files[n.file]?.path ?? '(unknown)')
  } else if (resolved === 'prefix') {
    const { assigned } = prefixGroups(rest, story)
    for (const n of rest) out.set(n.id, assigned.get(n.id) ?? 'misc')
  } else {
    const flow = flowGroups(
      rest.map((n) => n.id),
      story,
    )
    for (const n of rest) out.set(n.id, flow.get(n.id) ?? UNLINKED_KEY)
  }
  return out
}

import { describe, expect, it, vi } from 'vitest'

// The app runs ELK in a Web Worker. Tests run the same engine in-process.
vi.mock('elkjs/lib/elk-api', async () => {
  const bundled = await import('elkjs/lib/elk.bundled.js')
  return { default: bundled.default }
})
vi.mock('elkjs/lib/elk-worker.min.js?url', () => ({ default: '' }))

import { layoutProjectMap, mapChipWidth, type MapLayout } from './layout'
import { HELPERS_KEY, type GroupMode } from './mapgroups'
import type { FileInfo, MapEdge, MapEdgeKind, MapNode, MapNodeKind, ProjectMap } from './types'

function node(id: string, extra: Partial<MapNode> = {}): MapNode {
  return {
    id,
    kind: 'label',
    file: 0,
    line: 1,
    endLine: 1,
    stmts: 1,
    says: 0,
    menus: 0,
    choices: 0,
    inDegree: 0,
    outDegree: 0,
    reachable: true,
    root: false,
    indirect: false,
    duplicate: false,
    returns: false,
    endsScript: false,
    dynamicOut: 0,
    ...extra,
  }
}

function edge(from: string, to: string, kind: MapEdgeKind = 'jump', count = 1): MapEdge {
  return { from, to, kind, count }
}

const file: FileInfo = {
  path: 'script.rpy',
  lines: 10,
  bytes: 100,
  opaque: 0,
  labels: 1,
  issues: 0,
  origin: 'loose',
  archive: null,
  editable: true,
  decompiled: false,
  reasons: [],
}

let runs = 0
async function lay(map: ProjectMap, mode: GroupMode = 'prefix', helpers = true): Promise<MapLayout> {
  // The key is unique so a cached layout can never stand in for a real run.
  return layoutProjectMap(map, [file], mode, `test-${runs++}`, undefined, { helpers })
}

function expectSound(layout: MapLayout, map: ProjectMap) {
  expect(layout.nodes.length).toBe(map.nodes.length)
  expect(Number.isFinite(layout.width) && Number.isFinite(layout.height)).toBe(true)
  for (const n of layout.nodes) {
    expect(Number.isFinite(n.x) && Number.isFinite(n.y)).toBe(true)
    expect(n.w).toBeGreaterThan(0)
  }
  for (const e of [...layout.edges, ...layout.clusterEdges]) {
    expect(e.d.startsWith('M')).toBe(true)
    expect(e.d).not.toMatch(/NaN|Infinity/)
  }
}

/** Small deterministic generator so a failure can be replayed. */
function rng(seed: number): () => number {
  let s = seed >>> 0
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0
    return s / 2 ** 32
  }
}

describe('layoutProjectMap', () => {
  it('puts the start group on the left and bundles edges between groups', async () => {
    const map: ProjectMap = {
      nodes: [
        node('start', { root: true }),
        node('v18a'),
        node('v18b'),
        node('v18c'),
        node('v19a'),
        node('v19b'),
        node('v19c', { endsScript: true, returns: true }),
        node('gone', { kind: 'missing' }),
      ],
      edges: [
        edge('start', 'v18a'),
        edge('v18a', 'v18b'),
        edge('v18b', 'v18c'),
        edge('v18c', 'v19a', 'choice', 2),
        edge('v19a', 'v19b', 'fall'),
        edge('v19b', 'v19c'),
        edge('v19c', 'gone'),
      ],
    }
    const layout = await lay(map)
    expectSound(layout, map)
    const start = layout.nodes.find((n) => n.id === 'start')!
    const startCluster = layout.clusters[start.c]
    expect(startCluster.x).toBe(Math.min(...layout.clusters.map((c) => c.x)))
    const missing = layout.clusters.find((c) => c.key === '(missing)')!
    expect(missing.x).toBeGreaterThan(startCluster.x)

    const cross = layout.edges.find((e) => e.from === 'v18c' && e.to === 'v19a')!
    expect(cross.intra).toBe(false)
    expect(cross.fan?.startsWith('M')).toBe(true)
    const inside = layout.edges.find((e) => e.from === 'v18a' && e.to === 'v18b')!
    expect(inside.intra).toBe(true)
    expect(inside.fan).toBeUndefined()
  })

  it('survives links back into the start scene and into screens', async () => {
    // A pinned first-layer node with an incoming edge made ELK throw.
    const map: ProjectMap = {
      nodes: [
        node('start', { root: true }),
        node('screen:game_menu', { kind: 'screen', root: true }),
        node('screen:preferences', { kind: 'screen' }),
        node('menu_main', { kind: 'menu' }),
        node('ch1'),
        node('ch2'),
      ],
      edges: [
        edge('start', 'ch1'),
        edge('ch1', 'ch2'),
        edge('ch2', 'start'),
        edge('start', 'screen:game_menu', 'screen'),
        edge('screen:game_menu', 'screen:preferences', 'screen'),
        edge('screen:preferences', 'screen:game_menu', 'action'),
        edge('screen:preferences', 'start', 'action'),
        edge('menu_main', 'start', 'choice'),
        edge('ch2', 'menu_main', 'call'),
      ],
    }
    expectSound(await lay(map), map)
    expectSound(await lay(map, 'file'), map)
  })

  it('survives groups that link to each other in a loop, and a root with nothing in it', async () => {
    const map: ProjectMap = {
      nodes: [
        node('a1', { root: true }),
        node('a2'),
        node('a3'),
        node('b1'),
        node('b2'),
        node('b3'),
        node('gone', { kind: 'missing' }),
        node('gone2', { kind: 'missing' }),
      ],
      edges: [
        edge('a1', 'a2'),
        edge('a2', 'a3'),
        edge('a3', 'b1'),
        edge('b1', 'b2'),
        edge('b2', 'b3'),
        edge('b3', 'a1'),
        edge('b3', 'gone'),
        edge('gone', 'gone2'),
      ],
    }
    expectSound(await lay(map), map)
  })

  it('handles an empty map and a lone node', async () => {
    expectSound(await lay({ nodes: [], edges: [] }), { nodes: [], edges: [] })
    const one: ProjectMap = { nodes: [node('only', { root: true })], edges: [edge('only', 'only')] }
    expectSound(await lay(one), one)
  })

  it('lays out random graphs of every shape without throwing', async () => {
    const kinds: MapNodeKind[] = ['label', 'label', 'label', 'menu', 'screen', 'compiled', 'missing']
    const edgeKinds: MapEdgeKind[] = ['jump', 'choice', 'fall', 'call', 'screen', 'action']
    const prefixes = ['v1', 'v2', 'ch', 'end', 'x', '']
    for (let seed = 1; seed <= 40; seed++) {
      const rand = rng(seed)
      const n = 2 + Math.floor(rand() * 60)
      const nodes: MapNode[] = []
      for (let i = 0; i < n; i++) {
        const p = prefixes[Math.floor(rand() * prefixes.length)]
        nodes.push(
          node(`${p}${p && /\d$/.test(p) ? '' : '_'}n${i}`, {
            kind: kinds[Math.floor(rand() * kinds.length)],
            root: rand() < 0.15,
            endsScript: rand() < 0.1,
            returns: rand() < 0.1,
          }),
        )
      }
      const edges: MapEdge[] = []
      const m = Math.floor(rand() * n * 3)
      for (let i = 0; i < m; i++) {
        const a = nodes[Math.floor(rand() * n)]
        const b = nodes[Math.floor(rand() * n)]
        edges.push(edge(a.id, b.id, edgeKinds[Math.floor(rand() * edgeKinds.length)], 1 + Math.floor(rand() * 4)))
      }
      const map: ProjectMap = { nodes, edges }
      try {
        for (const mode of ['auto', 'flow', 'prefix', 'file'] as GroupMode[]) {
          expectSound(await lay(map, mode), map)
        }
        expectSound(await lay(map, 'auto', false), map)
      } catch (err) {
        throw new Error(`seed ${seed} failed: ${err instanceof Error ? err.message : String(err)}`)
      }
    }
  }, 120_000)

  /** A story chain where every scene calls a few shared subroutines. */
  function storyWithHelpers(scenes = 12): ProjectMap {
    const nodes: MapNode[] = [node('start', { root: true })]
    const edges: MapEdge[] = [edge('start', 's1')]
    const helpers = ['paus', 'snd_a', 'snd_b']
    for (const h of helpers) nodes.push(node(h, { returns: true }))
    for (let i = 1; i <= scenes; i++) {
      nodes.push(node(`s${i}`))
      if (i < scenes) edges.push(edge(`s${i}`, `s${i + 1}`))
      for (const h of helpers) edges.push(edge(`s${i}`, h, 'call'))
    }
    return { nodes, edges }
  }

  it('moves shared subroutines into a lane at the far side of the story', async () => {
    const map = storyWithHelpers()
    const layout = await lay(map, 'flow')
    expectSound(layout, map)
    const lane = layout.clusters.findIndex((c) => c.key === HELPERS_KEY)
    expect(lane).toBeGreaterThanOrEqual(0)
    expect(layout.clusters[lane].count).toBe(3)
    for (const [i, c] of layout.clusters.entries()) {
      if (i !== lane) expect(layout.clusters[lane].x).toBeGreaterThan(c.x)
    }
    // Links to helpers are kept apart from the bundled story flow.
    const links = layout.edges.filter((e) => e.helper)
    expect(links.length).toBe(36)
    for (const e of links) {
      expect(e.intra).toBe(false)
      expect(e.fan).toBeUndefined()
    }
    expect(layout.edges.filter((e) => !e.helper && e.to === 'paus')).toHaveLength(0)
    expect(layout.clusterEdges.some((c) => c.from === lane || c.to === lane)).toBe(false)
  })

  it('keeps helpers in the story when asked to', async () => {
    const map = storyWithHelpers()
    const layout = await lay(map, 'flow', false)
    expectSound(layout, map)
    expect(layout.clusters.some((c) => c.key === HELPERS_KEY)).toBe(false)
    expect(layout.edges.some((e) => e.helper)).toBe(false)
  })

  it('tiles the helper lane instead of running it through the layered engine', async () => {
    const nodes: MapNode[] = [node('start', { root: true })]
    const edges: MapEdge[] = [edge('start', 's1')]
    for (let h = 0; h < 40; h++) nodes.push(node(`snd${h}`, { returns: true }))
    // Enough scenes that the 40 helpers are clearly the minority.
    for (let i = 1; i <= 60; i++) {
      nodes.push(node(`s${i}`))
      if (i < 60) edges.push(edge(`s${i}`, `s${i + 1}`))
      for (let h = 0; h < 40; h += 1 + (i % 3)) edges.push(edge(`s${i}`, `snd${h}`, 'call'))
    }
    const map: ProjectMap = { nodes, edges }
    const layout = await lay(map, 'flow')
    expectSound(layout, map)
    const lane = layout.clusters.find((c) => c.key === HELPERS_KEY)!
    expect(lane.count).toBe(40)
    // 40 tiles should form a block, not a single column.
    expect(lane.w).toBeGreaterThan(lane.h / 4)
    const tiles = layout.nodes.filter((n) => n.id.startsWith('snd'))
    for (let i = 0; i < tiles.length; i++) {
      for (let j = i + 1; j < tiles.length; j++) {
        const a = tiles[i]
        const b = tiles[j]
        const gapX = Math.max(a.x - (b.x + b.w), b.x - (a.x + a.w))
        const gapY = Math.max(a.y - (b.y + b.h), b.y - (a.y + a.h))
        expect(Math.max(gapX, gapY)).toBeGreaterThanOrEqual(10)
      }
    }
  })

  it('lays out call-heavy random graphs with helpers without throwing', async () => {
    for (let seed = 100; seed < 130; seed++) {
      const rand = rng(seed)
      const n = 10 + Math.floor(rand() * 50)
      const nodes: MapNode[] = []
      for (let i = 0; i < n; i++) {
        nodes.push(
          node(`n${i}`, {
            kind: rand() < 0.08 ? 'missing' : 'label',
            root: i === 0 || rand() < 0.05,
            returns: rand() < 0.4,
            endsScript: rand() < 0.05,
          }),
        )
      }
      const edges: MapEdge[] = []
      for (let i = 0; i < n * 3; i++) {
        const a = nodes[Math.floor(rand() * n)]
        const b = nodes[Math.floor(rand() * n)]
        edges.push(edge(a.id, b.id, rand() < 0.6 ? 'call' : 'jump'))
      }
      const map: ProjectMap = { nodes, edges }
      try {
        for (const mode of ['auto', 'flow', 'file'] as GroupMode[]) expectSound(await lay(map, mode), map)
      } catch (err) {
        throw new Error(`seed ${seed} failed: ${err instanceof Error ? err.message : String(err)}`)
      }
    }
  }, 120_000)

  it('stacks condition chips that share a trunk between groups', async () => {
    const secret = 'Secret ending · karma > 5'
    const late = 'Open late · energy > 1'
    const files: FileInfo[] = [
      file,
      { ...file, path: 'other.rpy' },
    ]
    const map: ProjectMap = {
      nodes: [
        node('town', { kind: 'screen', root: true }),
        node('morning'),
        node('secret', { file: 1 }),
        node('night', { file: 1 }),
      ],
      edges: [
        edge('town', 'morning'),
        { ...edge('town', 'secret', 'jump'), badge: secret },
        { ...edge('town', 'night', 'jump'), badge: late },
      ],
    }
    const layout = await layoutProjectMap(map, files, 'file', `test-${runs++}`)
    expectSound(layout, map)
    const chips = layout.edges.filter((e) => e.badge)
    expect(chips.map((e) => e.badge).sort()).toEqual([late, secret])
    const [a, b] = chips
    const overlapX = Math.abs(a.lx! - b.lx!) < (mapChipWidth(a.badge!) + mapChipWidth(b.badge!)) / 2 - 1
    const overlapY = Math.abs(a.ly! - b.ly!) < 18 - 1
    expect(overlapX && overlapY).toBe(false)
    expect(Math.abs(a.ly! - b.ly!)).toBeGreaterThanOrEqual(26)
  })

  it('keeps room between neighbouring scenes', async () => {
    const map: ProjectMap = {
      nodes: [node('s', { root: true }), node('s_a'), node('s_b'), node('s_c')],
      edges: [edge('s', 's_a'), edge('s', 's_b'), edge('s', 's_c')],
    }
    const layout = await lay(map)
    const nodes = layout.nodes
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const a = nodes[i]
        const b = nodes[j]
        const gapX = Math.max(a.x - (b.x + b.w), b.x - (a.x + a.w))
        const gapY = Math.max(a.y - (b.y + b.h), b.y - (a.y + a.h))
        expect(Math.max(gapX, gapY)).toBeGreaterThanOrEqual(30)
      }
    }
  })
})

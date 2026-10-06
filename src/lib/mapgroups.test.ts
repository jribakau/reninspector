import { describe, expect, it } from 'vitest'
import {
  HELPERS_KEY,
  MISSING_KEY,
  UNLINKED_KEY,
  assignGroups,
  findHelpers,
  flowGroups,
  groupTitle,
  isTileGroup,
  resolveMode,
} from './mapgroups'
import type { FileInfo, MapEdge, MapEdgeKind, MapNode } from './types'

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

function edge(from: string, to: string, kind: MapEdgeKind = 'jump'): MapEdge {
  return { from, to, kind, count: 1 }
}

const files: FileInfo[] = [
  {
    path: 'script.rpy',
    lines: 1,
    bytes: 1,
    opaque: 0,
    labels: 1,
    issues: 0,
    origin: 'loose',
    archive: null,
    editable: true,
    decompiled: false,
    reasons: [],
  },
]

/** A story of `n` scenes in a chain, each also calling one shared sound helper. */
function withHelper(n: number, helperExtra: Partial<MapNode> = { returns: true }) {
  const nodes: MapNode[] = [node('start', { root: true })]
  const edges: MapEdge[] = []
  for (let i = 0; i < n; i++) nodes.push(node(`scene${i}`))
  nodes.push(node('beep', helperExtra))
  nodes.push(node('scene_end'))
  edges.push(edge('start', 'scene0'))
  for (let i = 0; i < n; i++) {
    edges.push(edge(`scene${i}`, i + 1 < n ? `scene${i + 1}` : 'scene_end'))
    edges.push(edge(`scene${i}`, 'beep', 'call'))
  }
  return { nodes, edges }
}

describe('findHelpers', () => {
  it('finds a subroutine called from several scenes that returns', () => {
    const { nodes, edges } = withHelper(6)
    expect([...findHelpers(nodes, edges)]).toEqual(['beep'])
  })

  it('keeps a shared routine that leaves by jump when many scenes call it', () => {
    const { nodes, edges } = withHelper(6, { returns: false })
    edges.push(edge('beep', 'scene_end'))
    expect([...findHelpers(nodes, edges)]).toEqual(['beep'])
  })

  it('ignores a jump-exit routine with only two callers', () => {
    const nodes = [node('start', { root: true }), node('a'), node('b'), node('fx'), node('end')]
    const edges = [
      edge('start', 'a'),
      edge('a', 'b'),
      edge('a', 'fx', 'call'),
      edge('b', 'fx', 'call'),
      edge('fx', 'end'),
    ]
    expect(findHelpers(nodes, edges).size).toBe(0)
  })

  it('ignores a label that is mostly jumped to', () => {
    const { nodes, edges } = withHelper(6)
    for (const e of edges) if (e.to === 'beep') e.kind = 'jump'
    expect(findHelpers(nodes, edges).size).toBe(0)
  })

  it('needs more than one caller for a single-purpose leaf', () => {
    const nodes = [node('start', { root: true }), node('a'), node('fx', { returns: true })]
    expect(findHelpers(nodes, [edge('start', 'a'), edge('a', 'fx', 'call')]).size).toBe(0)
    const two = [node('start', { root: true }), node('a'), node('b'), node('fx', { returns: true })]
    const edges = [edge('start', 'a'), edge('a', 'b'), edge('a', 'fx', 'call'), edge('b', 'fx', 'call')]
    expect([...findHelpers(two, edges)]).toEqual(['fx'])
  })

  it('never takes the start scene, screens or missing targets', () => {
    const nodes = [
      node('start', { root: true, returns: true }),
      node('screen:menu', { kind: 'screen', returns: true }),
      node('gone', { kind: 'missing' }),
      node('a'),
      node('b'),
      node('c'),
    ]
    const edges = ['a', 'b', 'c'].flatMap((f) => [
      edge(f, 'start', 'call'),
      edge(f, 'screen:menu', 'call'),
      edge(f, 'gone', 'call'),
    ])
    expect(findHelpers(nodes, edges).size).toBe(0)
  })

  it('keeps nothing when most of the project would count as helpers', () => {
    const nodes = ['a', 'b', 'c', 'd'].map((id) => node(id, { returns: true }))
    const edges: MapEdge[] = []
    for (const from of nodes) for (const to of nodes) if (from !== to) edges.push(edge(from.id, to.id, 'call'))
    expect(findHelpers(nodes, edges).size).toBe(0)
  })
})

describe('flowGroups', () => {
  const clique = (names: string[]) =>
    names.flatMap((a, i) => names.slice(i + 1).map((b) => edge(a, b)))

  it('splits two tight groups joined by one link', () => {
    const left = ['a1', 'a2', 'a3', 'a4']
    const right = ['b1', 'b2', 'b3', 'b4']
    const out = flowGroups([...left, ...right], [...clique(left), ...clique(right), edge('a1', 'b1')])
    expect(new Set(left.map((id) => out.get(id))).size).toBe(1)
    expect(new Set(right.map((id) => out.get(id))).size).toBe(1)
    expect(out.get('a1')).not.toBe(out.get('b1'))
  })

  it('names a group after its most linked scene', () => {
    const out = flowGroups(['x', 'hub', 'y', 'z'], [edge('hub', 'x'), edge('hub', 'y'), edge('hub', 'z')])
    expect(new Set(out.values())).toEqual(new Set(['hub']))
  })

  it('folds a tiny group into the neighbour it links to most', () => {
    const big = ['a1', 'a2', 'a3', 'a4']
    const out = flowGroups([...big, 'p', 'q'], [...clique(big), edge('a2', 'p'), edge('p', 'q')])
    expect(out.get('p')).toBe(out.get('a1'))
    expect(out.get('q')).toBe(out.get('a1'))
  })

  it('puts scenes with no links in the unlinked group', () => {
    const out = flowGroups(['a', 'b', 'c', 'lonely'], [edge('a', 'b'), edge('b', 'c'), edge('a', 'c')])
    expect(out.get('lonely')).toBe(UNLINKED_KEY)
  })

  it('gives the same answer every time', () => {
    const names = ['a1', 'a2', 'a3', 'b1', 'b2', 'b3', 'c1']
    const edges = [...clique(names.slice(0, 3)), ...clique(names.slice(3, 6)), edge('a1', 'b1'), edge('b3', 'c1')]
    const first = [...flowGroups(names, edges)]
    expect([...flowGroups(names, edges)]).toEqual(first)
  })

  it('handles an empty graph and self links', () => {
    expect(flowGroups([], []).size).toBe(0)
    expect(flowGroups(['a'], [edge('a', 'a')]).get('a')).toBe(UNLINKED_KEY)
  })
})

describe('assignGroups', () => {
  it('moves helpers and missing targets to their own groups in every mode', () => {
    const { nodes, edges } = withHelper(6)
    nodes.push(node('gone', { kind: 'missing' }))
    edges.push(edge('scene_end', 'gone'))
    const helpers = findHelpers(nodes, edges)
    for (const mode of ['auto', 'flow', 'prefix', 'file'] as const) {
      const out = assignGroups(nodes, edges, files, mode, helpers)
      expect(out.get('beep')).toBe(HELPERS_KEY)
      expect(out.get('gone')).toBe(MISSING_KEY)
      expect(out.size).toBe(nodes.length)
    }
  })

  it('uses name prefixes when most scenes have one, and story flow when they do not', () => {
    const prefixed = ['v1a', 'v1b', 'v1c', 'v2a', 'v2b', 'v2c'].map((id) => node(id))
    expect(resolveMode('auto', prefixed, [])).toBe('prefix')
    const plain = ['goali', 'gonil', 'gopark', 'gohome', 'gocity', 'goshop'].map((id) => node(id))
    expect(resolveMode('auto', plain, [])).toBe('flow')
    expect(resolveMode('file', plain, [])).toBe('file')
  })

  it('keeps the label a helper jumps to in the story', () => {
    const story = ['start', 'park', 'night', 'end']
    const nodes = [
      node('start', { root: true }),
      node('park'),
      node('night'),
      node('end', { endsScript: true }),
      node('town', { kind: 'screen' }),
      node('minigame'),
      node('minigame_done', { returns: true }),
    ]
    const edges = story.flatMap((a, i) => story.slice(i + 1).map((b) => edge(a, b)))
    edges.push(edge('park', 'minigame', 'call'), edge('night', 'minigame', 'call'), edge('town', 'minigame', 'call'))
    edges.push(edge('minigame', 'minigame_done'))
    const helpers = findHelpers(nodes, edges)
    expect([...helpers]).toEqual(['minigame'])
    const out = assignGroups(nodes, edges, files, 'flow', helpers)
    expect(out.get('minigame')).toBe(HELPERS_KEY)
    expect(out.get('minigame_done')).toBe(out.get('park'))
    expect(out.get('minigame_done')).not.toBe(UNLINKED_KEY)
  })

  it('groups by file when asked', () => {
    const nodes = [node('a'), node('b')]
    const out = assignGroups(nodes, [], files, 'file', new Set())
    expect(out.get('a')).toBe('script.rpy')
  })
})

describe('group titles', () => {
  it('names the special groups and leaves scene names alone', () => {
    expect(groupTitle(HELPERS_KEY)).toBe('Helpers')
    expect(groupTitle(UNLINKED_KEY)).toBe('Unlinked')
    expect(groupTitle(MISSING_KEY)).toBe('Missing targets')
    expect(groupTitle('goaliroom')).toBe('goaliroom')
    expect(isTileGroup(HELPERS_KEY)).toBe(true)
    expect(isTileGroup(MISSING_KEY)).toBe(false)
  })
})

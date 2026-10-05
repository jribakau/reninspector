import { describe, expect, it } from 'vitest'
import { firstLayerIds } from './layout'
import {
  fitChars,
  hairline,
  mapLevel,
  markRect,
  markState,
  nearestBox,
  nodeAt,
  packEdges,
  rangeText,
  roundedPath,
  storyDepth,
  titleFont,
  wrapText,
} from './graph'

describe('graph helpers', () => {
  it('wraps words and keeps at most six lines', () => {
    expect(wrapText('one two three', 7)).toEqual(['one two', 'three'])
    expect(wrapText('a '.repeat(20), 3).length).toBe(6)
  })

  it('describes a line range', () => {
    expect(rangeText(4, 4)).toBe('line 4')
    expect(rangeText(4, 9)).toBe('lines 4–9')
  })

  it('packs edges by kind and lifts the selection', () => {
    const packed = packEdges(
      [0, 1, 2],
      2,
      [
        { d: 'M0', index: 0 },
        { d: 'M1', index: 1 },
        { d: 'M2', index: 2 },
      ],
      [
        { from: 1, to: 2, kind: 'next' },
        { from: 3, to: 4, kind: 'next' },
        { from: 5, to: 6, kind: 'choice' },
      ],
    )
    expect(packed.hot).toBe('M0')
    expect(packed.batches).toEqual([
      { kind: 'next', d: 'M1' },
      { kind: 'choice', d: 'M2' },
    ])
  })

  it('hits the node under a point', () => {
    const nodes = [
      { id: 1, x: 0, y: 0, w: 10, h: 10 },
      { id: 2, x: 20, y: 0, w: 10, h: 10 },
    ]
    expect(nodeAt(nodes, 5, 5, 1)?.id).toBeUndefined()
    expect(nodeAt(nodes, 22, 2, 1)?.id).toBe(2)
  })
})

describe('firstLayerIds', () => {
  it('skips a node that already has an incoming edge', () => {
    const incoming = new Set(['screen:game_menu', 'later'])
    expect(firstLayerIds(['screen:game_menu', 'later', 'open'], incoming, new Set())).toEqual(['open'])
    expect(firstLayerIds(['screen:game_menu'], incoming, new Set(['screen:game_menu']))).toEqual([])
  })

  it('pins every root that nothing points at', () => {
    expect(firstLayerIds(['start', 'other', 'mid'], new Set(['mid']), new Set(['start', 'other']))).toEqual([
      'start',
      'other',
    ])
  })
})

describe('map detail levels', () => {
  it('shows group boxes alone when zoomed far out or crowded', () => {
    expect(mapLevel(0.1, 10).show).toBe(false)
    expect(mapLevel(0.5, 5000).show).toBe(false)
    expect(mapLevel(0.14, 10).show).toBe(true)
  })

  it('steps up with zoom and down with crowding', () => {
    expect(mapLevel(0.2, 50).tier).toBe(0)
    expect(mapLevel(0.4, 50).tier).toBe(1)
    expect(mapLevel(0.9, 50).tier).toBe(2)
    expect(mapLevel(0.9, 300).tier).toBe(1)
    expect(mapLevel(0.9, 600).tier).toBe(0)
  })

  it('keeps titles about 9.5px on screen until they reach the node height', () => {
    expect(titleFont(1)).toBe(13)
    expect(titleFont(0.5)).toBeCloseTo(19)
    expect(titleFont(0.1)).toBe(24)
    expect(titleFont(0)).toBe(24)
  })

  it('thickens hairlines as the map shrinks, within bounds', () => {
    expect(hairline(2)).toBe(1)
    expect(hairline(0.5)).toBeCloseTo(2.2)
    expect(hairline(0.01)).toBe(6)
  })

  it('never fits fewer than three characters', () => {
    expect(fitChars(200, 13, 16)).toBe(24)
    expect(fitChars(50, 24, 40)).toBe(3)
  })
})

describe('node marks', () => {
  it('picks a look, with start scenes first and then problems', () => {
    expect(markState(undefined)).toBe('label')
    expect(markState({ kind: 'label', root: false, reachable: true })).toBe('label')
    expect(markState({ kind: 'screen', root: false, reachable: true })).toBe('screen')
    expect(markState({ kind: 'label', root: false, reachable: false })).toBe('unreachable')
    expect(markState({ kind: 'missing', root: false, reachable: false })).toBe('missing')
    expect(markState({ kind: 'label', root: true, reachable: false })).toBe('root')
  })

  it('writes a node as a short closed path', () => {
    expect(markRect(10, 20, 112.04, 44)).toBe('M10 20h112v44h-112z')
    expect(markRect(0.26, 0, 5, 5)).toBe('M0.3 0h5v5h-5z')
  })

  const boxes = [
    { id: 'a', x: 0, y: 0, w: 100, h: 40 },
    { id: 'b', x: 120, y: 0, w: 100, h: 40 },
  ]

  it('finds the node under a point, or just beside it', () => {
    expect(nearestBox(boxes, 50, 20, 2)?.id).toBe('a')
    expect(nearestBox(boxes, 101, 20, 2)?.id).toBe('a')
    expect(nearestBox(boxes, 119, 20, 2)?.id).toBe('b')
  })

  it('prefers the closer node and gives up beyond the reach', () => {
    expect(nearestBox(boxes, 112, 20, 20)?.id).toBe('b')
    expect(nearestBox(boxes, 110, 20, 2)).toBeNull()
    expect(nearestBox([], 0, 0, 5)).toBeNull()
  })

  it('reaches further the more the map is zoomed out', () => {
    // A click 30 units past the right edge of the second node. 2px of screen is 40 units at a zoom of 0.05, 2 units at 1.
    expect(nearestBox(boxes, 250, 20, 2 / 0.05)?.id).toBe('b')
    expect(nearestBox(boxes, 250, 20, 2 / 1)).toBeNull()
  })
})

describe('roundedPath', () => {
  it('keeps a straight line straight', () => {
    expect(roundedPath([{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 20, y: 0 }], 8)).toBe('M0 0 L10 0 L20 0')
  })

  it('rounds an L-shape', () => {
    const d = roundedPath([{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }], 4)
    expect(d).toBe('M0 0 L6 0 Q10 0 10 4 L10 10')
  })

  it('clamps the radius to half of a very short segment', () => {
    const d = roundedPath([{ x: 0, y: 0 }, { x: 2, y: 0 }, { x: 2, y: 2 }], 10)
    expect(d).toBe('M0 0 L1 0 Q2 0 2 1 L2 2')
  })
})

describe('storyDepth', () => {
  const nodes = [
    { id: 'start', root: true },
    { id: 'a', root: false },
    { id: 'b', root: false },
    { id: 'lost', root: false },
    { id: 'gone', root: false, missing: true },
  ]

  it('walks a cycle from the root and parks unreachable nodes after', () => {
    const depth = storyDepth(nodes, [
      { from: 'start', to: 'a' },
      { from: 'a', to: 'b' },
      { from: 'b', to: 'a' },
      { from: 'start', to: 'gone' },
    ])
    expect(depth.get('start')).toBe(0)
    expect(depth.get('a')).toBe(1)
    expect(depth.get('b')).toBe(2)
    expect(depth.get('lost')).toBe(3)
    expect(depth.get('gone')).toBe(4)
  })

  it('starts every root at zero', () => {
    const depth = storyDepth(
      [
        { id: 's1', root: true },
        { id: 's2', root: true },
        { id: 'a', root: false },
        { id: 'gone', root: false, missing: true },
      ],
      [
        { from: 's1', to: 'a' },
        { from: 's2', to: 'a' },
      ],
    )
    expect(depth.get('s1')).toBe(0)
    expect(depth.get('s2')).toBe(0)
    expect(depth.get('a')).toBe(1)
    expect(depth.get('gone')).toBe(3)
  })
})
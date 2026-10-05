import { describe, expect, it } from 'vitest'
import { nodeAt, packEdges, rangeText, wrapText } from './graph'

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
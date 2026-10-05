import type { Chunk } from '@codemirror/merge'
import { Text } from '@codemirror/state'
import { describe, expect, it } from 'vitest'
import { buildMarks } from './gitGutter'

function chunk(fromA: number, toA: number, fromB: number, toB: number, endB = Math.max(fromB, toB - 1)): Chunk {
  return { fromA, toA, fromB, toB, endB } as Chunk
}

describe('git gutter marks', () => {
  it('marks a pure insertion as added and a replacement as modified', () => {
    const doc = Text.of(['one', 'two', 'three'])
    const added = buildMarks(doc, [chunk(4, 4, 4, 8)])
    expect(added.get(2)?.kind).toBe('added')

    const modified = buildMarks(doc, [chunk(4, 7, 4, 9)])
    expect(modified.get(2)?.kind).toBe('modified')
    expect(modified.has(1)).toBe(false)
  })

  it('pins a deletion to the line it left', () => {
    const doc = Text.of(['keep', 'stay'])
    const marks = buildMarks(doc, [chunk(5, 10, 5, 5, 4)])
    expect(marks.get(1)?.kind).toBe('deleted')
  })
})

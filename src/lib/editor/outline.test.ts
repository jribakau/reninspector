import { describe, expect, it } from 'vitest'
import { enclosing, mergeOutline, type OutlineEntry } from './outline'

const entries: OutlineEntry[] = [
  { id: 'screen:phone', kind: 'screen', name: 'phone', line: 10, endLine: 40 },
  { id: 'start', kind: 'label', name: 'start', line: 12, endLine: 30 },
  { id: 'choices', kind: 'menu', name: 'choices', line: 18, endLine: 28 },
  { id: 'transform:left', kind: 'transform', name: 'left', line: 1, endLine: 4 },
  { id: 'style:say_dialogue', kind: 'style', name: 'say_dialogue', line: 50, endLine: 52 },
]

describe('outline', () => {
  it('sorts by source position', () => {
    expect(mergeOutline(entries).map((e) => e.name)).toEqual(['left', 'phone', 'start', 'choices', 'say_dialogue'])
  })

  it('lists the enclosing chain from the outside in', () => {
    expect(enclosing(entries, 20).map((e) => e.kind)).toEqual(['screen', 'label', 'menu'])
    expect(enclosing(entries, 2).map((e) => e.name)).toEqual(['left'])
    expect(enclosing(entries, 51).map((e) => e.kind)).toEqual(['style'])
    expect(enclosing(entries, 45)).toEqual([])
  })
})

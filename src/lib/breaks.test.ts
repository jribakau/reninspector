import { describe, expect, it } from 'vitest'
import { breakKey, linesFor, togglePoint } from './breaks'

describe('breakpoints', () => {
  it('toggles one file and line', () => {
    const once = togglePoint([], 'chapter\\a.rpy', 12)
    expect(once).toEqual(['chapter/a.rpy:12'])
    expect(togglePoint(once, 'chapter/a.rpy', 12)).toEqual([])
    expect(linesFor('chapter/a.rpy', once)).toEqual([12])
    expect(linesFor('other.rpy', once)).toEqual([])
  })

  it('ignores line 0 and keeps the key stable', () => {
    expect(togglePoint(['a.rpy:1'], 'a.rpy', 0)).toEqual(['a.rpy:1'])
    expect(breakKey('a.rpy', 4)).toBe('a.rpy:4')
  })
})

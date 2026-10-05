import { describe, expect, it } from 'vitest'
import { linkMode, linkTitle, previewHits } from './ctrlLink'

describe('ctrl-hover links', () => {
  it('treats a variable as a reference table and everything else as a jump', () => {
    expect(linkMode('variable')).toBe('refs')
    expect(linkMode('label')).toBe('jump')
    expect(linkMode('screen')).toBe('jump')
    expect(linkMode('image')).toBe('jump')
  })

  it('names a jump by its destination', () => {
    expect(linkTitle('label', 'start', 'label')).toBe('Jump to start')
    expect(linkTitle('screen', 'say', null)).toBe('Go to screen say')
    expect(linkTitle('variable', 'flags', 'variable')).toBe('flags')
  })

  it('keeps a short preview of a long reference list', () => {
    const hits = Array.from({ length: 10 }, (_, i) => i)
    expect(previewHits(hits)).toEqual({ shown: [0, 1, 2, 3, 4, 5, 6, 7], more: 2 })
    expect(previewHits([1, 2]).more).toBe(0)
  })
})
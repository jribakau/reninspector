import { EditorState } from '@codemirror/state'
import { describe, expect, it } from 'vitest'
import { hintsOnLine, labelWordCounts, sayWords } from './inlay'

const doc = `label start:
    e "Hello there"
    "A narrator line"
    menu:
        "Yes":
            jump shop
        "No":
            e "Maybe later"
    jump shop
label shop:
    e "Welcome"
`

describe('sayWords', () => {
  it('counts dialogue and skips menu choices', () => {
    expect(sayWords('    e "Hello there"')).toBe(2)
    expect(sayWords('    "A narrator line"')).toBe(3)
    expect(sayWords('        "Yes":')).toBe(0)
    expect(sayWords('    e "Hi {w} there"')).toBe(2)
  })
})

describe('labelWordCounts', () => {
  it('sums dialogue per label and does not double-count a nested one', () => {
    const state = EditorState.create({ doc })
    const counts = labelWordCounts(state.doc)
    expect(counts.get(1)).toBe(2 + 3 + 2)
    expect(counts.get(10)).toBe(1)
  })
})

describe('labelWordCounts edges', () => {
  it('stops counting at a following top-level statement and ignores UI text', () => {
    const src = 'label a:\n    e "One two"\nscreen s():\n    text "Not dialogue here"\n    voice "x.ogg"\n'
    const counts = labelWordCounts(EditorState.create({ doc: src }).doc)
    expect(counts.get(1)).toBe(2)
    expect(sayWords('    text "Not dialogue"')).toBe(0)
    expect(sayWords('    textbutton "Go"')).toBe(0)
  })
})

describe('hintsOnLine', () => {
  const names = (speaker: string) => (speaker === 'e' ? 'Eileen' : undefined)
  const sites = (name: string) => (name === 'shop' ? { path: 'game/script.rpy', line: 10 } : undefined)

  it('shows the display name, the jump target, and the word count', () => {
    expect(hintsOnLine('    e "Hello"', 2, undefined, names, sites)).toEqual([{ at: 5, text: 'Eileen' }])
    expect(hintsOnLine('    jump shop', 8, undefined, names, sites)).toEqual([{ at: 13, text: 'script.rpy:10' }])
    expect(hintsOnLine('    call screen phone', 3, undefined, names, sites)).toEqual([])
    expect(hintsOnLine('label start:', 1, 7, names, sites)).toEqual([{ at: 12, text: '7 words' }])
    expect(hintsOnLine('label start:', 1, 0, names, sites)).toEqual([])
  })
})

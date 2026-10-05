import { EditorSelection, EditorState } from '@codemirror/state'
import { describe, expect, it } from 'vitest'
import { selectionCovers, selectionText } from './editMenu'

function state(doc: string, anchor: number, head = anchor) {
  return EditorState.create({ doc, selection: EditorSelection.single(anchor, head) })
}

describe('editor selection', () => {
  it('copies only the text inside a selection', () => {
    const doc = 'jump start'
    expect(selectionText(state(doc, 5, 10))).toBe('start')
    expect(selectionText(state(doc, 4))).toBe('')
  })

  it('treats either edge of a selection as inside it', () => {
    const doc = 'jump start'
    const here = state(doc, 5, 10)
    expect(selectionCovers(here, 5)).toBe(true)
    expect(selectionCovers(here, 10)).toBe(true)
    expect(selectionCovers(here, 4)).toBe(false)
  })
})

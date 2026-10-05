import { EditorState } from '@codemirror/state'
import { describe, expect, it } from 'vitest'
import { renpyFold, renpyIndent } from './fold'

const src = `label start:
    e "Hi"
    # still inside
    menu:
        "Yes":
            jump later

label later:
    return
`

describe('renpy folds and indent', () => {
  it('folds a block through comments and stops at the next sibling', () => {
    const editor = EditorState.create({ doc: src })
    const fold = renpyFold(editor, 0)
    expect(fold).not.toBeNull()
    const text = editor.doc.sliceString(fold!.from, fold!.to)
    expect(text).toContain('menu:')
    expect(text).not.toContain('label later')
  })

  it('does not fold a line that is not a block header', () => {
    const editor = EditorState.create({ doc: src })
    const line = editor.doc.line(2)
    expect(renpyFold(editor, line.from)).toBeNull()
  })

  it('indents one step after a colon', () => {
    const editor = EditorState.create({ doc: 'label start:\n' })
    const indent = renpyIndent(() => 4)
    const next = editor.doc.line(2).from
    expect(indent({ state: editor }, next)).toBe(4)
  })
})

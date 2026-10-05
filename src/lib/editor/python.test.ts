import { EditorState } from '@codemirror/state'
import { describe, expect, it } from 'vitest'
import { inPython, pythonLine } from './python'

const src = `label start:
    e "Hi"
init python:
    def greet(who):
        return who
    # still python
    x = 1

label later:
    $ points += 1
    e "Next"
python early:
    class Box:
        pass
`

describe('python regions', () => {
  it('recognises headers without treating them as inline code', () => {
    expect(pythonLine('init python:', null)).toEqual({ indent: 0, inline: false })
    expect(pythonLine('    python early:', null)).toEqual({ indent: 4, inline: false })
    expect(pythonLine('init -1 python hide:', null).inline).toBe(false)
    expect(pythonLine('    $ points += 1', null)).toEqual({ indent: null, inline: true })
  })

  it('keeps a block open across comments and blanks, and closes it at the next statement', () => {
    const doc = EditorState.create({ doc: src }).doc
    expect(inPython(doc, 3)).toBe(false)
    expect(inPython(doc, 4)).toBe(true)
    expect(inPython(doc, 6)).toBe(false)
    expect(inPython(doc, 7)).toBe(true)
    expect(inPython(doc, 8)).toBe(false)
    expect(inPython(doc, 9)).toBe(false)
    expect(inPython(doc, 10)).toBe(true)
    expect(inPython(doc, 11)).toBe(false)
    expect(inPython(doc, 13)).toBe(true)
  })
})

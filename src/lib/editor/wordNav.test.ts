import { EditorState } from '@codemirror/state'
import { describe, expect, it } from 'vitest'
import { isStylePrefixValue, preferHere, preferredKind, wordAt } from './wordNav'

function state(doc: string) {
  return EditorState.create({ doc })
}

describe('word navigation', () => {
  it('reads a dotted name under the caret', () => {
    const doc = '    jump store.flag'
    const found = wordAt(state(doc), doc.indexOf('store') + 2)
    expect(found?.text).toBe('store.flag')
  })

  it('ignores punctuation that is not part of a name', () => {
    expect(wordAt(state('    e "Hi"'), 6)).toBeNull()
  })

  it('picks a kind from the statement in front of the word', () => {
    expect(preferredKind('    jump start', 9, 'start')).toBe('label')
    expect(preferredKind('    call start', 9, 'start')).toBe('label')
    expect(preferredKind('    show eileen happy', 9, 'eileen')).toBe('image')
    expect(preferredKind('    e "Hello"', 4, 'e')).toBe('character')
    expect(preferredKind('    jump start  # start', 18, 'start')).toBeNull()
  })

  it('treats the quoted argument of style_prefix as a prefix', () => {
    const line = '    textbutton "Go" style_prefix "say"  # style_prefix "no"'
    const at = line.indexOf('say')
    expect(isStylePrefixValue(line, at, 'say')).toBe(true)
    expect(isStylePrefixValue(line, line.indexOf('Go'), 'Go')).toBe(false)
    expect(isStylePrefixValue(line, line.lastIndexOf('style_prefix') + 'style_prefix "'.length, 'no')).toBe(false)
  })

  it('treats a call in a python line as a function', () => {
    const doc = 'label start:\n    $ greet(who)\n'
    const line = state(doc).doc.line(2)
    expect(preferHere(state(doc), 2, line.text, line.text.indexOf('greet'), 'greet')).toBe('function')
  })
})

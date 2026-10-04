import { Chunk } from '@codemirror/merge'
import { Text } from '@codemirror/state'
import { expect, it } from 'vitest'
import { revertEdit, takeChunk } from './chunks'

function doc(body: string): Text {
  return Text.of(body.split('\n'))
}

/** Reverting every chunk of B puts A back. */
function reverted(a: string, b: string): string {
  const left = doc(a)
  const right = doc(b)
  let out = b
  for (const chunk of [...Chunk.build(left, right)].reverse()) {
    const edit = revertEdit(left, right, chunk)
    out = out.slice(0, edit.from) + edit.insert + out.slice(edit.to)
  }
  return out
}

function taken(a: string, b: string, index: number): string {
  const left = doc(a)
  const right = doc(b)
  const chunks = Chunk.build(left, right)
  return takeChunk(left, right, chunks[index])
}

function oneChange(a: string, b: string) {
  const chunks = Chunk.build(doc(a), doc(b))
  expect(chunks).toHaveLength(1)
  expect(reverted(a, b)).toBe(a)
  expect(taken(a, b, 0)).toBe(b)
}

it('reverts and takes a change in the middle of the file', () => {
  oneChange('one\ntwo\nthree\n', 'one\nTWO\nthree\n')
})

it('reverts and takes lines added at the start, middle, and end', () => {
  oneChange('two\nthree\n', 'one\ntwo\nthree\n')
  oneChange('one\nthree\n', 'one\ntwo\nthree\n')
  oneChange('one\ntwo\n', 'one\ntwo\nthree\n')
})

it('reverts and takes lines deleted at the start, middle, and end', () => {
  oneChange('one\ntwo\nthree\n', 'two\nthree\n')
  oneChange('one\ntwo\nthree\n', 'one\nthree\n')
  oneChange('one\ntwo\nthree\n', 'one\ntwo\n')
})

it('keeps the last line correct with and without a trailing newline', () => {
  oneChange('one\ntwo\n', 'one\ntwo')
  oneChange('one\ntwo', 'one\ntwo\n')
  oneChange('one\ntwo', 'one\nTWO')
  oneChange('one\ntwo\n', 'one\nTWO')
})

it('treats an empty original as an untracked file', () => {
  oneChange('', 'hello\n')
})

it('leaves identical text unchanged', () => {
  expect(Chunk.build(doc('same\n'), doc('same\n'))).toHaveLength(0)
  expect(reverted('same\n', 'same\n')).toBe('same\n')
})

it('takes one chunk and leaves the other change on the original', () => {
  const a = 'a\nb\nc\n'
  const b = 'A\nb\nC\n'
  expect(Chunk.build(doc(a), doc(b))).toHaveLength(2)
  expect(reverted(a, b)).toBe(a)
  expect(taken(a, b, 0)).toBe('A\nb\nc\n')
  expect(taken(a, b, 1)).toBe('a\nb\nC\n')
})

it('clamps a revert that starts past the end of the document', () => {
  const edit = revertEdit(doc('a\n'), doc(''), { fromA: 0, toA: 2, fromB: 10, toB: 12 })
  expect(edit.from).toBe(0)
  expect(edit.to).toBe(0)
  expect(edit.from).toBeLessThanOrEqual(edit.to)
})

import { HighlightStyle, StreamLanguage, syntaxHighlighting, type StringStream } from '@codemirror/language'
import { tags as t } from '@lezer/highlight'

/** Statement keywords, highlighted when they start a line. */
const STATEMENT = new Set([
  'label', 'menu', 'jump', 'call', 'return', 'if', 'elif', 'else', 'while', 'for', 'pass',
  'show', 'scene', 'hide', 'with', 'define', 'default', 'image', 'init', 'python', 'screen',
  'transform', 'style', 'translate', 'play', 'stop', 'queue', 'pause', 'voice', 'window',
  'nvl', 'camera', 'layeredimage', 'testcase', 'rpy', 'def', 'class', 'import', 'from', 'try',
  'except', 'finally', 'raise', 'global', 'lambda', 'use', 'text', 'add', 'vbox', 'hbox',
  'frame', 'imagebutton', 'textbutton', 'key', 'timer', 'on', 'action', 'fixed', 'grid',
])

/** Words that are keywords anywhere in a line. */
const INLINE = new Set([
  'at', 'as', 'behind', 'zorder', 'onlayer', 'expression', 'from', 'in', 'and', 'or', 'not',
  'is', 'True', 'False', 'None', 'early', 'hide', 'with', 'pass', 'set', 'fadein', 'fadeout',
  'loop', 'noloop', 'volume', 'channel', 'sound', 'music', 'audio', 'ambient',
])

interface State {
  triple: string | null
  quote: string | null
  first: boolean
  expect: 'def' | 'ref' | null
}

function readString(stream: StringStream, state: State): string {
  const close = state.triple ?? state.quote ?? '"'
  if (stream.peek() === '{' && !stream.match('{{', false)) {
    stream.next()
    while (!stream.eol() && stream.peek() !== '}' && !stream.match(close, false)) stream.next()
    if (stream.peek() === '}') stream.next()
    return 'meta'
  }
  let progressed = false
  while (!stream.eol()) {
    if (stream.match(close)) {
      state.triple = null
      state.quote = null
      return 'string'
    }
    const c = stream.peek()
    if (c === '\\') {
      stream.next()
      stream.next()
      progressed = true
      continue
    }
    if (c === '{' && progressed && !stream.match('{{', false)) return 'string'
    stream.next()
    progressed = true
  }
  // A single-quoted string that is still open at the end of the line is treated as ended,
  // so one stray quote cannot recolor the rest of a 200k line file.
  if (!state.triple) state.quote = null
  return 'string'
}

export const renpyLanguage = StreamLanguage.define<State>({
  name: 'renpy',
  startState: () => ({ triple: null, quote: null, first: true, expect: null }),
  token(stream, state) {
    if (state.triple || state.quote) return readString(stream, state)
    if (stream.sol()) {
      state.first = true
      state.expect = null
    }
    if (stream.eatSpace()) return null
    const ch = stream.peek()
    if (ch === '#') {
      stream.skipToEnd()
      return 'comment'
    }
    if (ch === '"' || ch === "'") {
      state.expect = null
      state.first = false
      if (stream.match(ch + ch + ch)) {
        state.triple = ch + ch + ch
      } else {
        stream.next()
        state.quote = ch
      }
      return 'string'
    }
    if (stream.match(/^-?\d+(\.\d+)?/)) {
      state.first = false
      return 'number'
    }
    if (ch === '$') {
      stream.next()
      state.first = false
      return 'operator'
    }
    const word = stream.match(/^[A-Za-z_][A-Za-z0-9_]*/) as RegExpMatchArray | null
    if (word) {
      const w = word[0]
      const wasFirst = state.first
      state.first = false
      if (state.expect) {
        const kind = state.expect
        state.expect = null
        return kind === 'def' ? 'def' : 'link'
      }
      if (wasFirst && STATEMENT.has(w)) {
        if (w === 'label' || w === 'menu') state.expect = 'def'
        else if (w === 'jump' || w === 'call') state.expect = 'ref'
        return 'keyword'
      }
      if (INLINE.has(w)) return 'keyword'
      if (wasFirst && stream.match(/^(\s+[A-Za-z_@-][\w@-]*)*\s+["']/, false)) return 'atom'
      return 'variableName'
    }
    stream.next()
    state.expect = null
    state.first = false
    if ('()[]{}'.includes(ch ?? '')) return 'bracket'
    if ('=+-*/<>!%&|^~'.includes(ch ?? '')) return 'operator'
    return null
  },
  blankLine(state) {
    state.first = true
  },
  languageData: { commentTokens: { line: '#' } },
})

export const renpyHighlight = syntaxHighlighting(
  HighlightStyle.define([
    { tag: t.keyword, color: 'var(--syn-keyword)', fontWeight: '600' },
    { tag: t.string, color: 'var(--syn-string)' },
    { tag: t.comment, color: 'var(--syn-comment)', fontStyle: 'italic' },
    { tag: t.number, color: 'var(--syn-number)' },
    { tag: t.meta, color: 'var(--syn-tag)' },
    { tag: t.atom, color: 'var(--syn-speaker)' },
    { tag: t.definition(t.variableName), color: 'var(--syn-def)', fontWeight: '700' },
    { tag: t.link, color: 'var(--syn-ref)', textDecoration: 'none' },
    { tag: t.operator, color: 'var(--syn-op)' },
    { tag: t.variableName, color: 'var(--text)' },
    { tag: t.bracket, color: 'var(--dim)' },
  ]),
)

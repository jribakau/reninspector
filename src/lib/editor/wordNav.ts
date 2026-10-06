import type { EditorState } from '@codemirror/state'
import type { EditorView } from '@codemirror/view'
import { inPython } from './python'
import type { Symbol } from '../types'

export interface WordSpan {
  from: number
  to: number
  text: string
}

/** The identifier under `pos`, including dotted names such as `store.flag`. */
export function wordAt(state: EditorState, pos: number): WordSpan | null {
  const line = state.doc.lineAt(pos)
  const text = line.text
  let at = pos - line.from
  if (at > 0 && at === text.length) at -= 1
  if (at < 0 || at >= text.length || !/[\w.]/.test(text[at])) return null
  let from = at
  let to = at + 1
  while (from > 0 && /[\w.]/.test(text[from - 1])) from -= 1
  while (to < text.length && /[\w.]/.test(text[to])) to += 1
  const word = text.slice(from, to)
  if (!word) return null
  return { from: line.from + from, to: line.from + to, text: word }
}

/** The Ren'Py statement in front of a word, when it picks a symbol kind. */
export function preferredKind(line: string, wordStart: number, word: string): string | null {
  const hash = line.indexOf('#')
  const code = hash >= 0 ? line.slice(0, hash) : line
  if (wordStart >= code.length) return null
  const before = code.slice(0, wordStart)
  const after = code.slice(wordStart + word.length)
  const prev = before.trim().split(/\s+/).pop() ?? ''
  if (prev === 'jump' || prev === 'call' || prev === 'menu' || prev === 'label') return 'label'
  if (prev === 'screen') return 'screen'
  if (prev === 'show' || prev === 'scene' || prev === 'hide' || prev === 'image') return 'image'
  if (prev === 'transform') return 'transform'
  if (prev === 'define' || prev === 'default') return 'variable'
  if ((prev === 'at' || prev.endsWith(',')) && /\bat(?:\s|$)/.test(before)) return 'transform'
  if (prev === 'def' || prev === 'class') return prev === 'class' ? 'class' : 'function'
  if (/^\s*$/.test(before) && /["']/.test(after)) return 'character'
  return null
}

/** The word is the quoted value of `style_prefix`, so a rename is a prefix rename. */
export function isStylePrefixValue(line: string, wordStart: number, word: string): boolean {
  const hash = line.indexOf('#')
  if (hash >= 0 && wordStart >= hash) return false
  const limit = hash >= 0 ? hash : line.length
  if (wordStart + word.length > limit) return false
  const before = line.slice(0, wordStart)
  const after = line.slice(wordStart + word.length, limit)
  if (!/^["']/.test(after)) return false
  return /style_prefix\s*["']$/.test(before)
}

/** Kind hint for the word at `wordStart` on a 1-based line. */
export function preferHere(state: EditorState, lineNo: number, line: string, wordStart: number, word: string): string | null {
  const base = preferredKind(line, wordStart, word)
  if (base) return base
  const after = line.slice(wordStart + word.length)
  if (inPython(state.doc, lineNo) && after.trimStart().startsWith('(')) return 'function'
  return null
}

export interface SymbolLookup {
  path: string | null
  resolveSymbol(path: string, line: number, word: string, prefer: string | null): Promise<{ symbol?: Symbol | null; hit?: boolean }>
  symbolsOf(kind: string): Symbol[]
  lookupSymbol(word: string): Symbol | undefined
}

export async function symbolHere(lineNo: number, word: string, prefer: string | null, lookup: SymbolLookup): Promise<Symbol | undefined> {
  if (lookup.path) {
    try {
      const found = await lookup.resolveSymbol(lookup.path, lineNo, word, prefer)
      if (found.symbol) return found.symbol
      if (found.hit) return undefined
    } catch {
      // The name table still answers when the project command fails.
    }
  }
  if (prefer === 'function' || prefer === 'class') {
    return lookup.symbolsOf(prefer).find((s) => s.name === word)
  }
  return lookup.lookupSymbol(word)
}

export interface WordActions {
  lookup: SymbolLookup
  inPythonLine(doc: EditorState['doc'], lineNo: number): boolean
  pythonDefinition(line: number, ch: number): Promise<'external' | { path: string; line: number } | null | undefined>
  notice(text: string): void
  goto(file: string, line: number): void
  refs(kind: string, name: string): void
  rename(kind: string, name: string): void
}

/** Go to, find, or rename the word at the caret. Returns false when there is no word. */
export function followWord(view: EditorView, how: 'goto' | 'refs' | 'rename', actions: WordActions): boolean {
  const found = wordAt(view.state, view.state.selection.main.head)
  if (!found) return false
  const line = view.state.doc.lineAt(found.from)
  if (how === 'rename' && isStylePrefixValue(line.text, found.from - line.from, found.text)) {
    actions.rename('style-prefix', found.text)
    return true
  }
  const prefer = preferHere(view.state, line.number, line.text, found.from - line.from, found.text)
  void symbolHere(line.number, found.text, prefer, actions.lookup).then(async (sym) => {
    if (!sym) {
      if (how !== 'goto' || !actions.inPythonLine(view.state.doc, line.number)) return
      const hit = await actions.pythonDefinition(line.number - 1, found.from - line.from)
      if (hit === 'external') actions.notice('That definition is outside this project, so it was not opened.')
      else if (hit) actions.goto(hit.path, hit.line)
      return
    }
    if (how === 'goto') {
      if (sym.path) actions.goto(sym.path, sym.line)
      return
    }
    if (how === 'refs') {
      actions.refs(sym.kind, sym.name)
      return
    }
    actions.rename(sym.kind, sym.name)
  })
  return true
}

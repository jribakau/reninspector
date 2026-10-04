import { StateEffect, StateField, type Extension } from '@codemirror/state'
import { Decoration, EditorView, hoverTooltip, type DecorationSet } from '@codemirror/view'
import { api } from '../api'
import type { SpellHit } from '../types'

export const setSpelling = StateEffect.define<SpellHit[]>()

const spellMark = Decoration.mark({ class: 'cm-spell' })

function ranges(state: { doc: { lines: number; line: (n: number) => { from: number; to: number } } }, hits: SpellHit[]): DecorationSet {
  const marks = []
  for (const hit of hits) {
    if (hit.line < 1 || hit.line > state.doc.lines || hit.from >= hit.to) continue
    const line = state.doc.line(hit.line)
    const from = Math.min(line.to, line.from + hit.from)
    const to = Math.min(line.to, line.from + hit.to)
    if (from < to) marks.push(spellMark.range(from, to))
  }
  marks.sort((a, b) => a.from - b.from)
  return Decoration.set(marks, true)
}

const spellHits = StateField.define<SpellHit[]>({
  create: () => [],
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setSpelling)) return effect.value
    if (tr.docChanged) return []
    return value
  },
})

const spellDeco = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setSpelling)) return ranges(tr.state, effect.value)
    }
    if (tr.docChanged) return Decoration.none
    return value
  },
  provide: (field) => EditorView.decorations.from(field),
})

let afterAdd: (() => void) | null = null

/** Called after a word is added, so the open buffer can be checked again. */
export function onDictionaryChange(fn: (() => void) | null) {
  afterAdd = fn
}

function hitAt(hits: SpellHit[], state: { doc: { line: (n: number) => { from: number; to: number } } }, pos: number): { hit: SpellHit; from: number; to: number } | null {
  for (const hit of hits) {
    const line = state.doc.line(hit.line)
    const from = line.from + hit.from
    const to = line.from + hit.to
    if (pos >= from && pos <= to) return { hit, from, to }
  }
  return null
}

const spellHover = hoverTooltip((view, pos) => {
  const found = hitAt(view.state.field(spellHits), view.state, pos)
  if (!found) return null
  const { hit, from, to } = found
  return {
    pos: from,
    end: to,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = 'spell-tip'
      const title = document.createElement('div')
      title.className = 'spell-word'
      title.textContent = hit.word
      dom.append(title)
      const list = document.createElement('div')
      list.className = 'spell-list'
      dom.append(list)
      void api.spellSuggest(hit.word).then((words) => {
        if (!words.length) {
          const none = document.createElement('div')
          none.textContent = 'No suggestions'
          list.append(none)
        }
        for (const word of words) {
          const button = document.createElement('button')
          button.type = 'button'
          button.textContent = word
          button.addEventListener('mousedown', (event) => {
            event.preventDefault()
            view.dispatch({ changes: { from, to, insert: word }, selection: { anchor: from + word.length } })
            view.focus()
          })
          list.append(button)
        }
      }).catch(() => {})
      const add = document.createElement('button')
      add.type = 'button'
      add.className = 'spell-add'
      add.textContent = 'Add to dictionary'
      add.addEventListener('mousedown', (event) => {
        event.preventDefault()
        void api.spellAddWord(hit.word).then(() => afterAdd?.())
      })
      dom.append(add)
      return { dom }
    },
  }
})

export function spellSupport(): Extension {
  return [spellHits, spellDeco, spellHover]
}

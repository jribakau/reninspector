import { Chunk } from '@codemirror/merge'
import { StateEffect, StateField, type Extension, type Text } from '@codemirror/state'
import { GutterMarker, gutter, showTooltip } from '@codemirror/view'
import { revertEdit } from './chunks'
import { tipButton } from './hoverDocs'

export type GitKind = 'added' | 'modified' | 'deleted'

export interface GitMark {
  kind: GitKind
  chunk: number
}

interface GitBars {
  index: Text | null
  chunks: readonly Chunk[]
  marks: Map<number, GitMark>
}

export const setIndex = StateEffect.define<Text | null>()
const emptyBars: GitBars = { index: null, chunks: [], marks: new Map() }

/** One gutter mark per changed line. A deletion sits on the line it was removed from. */
export function buildMarks(doc: Text, chunks: readonly Chunk[]): Map<number, GitMark> {
  const marks = new Map<number, GitMark>()
  chunks.forEach((chunk, index) => {
    if (chunk.fromB === chunk.toB) {
      const pos = chunk.fromB === 0 ? 0 : Math.max(0, Math.min(doc.length, chunk.fromB) - 1)
      const line = doc.lineAt(Math.min(pos, doc.length)).number
      if (!marks.has(line)) marks.set(line, { kind: 'deleted', chunk: index })
      return
    }
    const start = doc.lineAt(Math.min(chunk.fromB, doc.length)).number
    const end = doc.lineAt(Math.min(doc.length, chunk.endB)).number
    const kind: GitKind = chunk.fromA === chunk.toA ? 'added' : 'modified'
    for (let n = start; n <= end; n++) {
      const prev = marks.get(n)
      if (!prev || prev.kind !== 'modified') marks.set(n, { kind, chunk: index })
    }
  })
  return marks
}

const gitBars = StateField.define<GitBars>({
  create: () => emptyBars,
  update(value, tr) {
    let index = value.index
    let next = false
    for (const e of tr.effects) {
      if (!e.is(setIndex)) continue
      index = e.value
      next = true
    }
    if (!index) return emptyBars
    if (!next && !tr.docChanged) return value
    const chunks = next
      ? Chunk.build(index, tr.state.doc, { scanLimit: 500 })
      : Chunk.updateB(value.chunks, index, tr.state.doc, tr.changes, { scanLimit: 500 })
    return { index, chunks, marks: buildMarks(tr.state.doc, chunks) }
  },
})

class ChangeMarker extends GutterMarker {
  kind: GitKind
  constructor(kind: GitKind) {
    super()
    this.kind = kind
  }
  eq(other: ChangeMarker) {
    return other instanceof ChangeMarker && other.kind === this.kind
  }
  toDOM() {
    const el = document.createElement('span')
    el.className = `git-bar ${this.kind}`
    if (this.kind === 'deleted') el.textContent = '▾'
    return el
  }
}

const changeGutter = gutter({
  class: 'cm-change-gutter',
  lineMarker(view, line) {
    const n = view.state.doc.lineAt(line.from).number
    const mark = view.state.field(gitBars).marks.get(n)
    return mark ? new ChangeMarker(mark.kind) : null
  },
  lineMarkerChange: (update) => update.startState.field(gitBars) !== update.state.field(gitBars),
  domEventHandlers: {
    mousedown(view, line, event) {
      const n = view.state.doc.lineAt(line.from).number
      if (!view.state.field(gitBars).marks.has(n)) return false
      event.preventDefault()
      view.dispatch({ effects: setGitTip.of(n) })
      return true
    },
  },
})

const setGitTip = StateEffect.define<number | null>()

/** Gutter, marks, and the revert tooltip. `file` is read when the tooltip opens a diff. */
export function gitChangeExtensions(file: () => string | null, openDiff: (path: string, rev: string) => void): Extension[] {
  const gitTip = StateField.define<number | null>({
    create: () => null,
    update(value, tr) {
      for (const e of tr.effects) if (e.is(setGitTip)) return e.value
      if (tr.docChanged) return null
      return value
    },
    provide: (field) =>
      showTooltip.compute([field, gitBars], (state) => {
        const lineNo = state.field(field)
        if (!lineNo || lineNo < 1 || lineNo > state.doc.lines) return null
        if (!state.field(gitBars).marks.has(lineNo)) return null
        return {
          pos: state.doc.line(lineNo).from,
          above: false,
          create(view) {
            const dom = document.createElement('div')
            dom.className = 'git-tip'
            const bars = view.state.field(gitBars)
            const mark = bars.marks.get(lineNo)
            const chunk = mark ? bars.chunks[mark.chunk] : undefined
            const old = chunk && bars.index ? bars.index.sliceString(chunk.fromA, chunk.endA) : ''
            if (old) {
              const pre = document.createElement('pre')
              pre.textContent = old.length > 2000 ? `${old.slice(0, 2000)}…` : old
              dom.append(pre)
            } else {
              const note = document.createElement('div')
              note.textContent = 'These lines were added.'
              dom.append(note)
            }
            const row = document.createElement('div')
            row.className = 'row'
            const revert = tipButton('Revert change', () => {
              const current = view.state.field(gitBars)
              const hit = current.marks.get(lineNo)
              const piece = hit ? current.chunks[hit.chunk] : undefined
              if (!piece || !current.index) return
              view.dispatch({
                changes: revertEdit(current.index, view.state.doc, piece),
                effects: setGitTip.of(null),
              })
            })
            const open = tipButton('Open changes', () => {
              const path = file()
              if (path) openDiff(`game/${path}`, 'INDEX')
              view.dispatch({ effects: setGitTip.of(null) })
            })
            const close = tipButton('Close', () => view.dispatch({ effects: setGitTip.of(null) }))
            row.append(revert, open, close)
            dom.append(row)
            return { dom }
          },
        }
      }),
  })
  return [changeGutter, gitBars, gitTip]
}

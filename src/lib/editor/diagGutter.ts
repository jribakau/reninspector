import { RangeSetBuilder, StateEffect, StateField, type Extension } from '@codemirror/state'
import { Decoration, EditorView, GutterMarker, gutter, type DecorationSet } from '@codemirror/view'
import type { Diagnostic, Severity } from '../types'

export interface LineMark {
  severity: Severity
  message: string
}

const MAX_RANGE_LINES = 4000

export const setRange = StateEffect.define<{ from: number; to: number } | null>()

export const rangeField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    let next = value.map(tr.changes)
    for (const e of tr.effects) {
      if (!e.is(setRange)) continue
      if (!e.value) {
        next = Decoration.none
        continue
      }
      const doc = tr.state.doc
      const b = new RangeSetBuilder<Decoration>()
      const from = Math.max(1, Math.min(doc.lines, e.value.from))
      const to = Math.min(doc.lines, e.value.to, from + MAX_RANGE_LINES)
      for (let n = from; n <= to; n++) {
        b.add(
          doc.line(n).from,
          doc.line(n).from,
          Decoration.line({ class: n === from ? 'cm-range-start' : 'cm-range' }),
        )
      }
      next = b.finish()
    }
    return next
  },
  provide: (f) => EditorView.decorations.from(f),
})

export const setDiags = StateEffect.define<Map<number, LineMark>>()

const diagField = StateField.define<Map<number, LineMark>>({
  create: () => new Map(),
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setDiags)) return e.value
    return value
  },
})

class DiagMarker extends GutterMarker {
  severity: Severity
  message: string
  constructor(severity: Severity, message: string) {
    super()
    this.severity = severity
    this.message = message
  }
  eq(other: DiagMarker) {
    return other instanceof DiagMarker && other.severity === this.severity && other.message === this.message
  }
  toDOM() {
    const el = document.createElement('span')
    el.className = `diag-dot ${this.severity}`
    el.title = this.message
    return el
  }
}

export const setLive = StateEffect.define<number | null>()
const liveMark = Decoration.line({ class: 'cm-live-line' })

class LiveMarker extends GutterMarker {
  eq(other: LiveMarker) {
    return other instanceof LiveMarker
  }
  toDOM() {
    const el = document.createElement('span')
    el.className = 'live-mark'
    el.title = 'The game is on this line'
    el.textContent = '▶'
    return el
  }
}

const liveField = StateField.define<number | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setLive)) return e.value
    return value
  },
})

const liveDecoField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(_value, tr) {
    let line = tr.startState.field(liveField, false)
    for (const e of tr.effects) if (e.is(setLive)) line = e.value
    if (!line || line < 1 || line > tr.state.doc.lines) return Decoration.none
    const at = tr.state.doc.line(line)
    return Decoration.set([liveMark.range(at.from)])
  },
  provide: (f) => EditorView.decorations.from(f),
})

class LiveSpacer extends GutterMarker {
  toDOM() {
    const el = document.createElement('span')
    el.className = 'live-spacer'
    el.textContent = '▶'
    return el
  }
}

const liveGutter = gutter({
  class: 'cm-live-gutter',
  lineMarker(v, line) {
    const n = v.state.doc.lineAt(line.from).number
    return v.state.field(liveField) === n ? new LiveMarker() : null
  },
  lineMarkerChange: (u) => u.transactions.some((tr) => tr.effects.some((e) => e.is(setLive)) || tr.docChanged),
  initialSpacer: () => new LiveSpacer(),
})

const diagGutter = gutter({
  class: 'cm-diag-gutter',
  lineMarker(v, line) {
    const n = v.state.doc.lineAt(line.from).number
    const d = v.state.field(diagField).get(n)
    return d ? new DiagMarker(d.severity, d.message) : null
  },
  lineMarkerChange: (u) => u.transactions.some((tr) => tr.effects.some((e) => e.is(setDiags))),
  initialSpacer: () => new DiagMarker('info', ''),
})

export const markerExtensions: Extension[] = [liveGutter, liveField, liveDecoField, diagGutter, diagField, rangeField]

export interface PythonMark {
  line: number
  message: string
  severity: 'error' | 'warning' | 'info'
  code: string
}

/** The strongest mark per line: project diagnostics, then syntax, then the language server. */
export function diagnosticMarks(
  diagnostics: Diagnostic[],
  syntax: Map<number, string>,
  python: PythonMark[],
  level: 'syntax' | 'all',
  inPythonLine: (line: number) => boolean,
): Map<number, LineMark> {
  const map = new Map<number, LineMark>()
  const rank = { error: 0, warning: 1, info: 2 } as const
  for (const d of diagnostics) {
    if (d.line < 1 || d.severity === 'info') continue
    const prev = map.get(d.line)
    if (!prev || rank[d.severity] < rank[prev.severity]) {
      map.set(d.line, { severity: d.severity, message: d.message })
    }
  }
  for (const [line, message] of syntax) {
    map.set(line, { severity: 'error', message })
  }
  for (const d of python) {
    if (d.severity === 'info') continue
    if (level === 'syntax' && d.code !== 'invalid-syntax') continue
    if (!inPythonLine(d.line)) continue
    const severity: Severity = d.severity === 'warning' ? 'warning' : 'error'
    const prev = map.get(d.line)
    if (!prev || severity === 'error') map.set(d.line, { severity, message: d.message })
  }
  return map
}

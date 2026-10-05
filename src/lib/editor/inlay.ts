import { RangeSetBuilder, type Extension } from '@codemirror/state'
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet } from '@codemirror/view'

export interface InlayHint {
  at: number
  text: string
}

export interface LabelSite {
  path: string
  line: number
}

const SAY = /^(\s*)(?:([A-Za-z_]\w*)\s+)?("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')\s*(.*)$/
const LABEL = /^(\s*)label\s+[A-Za-z_]\w*\s*:/
const JUMP = /^(\s*)(jump|call)\s+([A-Za-z_][\w.]*)/

/** Words in a say line. Menu choices (`"Yes":`) and other statements count as none. */
export function sayWords(line: string): number {
  const trimmed = line.trim()
  if (!trimmed || trimmed.startsWith('#')) return 0
  const m = SAY.exec(line)
  if (!m || m[4].startsWith(':')) return 0
  if (m[2] && NOT_SPEAKER.has(m[2])) return 0
  return wordCount(m[3].slice(1, -1))
}

export function wordCount(text: string): number {
  const plain = text.replace(/\{\{|\}\}|\{[^}]*\}/g, ' ').replace(/\\(.)/g, '$1')
  return plain.split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w)).length
}

/** Words of dialogue under each `label`, keyed by the label's line number. Nested labels are not counted twice. */
export function labelWordCounts(doc: { lines: number; line: (n: number) => { text: string } }): Map<number, number> {
  const counts = new Map<number, number>()
  if (doc.lines > 30000) return counts // keeps typing responsive in huge files
  const stack: { indent: number; line: number; words: number }[] = []
  const closeTo = (indent: number) => {
    while (stack.length && stack[stack.length - 1].indent >= indent) {
      const done = stack.pop()!
      counts.set(done.line, done.words)
    }
  }
  for (let n = 1; n <= doc.lines; n++) {
    const text = doc.line(n).text
    const label = LABEL.exec(text)
    if (label) {
      closeTo(label[1].length)
      stack.push({ indent: label[1].length, line: n, words: 0 })
      continue
    }
    const trimmed = text.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    // A statement at or left of a label's own column ends that label (e.g. a following `screen`).
    closeTo(text.length - text.trimStart().length)
    const words = sayWords(text)
    if (words && stack.length) stack[stack.length - 1].words += words
  }
  closeTo(-1)
  return counts
}

/** Statements that take a quoted string but are not dialogue. */
const NOT_SPEAKER = new Set([
  'voice', 'text', 'textbutton', 'add', 'key', 'label', 'tooltip', 'action', 'hover',
  'image', 'define', 'default', 'play', 'queue', 'stop', 'scene', 'show', 'hide', 'with',
  'window', 'pause', 'return', 'jump', 'call', 'title', 'alt', 'style', 'use', 'if', 'elif', 'while',
])

function speakerOf(say: RegExpExecArray | null): string | undefined {
  const name = say?.[2]
  return name && !NOT_SPEAKER.has(name) ? name : undefined
}

export function hintsOnLine(
  line: string,
  lineNo: number,
  words: number | undefined,
  displayName: (speaker: string) => string | undefined,
  labelSite: (name: string) => LabelSite | undefined,
): InlayHint[] {
  const label = LABEL.exec(line)
  if (label) {
    if (!words) return []
    const noun = words === 1 ? 'word' : 'words'
    return [{ at: line.length, text: `${words} ${noun}` }]
  }
  const jump = JUMP.exec(line)
  if (jump && !(jump[2] === 'call' && jump[3] === 'screen')) {
    const site = labelSite(jump[3])
    if (!site) return []
    const file = site.path.split('/').pop() || site.path
    return [{ at: jump[1].length + jump[2].length + 1 + jump[3].length, text: `${file}:${site.line}` }]
  }
  const say = SAY.exec(line)
  const speaker = speakerOf(say)
  if (say && speaker && !say[4].startsWith(':')) {
    const name = displayName(speaker)
    if (!name) return []
    return [{ at: say[1].length + say[2].length, text: name }]
  }
  return []
}

class HintWidget extends WidgetType {
  constructor(readonly text: string) {
    super()
  }
  eq(other: HintWidget) {
    return other.text === this.text
  }
  toDOM() {
    const el = document.createElement('span')
    el.className = 'cm-inlay'
    el.textContent = this.text
    return el
  }
  ignoreEvent() {
    return true
  }
}

function build(
  view: EditorView,
  counts: Map<number, number>,
  displayName: (speaker: string) => string | undefined,
  labelSite: (name: string) => LabelSite | undefined,
): DecorationSet {
  const b = new RangeSetBuilder<Decoration>()
  const doc = view.state.doc
  for (const range of view.visibleRanges) {
    let pos = range.from
    while (pos <= range.to) {
      const line = doc.lineAt(pos)
      for (const hint of hintsOnLine(line.text, line.number, counts.get(line.number), displayName, labelSite)) {
        const at = line.from + hint.at
        if (at >= range.from && at <= range.to) {
          b.add(at, at, Decoration.widget({ widget: new HintWidget(hint.text), side: 1 }))
        }
      }
      if (line.to >= range.to) break
      pos = line.to + 1
    }
  }
  return b.finish()
}

/** Grey notes: a character's display name, where a jump goes, and how many words a label has. */
export function inlayHints(
  displayName: (speaker: string) => string | undefined,
  labelSite: (name: string) => LabelSite | undefined,
): Extension {
  return ViewPlugin.fromClass(
    class {
      counts: Map<number, number>
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.counts = labelWordCounts(view.state.doc)
        this.decorations = build(view, this.counts, displayName, labelSite)
      }
      update(update: { docChanged: boolean; viewportChanged: boolean; view: EditorView; state: { doc: { lines: number; line: (n: number) => { text: string } } } }) {
        if (update.docChanged) this.counts = labelWordCounts(update.state.doc)
        if (update.docChanged || update.viewportChanged) {
          this.decorations = build(update.view, this.counts, displayName, labelSite)
        }
      }
    },
    { decorations: (v) => v.decorations },
  )
}

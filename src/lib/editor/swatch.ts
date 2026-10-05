import { RangeSetBuilder, type Extension } from '@codemirror/state'
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet } from '@codemirror/view'

const HEX = /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3})(?![0-9a-fA-F])/g
const IMAGE_PATH = /\.(png|jpe?g|webp|gif)$/i

export interface ColorSpan {
  from: number
  to: number
  hex: string
}

export interface QuotedSpan {
  from: number
  to: number
  text: string
}

/** Colour literals inside quotes on one line. A `#` outside a string starts a comment. */
export function colorSpans(line: string): ColorSpan[] {
  const out: ColorSpan[] = []
  for (const q of stringsIn(line)) {
    HEX.lastIndex = 0
    let m: RegExpExecArray | null
    const slice = line.slice(q.from, q.to)
    while ((m = HEX.exec(slice))) {
      out.push({ from: q.from + m.index, to: q.from + m.index + m[0].length, hex: m[0] })
    }
  }
  return out
}

/** The quoted string containing `index`, quotes not included. */
export function quotedStringAt(line: string, index: number): QuotedSpan | null {
  for (const q of stringsIn(line)) {
    if (index >= q.from && index <= q.to) return q
  }
  return null
}

export function isImagePath(text: string): boolean {
  return IMAGE_PATH.test(text.trim())
}

/** `#rgb` and `#rrggbbaa` become the 6-digit form a colour input accepts. */
export function pickerHex(hex: string): string {
  const body = hex.slice(1)
  if (body.length === 3) return `#${body[0]}${body[0]}${body[1]}${body[1]}${body[2]}${body[2]}`.toLowerCase()
  return `#${body.slice(0, 6)}`.toLowerCase()
}

/** Keeps a trailing alpha byte when the original literal had one. */
export function replaceHex(original: string, picked: string): string {
  const alpha = original.length === 9 ? original.slice(7) : ''
  return `${picked}${alpha}`
}

function stringsIn(line: string): QuotedSpan[] {
  const out: QuotedSpan[] = []
  let i = 0
  while (i < line.length) {
    const c = line[i]
    if (c === '"' || c === "'") {
      const triple = line.startsWith(c + c + c, i)
      i += triple ? 3 : 1
      const start = i
      if (triple) {
        const end = line.indexOf(c + c + c, i)
        const stop = end < 0 ? line.length : end
        out.push({ from: start, to: stop, text: line.slice(start, stop) })
        i = end < 0 ? line.length : end + 3
      } else {
        let j = i
        while (j < line.length) {
          if (line[j] === '\\') {
            j += 2
            continue
          }
          if (line[j] === c) break
          j++
        }
        out.push({ from: start, to: j, text: line.slice(start, j) })
        i = j < line.length ? j + 1 : line.length
      }
      continue
    }
    if (c === '#') break
    i++
  }
  return out
}

class SwatchWidget extends WidgetType {
  constructor(
    readonly hex: string,
    readonly from: number,
    readonly to: number,
  ) {
    super()
  }

  eq(other: SwatchWidget) {
    return other.hex === this.hex && other.from === this.from && other.to === this.to
  }

  toDOM(view: EditorView) {
    const el = document.createElement('span')
    el.className = 'cm-swatch'
    el.title = this.hex
    el.style.background = pickerHex(this.hex)
    const from = this.from
    const to = this.to
    const hex = this.hex
    el.addEventListener('mousedown', (event) => {
      event.preventDefault()
      event.stopPropagation()
      if (view.state.readOnly) return
      openPicker(view, from, to, hex)
    })
    return el
  }

  ignoreEvent() {
    return true
  }
}

// A cancelled picker never fires `change`, so keep one input around and drop the old one.
let activePicker: HTMLInputElement | null = null

function openPicker(view: EditorView, from: number, to: number, hex: string) {
  activePicker?.remove()
  const input = document.createElement('input')
  activePicker = input
  input.type = 'color'
  input.value = pickerHex(hex)
  input.style.position = 'fixed'
  input.style.opacity = '0'
  input.style.pointerEvents = 'none'
  document.body.append(input)
  let end = to
  const apply = () => {
    const current = view.state.doc.sliceString(from, end)
    if (!current.startsWith('#')) return
    const insert = replaceHex(current, input.value)
    if (insert.toLowerCase() === current.toLowerCase()) return
    view.dispatch({ changes: { from, to: end, insert } })
    end = from + insert.length
  }
  input.addEventListener('input', apply)
  input.addEventListener('change', () => input.remove())
  input.click()
}

function build(view: EditorView): DecorationSet {
  const b = new RangeSetBuilder<Decoration>()
  const doc = view.state.doc
  for (const range of view.visibleRanges) {
    let pos = range.from
    while (pos <= range.to) {
      const line = doc.lineAt(pos)
      for (const span of colorSpans(line.text)) {
        const at = line.from + span.to
        if (at >= range.from && at <= range.to) {
          b.add(at, at, Decoration.widget({ widget: new SwatchWidget(span.hex, line.from + span.from, at), side: 1 }))
        }
      }
      if (line.to >= range.to) break
      pos = line.to + 1
    }
  }
  return b.finish()
}

/** A colour chip after `"#rrggbb"` (and the 3- and 8-digit forms) inside strings. */
export function colorSwatches(): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = build(view)
      }
      update(update: { docChanged: boolean; viewportChanged: boolean; view: EditorView }) {
        if (update.docChanged || update.viewportChanged) this.decorations = build(update.view)
      }
    },
    { decorations: (v) => v.decorations },
  )
}

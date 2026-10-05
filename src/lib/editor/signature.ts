import { StateEffect, StateField, type Extension } from '@codemirror/state'
import { keymap, showTooltip, ViewPlugin, type Tooltip, type ViewUpdate } from '@codemirror/view'
import { docNow, loadDocs, type DocEntry } from '../docs/reference'
import { inPython } from './python'
import { pySignature, type PySignature } from '../pylsp.svelte'

export interface CallContext {
  name: string
  /** Zero-based index of the argument the caret is in. */
  activeArg: number
  /** Set when the current argument is `name=`. */
  keyword: string | null
}

const hideSig = StateEffect.define<null>()
const setTySig = StateEffect.define<TySig | null>()

interface TySig extends PySignature {
  pos: number
}

/** Asks ty for a signature while the caret is inside a Python call, then falls back to the bundled docs. */
const tySigFetch = ViewPlugin.fromClass(class {
  timer: ReturnType<typeof setTimeout> | null = null
  update(update: ViewUpdate) {
    if (!update.docChanged && !update.selectionSet && !update.viewportChanged) return
    if (this.timer) clearTimeout(this.timer)
    const view = update.view
    this.timer = setTimeout(() => {
      const head = view.state.selection.main.head
      const line = view.state.doc.lineAt(head)
      const character = head - line.from
      if (!inPython(view.state.doc, line.number) || !callContext(view.state.doc.sliceString(Math.max(0, head - 500), head))) {
        view.dispatch({ effects: setTySig.of(null) })
        return
      }
      void pySignature(line.number - 1, character).then((sig) => {
        if (view.state.selection.main.head !== head) return
        view.dispatch({ effects: setTySig.of(sig ? { ...sig, pos: head } : null) })
      })
    }, 120)
  }
  destroy() {
    if (this.timer) clearTimeout(this.timer)
  }
})

const tySigField = StateField.define<TySig | null>({
  create: () => null,
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setTySig)) return effect.value
    if (tr.docChanged) return null
    return value
  },
})

/** The call the caret is inside, looking only at `before` (the text up to the caret). */
export function callContext(before: string): CallContext | null {
  let depth = 0
  let open = -1
  let commas = 0
  let string: string | null = null
  for (let i = 0; i < before.length; i++) {
    const c = before[i]
    if (string) {
      if (c === '\\') {
        i += 1
        continue
      }
      if (c === string) string = null
      continue
    }
    if (c === '"' || c === "'") {
      string = c
      continue
    }
    if (c === '#') {
      while (i + 1 < before.length && before[i + 1] !== '\n') i += 1
      continue
    }
    if (c === '(') {
      depth += 1
      if (depth === 1) {
        open = i
        commas = 0
      }
      continue
    }
    if (c === ')') {
      depth = Math.max(0, depth - 1)
      if (depth === 0) open = -1
      continue
    }
    if (c === ',' && depth === 1) commas += 1
  }
  if (open < 0 || depth < 1) return null
  const named = /([A-Za-z_][\w.]*)\s*$/.exec(before.slice(0, open))
  if (!named) return null
  const arg = argumentText(before.slice(open + 1))
  const keyword = /^\s*([A-Za-z_]\w*)\s*=/.exec(arg)?.[1] ?? null
  return { name: named[1], activeArg: commas, keyword }
}

function argumentText(inside: string): string {
  let depth = 0
  let string: string | null = null
  let start = 0
  for (let i = 0; i < inside.length; i++) {
    const c = inside[i]
    if (string) {
      if (c === '\\') {
        i += 1
        continue
      }
      if (c === string) string = null
      continue
    }
    if (c === '"' || c === "'") {
      string = c
      continue
    }
    if (c === '(') depth += 1
    else if (c === ')' && depth > 0) depth -= 1
    else if (c === ',' && depth === 0) start = i + 1
  }
  return inside.slice(start)
}

export function activeParam(ctx: CallContext, params: { name: string }[]): number {
  if (ctx.keyword) {
    const at = params.findIndex((p) => p.name === ctx.keyword)
    if (at >= 0) return at
  }
  return Math.min(ctx.activeArg, Math.max(0, params.length - 1))
}

function tyTooltip(pos: number, sig: TySig): Tooltip {
  return {
    pos,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = 'sig-tip'
      const row = document.createElement('div')
      row.className = 'sig-row'
      row.textContent = sig.label
      dom.append(row)
      const name = sig.params[sig.active]
      if (name) {
        const body = document.createElement('div')
        body.className = 'sig-note'
        body.textContent = name
        dom.append(body)
      }
      return { dom }
    },
  }
}

function tooltip(pos: number, entry: DocEntry, active: number): Tooltip {
  return {
    pos,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = 'sig-tip'
      const row = document.createElement('div')
      row.className = 'sig-row'
      row.append(document.createTextNode(`${entry.name}(`))
      entry.params.forEach((param, i) => {
        if (i) row.append(document.createTextNode(', '))
        const el = document.createElement('span')
        el.textContent = param.name
        if (i === active) el.className = 'sig-active'
        row.append(el)
      })
      row.append(document.createTextNode(')'))
      dom.append(row)
      const note = entry.params[active]?.doc || entry.summary
      const body = document.createElement('div')
      body.className = 'sig-note'
      body.textContent = note
      dom.append(body)
      return { dom }
    },
  }
}

/** Signature help after `(` or `,`, hidden by `)` or Escape. */
export function signatureHelp(): Extension {
  const field = StateField.define<boolean>({
    create() {
      void loadDocs()
      return false
    },
    update(value, tr) {
      for (const e of tr.effects) if (e.is(hideSig)) return false
      if (!tr.docChanged) return value
      let inserted = ''
      tr.changes.iterChanges((_from, _to, _fa, _tb, text) => {
        inserted += text
      })
      // Auto-close brackets inserts "()" in one go, which must open the tip, not close it.
      if (inserted.includes('(') || inserted.includes(',')) return true
      if (inserted.includes(')')) return false
      return value
    },
    provide: (f) =>
      showTooltip.compute([f, tySigField, 'doc', 'selection'], (state) => {
        if (!state.field(f)) return null
        const head = state.selection.main.head
        const ty = state.field(tySigField)
        if (ty && ty.pos === head) return tyTooltip(head, ty)
        const from = Math.max(0, head - 500)
        const ctx = callContext(state.doc.sliceString(from, head))
        if (!ctx) return null
        const entry = docNow(ctx.name)
        if (!entry?.params.length) return null
        return tooltip(head, entry, activeParam(ctx, entry.params))
      }),
  })

  return [
    tySigField,
    tySigFetch,
    field,
    keymap.of([
      {
        key: 'Escape',
        run(view) {
          if (!view.state.field(field, false)) return false
          view.dispatch({ effects: hideSig.of(null) })
          return true
        },
      },
    ]),
  ]
}

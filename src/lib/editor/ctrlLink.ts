import { StateEffect, StateField } from '@codemirror/state'
import { Decoration, EditorView, ViewPlugin, showTooltip, type Tooltip } from '@codemirror/view'
import type { RefHit } from '../types'
import { preferHere, symbolHere, wordAt, type WordActions } from './wordNav'

/** How many reference rows the hover table shows before offering the full list. */
export const REF_PREVIEW = 8

export interface CtrlHost {
  actions: WordActions
  references: (kind: string, name: string) => Promise<RefHit[]>
}

/** Variables get a reference table. Everything else with a definition is a jump. */
export function linkMode(kind: string): 'refs' | 'jump' {
  return kind === 'variable' ? 'refs' : 'jump'
}

/** Caption for a ctrl-hover. A `jump` or `call` says where it goes. */
export function linkTitle(kind: string, name: string, prefer: string | null): string {
  if (linkMode(kind) === 'refs') return name
  if (prefer === 'label') return `Jump to ${name}`
  return `Go to ${kind} ${name}`
}

export function previewHits<T>(hits: T[], limit = REF_PREVIEW): { shown: T[]; more: number } {
  return { shown: hits.slice(0, limit), more: Math.max(0, hits.length - limit) }
}

let modifierDown = false

/** True while Control or Command is held, so the ordinary hover can step aside. */
export function ctrlHeld(): boolean {
  return modifierDown
}

interface Link {
  from: number
  to: number
  tooltip: Tooltip
}

const setLink = StateEffect.define<Link | null>()
const linkMark = Decoration.mark({ class: 'cm-ctrl-link' })

const linkField = StateField.define<Link | null>({
  create: () => null,
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setLink)) return effect.value
    if (!value || !tr.docChanged) return value
    const from = tr.changes.mapPos(value.from, 1)
    const to = tr.changes.mapPos(value.to, -1)
    if (from >= to) return null
    return { from, to, tooltip: { ...value.tooltip, pos: from, end: to } }
  },
  provide: (field) => [
    EditorView.decorations.from(field, (link) =>
      link ? Decoration.set(linkMark.range(link.from, link.to)) : Decoration.none,
    ),
    showTooltip.from(field, (link) => link?.tooltip ?? null),
  ],
})

function fileName(path: string): string {
  const parts = path.split(/[/\\]/)
  return parts[parts.length - 1] || path
}

function stopMouse(event: Event) {
  event.preventDefault()
}

/** Underline the word under the pointer while Control is held, and say where it leads. */
export function ctrlLink(host: () => CtrlHost) {
  const plugin = ViewPlugin.define((view) => new CtrlHover(view, host), {
    eventHandlers: {
      mousemove(event, view) {
        this.track(event, view)
      },
      mouseleave() {
        this.leave()
      },
    },
  })
  return [linkField, plugin]
}

class CtrlHover {
  private held = false
  private inside = false
  private overTip = false
  private x = 0
  private y = 0
  private token = 0
  private pendingKey = ''
  private wait = 0
  private linger = 0
  private readonly refs = new Map<string, Promise<RefHit[]>>()

  constructor(
    private readonly view: EditorView,
    private readonly host: () => CtrlHost,
  ) {
    this.onKey = this.onKey.bind(this)
    this.onBlur = this.onBlur.bind(this)
    window.addEventListener('keydown', this.onKey)
    window.addEventListener('keyup', this.onKey)
    window.addEventListener('blur', this.onBlur)
  }

  destroy() {
    window.removeEventListener('keydown', this.onKey)
    window.removeEventListener('keyup', this.onKey)
    window.removeEventListener('blur', this.onBlur)
    clearTimeout(this.wait)
    clearTimeout(this.linger)
    modifierDown = false
    this.view.dom.classList.remove('cm-ctrl-held')
  }

  private onBlur() {
    this.held = false
    modifierDown = false
    this.view.dom.classList.remove('cm-ctrl-held')
    this.clearLink()
  }

  private onKey(event: KeyboardEvent) {
    const next = event.ctrlKey || event.metaKey
    if (next === this.held) return
    this.held = next
    modifierDown = next
    this.view.dom.classList.toggle('cm-ctrl-held', next)
    if (!next) {
      this.pendingKey = ''
      this.refs.clear()
      this.clearLink()
      return
    }
    if (this.inside) this.arm()
  }

  track(event: MouseEvent, view: EditorView) {
    this.inside = true
    this.x = event.clientX
    this.y = event.clientY
    clearTimeout(this.linger)
    const next = event.ctrlKey || event.metaKey
    if (next !== this.held) {
      this.held = next
      modifierDown = next
      view.dom.classList.toggle('cm-ctrl-held', next)
      if (!next) {
        this.pendingKey = ''
        this.clearLink()
        return
      }
    }
    if (!this.held) return
    const pos = view.posAtCoords({ x: event.clientX, y: event.clientY })
    const found = pos == null ? null : wordAt(view.state, pos)
    const key = found ? `${found.from}:${found.to}` : ''
    if (key === this.pendingKey) return
    this.pendingKey = key
    if (!found) {
      this.clearLink()
      return
    }
    this.arm()
  }

  leave() {
    this.inside = false
    clearTimeout(this.linger)
    this.linger = window.setTimeout(() => {
      if (!this.overTip) {
        this.pendingKey = ''
        this.clearLink()
      }
    }, 200)
  }

  private arm() {
    clearTimeout(this.wait)
    this.wait = window.setTimeout(() => void this.lookup(), 40)
  }

  private clearLink() {
    this.token += 1
    clearTimeout(this.wait)
    if (this.view.state.field(linkField)) this.view.dispatch({ effects: setLink.of(null) })
  }

  private async lookup() {
    if (!this.held) return
    const token = ++this.token
    const pos = this.view.posAtCoords({ x: this.x, y: this.y })
    if (pos == null) return
    const found = wordAt(this.view.state, pos)
    if (!found) return
    const line = this.view.state.doc.lineAt(found.from)
    const prefer = preferHere(this.view.state, line.number, line.text, found.from - line.from, found.text)
    const actions = this.host().actions
    const sym = await symbolHere(line.number, found.text, prefer, actions.lookup)
    if (token !== this.token || !this.held) return
    if (sym?.path && linkMode(sym.kind) === 'refs') {
      this.show(found.from, found.to, this.jumpTip(sym.name, 'References…'))
      const hits = await this.referenceHits(sym.kind, sym.name)
      if (token !== this.token || !this.held) return
      this.show(found.from, found.to, this.refsTip(sym.kind, sym.name, hits, actions), true)
      return
    }
    if (sym?.path) {
      this.show(found.from, found.to, this.jumpTip(linkTitle(sym.kind, sym.name, prefer), `${fileName(sym.path)}:${sym.line}`))
      return
    }
    if (!actions.inPythonLine(this.view.state.doc, line.number)) {
      this.clearLink()
      return
    }
    const hit = await actions.pythonDefinition(line.number - 1, found.from - line.from)
    if (token !== this.token || !this.held) return
    if (!hit || hit === 'external') {
      this.clearLink()
      return
    }
    this.show(found.from, found.to, this.jumpTip('Go to definition', `${fileName(hit.path)}:${hit.line}`))
  }

  private referenceHits(kind: string, name: string): Promise<RefHit[]> {
    const key = `${kind}:${name}`
    const cached = this.refs.get(key)
    if (cached) return cached
    const pending = this.host()
      .references(kind, name)
      .catch(() => [] as RefHit[])
    this.refs.set(key, pending)
    return pending
  }

  private show(from: number, to: number, tooltip: Omit<Tooltip, 'pos' | 'end'>, replace = false) {
    const current = this.view.state.field(linkField)
    if (!replace && current && current.from === from && current.to === to) return
    this.view.dispatch({
      effects: setLink.of({
        from,
        to,
        tooltip: { ...tooltip, pos: from, end: to, above: false, strictSide: true },
      }),
    })
  }

  private jumpTip(title: string, where: string): Omit<Tooltip, 'pos' | 'end'> {
    return {
      create: () => {
        const dom = document.createElement('div')
        dom.className = 'cm-ctrl-tip'
        const head = document.createElement('div')
        head.className = 'head'
        head.textContent = title
        const meta = document.createElement('div')
        meta.className = 'where'
        meta.textContent = where
        dom.append(head, meta)
        this.watchTip(dom)
        return { dom }
      },
    }
  }

  private refsTip(kind: string, name: string, hits: RefHit[], actions: WordActions): Omit<Tooltip, 'pos' | 'end'> {
    const { shown, more } = previewHits(hits)
    return {
      create: () => {
        const dom = document.createElement('div')
        dom.className = 'cm-ctrl-tip'
        const head = document.createElement('div')
        head.className = 'head'
        head.textContent = hits.length === 1 ? `${name} · 1 reference` : `${name} · ${hits.length} references`
        dom.append(head)
        if (!shown.length) {
          const empty = document.createElement('div')
          empty.className = 'where'
          empty.textContent = 'No references'
          dom.append(empty)
        }
        for (const hit of shown) {
          const row = document.createElement('button')
          row.type = 'button'
          row.className = 'hit'
          const text = document.createElement('span')
          text.className = 'line'
          text.textContent = hit.text || ' '
          const where = document.createElement('span')
          where.className = 'where'
          where.textContent = `${fileName(hit.path)}:${hit.line}`
          row.append(text, where)
          row.addEventListener('mousedown', stopMouse)
          row.addEventListener('click', () => actions.goto(hit.path, hit.line))
          dom.append(row)
        }
        if (more > 0) {
          const all = document.createElement('button')
          all.type = 'button'
          all.className = 'more'
          all.textContent = `Show all ${hits.length} references`
          all.addEventListener('mousedown', stopMouse)
          all.addEventListener('click', () => actions.refs(kind, name))
          dom.append(all)
        }
        this.watchTip(dom)
        return { dom }
      },
    }
  }

  /** Moving onto the table should not dismiss it before a row can be clicked. */
  private watchTip(dom: HTMLElement) {
    dom.addEventListener('mouseenter', () => {
      this.overTip = true
      clearTimeout(this.linger)
    })
    dom.addEventListener('mouseleave', () => {
      this.overTip = false
      this.leave()
    })
  }
}

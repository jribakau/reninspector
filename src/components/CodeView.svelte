<script lang="ts">
  import { onMount } from 'svelte'
  import { acceptCompletion, autocompletion, closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete'
import { copyLineDown, copyLineUp, defaultKeymap, history, historyKeymap, indentLess, indentMore, moveLineDown, moveLineUp, redo, selectAll, toggleComment, undo } from '@codemirror/commands'
import { bracketMatching, codeFolding, foldCode, foldGutter, foldKeymap, foldService, indentService, indentUnit, unfoldCode } from '@codemirror/language'
  import { Chunk } from '@codemirror/merge'
  import { highlightSelectionMatches, openSearchPanel, search, searchKeymap, selectNextOccurrence } from '@codemirror/search'
  import { Compartment, EditorSelection, EditorState, RangeSetBuilder, StateEffect, StateField, Text } from '@codemirror/state'
  import {
    crosshairCursor,
    Decoration,
    drawSelection,
    EditorView,
    GutterMarker,
    gutter,
    highlightActiveLine,
    hoverTooltip,
    keymap,
    lineNumbers,
    showTooltip,
    rectangularSelection,
    type DecorationSet,
  } from '@codemirror/view'
  import { api, readAsset, readFileText, readPreview, errorText } from '../lib/api'
  import { askText } from '../lib/dialog.svelte'
  import { keymapExtension, keymapNow, setKeymapSave } from '../lib/editor/keymaps'
import { colorSwatches, isImagePath, quotedStringAt } from '../lib/editor/swatch'
import { editorTheme } from '../lib/editor/theme'
  import { appearance } from '../lib/project.svelte'
  import { settings } from '../lib/settings.svelte'
  import { renpyHighlight, renpyLanguage } from '../lib/renpyLang'
  import { revertEdit } from '../lib/editor/chunks'
  import { renpyComplete } from '../lib/editor/complete'
  import { onDictionaryChange, setSpelling, spellSupport } from '../lib/editor/spell'
  import { git } from '../lib/git.svelte'
  import { docNow, loadDocs, type DocEntry } from '../lib/docs/reference'
import { formatEdits } from '../lib/editor/format'
import { signatureHelp } from '../lib/editor/signature'
import { inlayHints } from '../lib/editor/inlay'
import { inPython } from '../lib/editor/python'
import { py, pyDefinition, pyDiags, pyHover, pythonDiagnostics, setPyFile, syncDocument } from '../lib/pylsp.svelte'
import { app, fileInfo, fileOfNode, lookupSymbol, nodeByName, openDiff, registerBufferSave, registerEditor, registerSave, symbolsOf, type Loc } from '../lib/store.svelte'
  import type { Diagnostic, Severity, Symbol } from '../lib/types'

  interface Props {
    loc: Loc | null
    diagnostics: Diagnostic[]
    changeSeq: number
    changedPaths: string[]
    editing: boolean
    dirty: boolean
    modified: boolean
    reloadFile: { path: string; seq: number } | null
    oncursor: (file: string, line: number) => void
    ondirty: (file: string, dirty: boolean) => void
    onsave: (file: string, text: string) => Promise<boolean>
    onrevert: (file: string) => void
    onstale: (file: string) => void
    ongoto: (file: string, line: number) => void
    onrefs: (kind: string, name: string) => void
    onrename: (kind: string, name: string) => void
    /** Line the running game is on, when this file is that file. */
    liveLine: number | null
    hint?: string
  }

  let {
    loc,
    diagnostics,
    changeSeq,
    changedPaths,
    editing,
    dirty,
    modified,
    reloadFile,
    oncursor,
    ondirty,
    onsave,
    onrevert,
    onstale,
    ongoto,
    onrefs,
    onrename,
    liveLine,
    hint = '',
  }: Props = $props()

  let host: HTMLDivElement
  let view: EditorView | null = null
  let editorReady = $state(0)
  let loadedFile = $state<string | null>(null)
  let loading = $state(false)
  let errorMsg = $state('')
  let lineCount = $state(0)
  let reqToken = 0
  let lastLoc: Loc | null = null
  let appliedSeq = -1
  let seenReload = -1

  /** Unsaved buffers, so switching labels does not throw away edits. */
  const buffers = new Map<string, EditorState>()
  /** Last document loaded from disk or successfully saved. */
  const saved = new Map<string, Text>()
  let syntax = new Map<number, string>()
  let syntaxTimer: ReturnType<typeof setTimeout> | null = null
  let dirtyTimer: ReturnType<typeof setTimeout> | null = null
  let reportedFile = ''
  let reportedLine = -1

  // Past this size, building or comparing the whole document waits until typing pauses.
  const DIRTY_FAST_LIMIT = 200_000

  // Live bindings for the keymap, which is created once per buffer.
  const ctx = {
    save: () => {
      void save()
    },
    goto: (view: EditorView) => followWord(view, 'goto'),
    refs: (view: EditorView) => followWord(view, 'refs'),
    rename: (view: EditorView) => followWord(view, 'rename'),
  }

  function wordAt(state: EditorState, pos: number): { from: number; to: number; text: string } | null {
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

  function preferredKind(line: string, wordStart: number, word: string): string | null {
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

  function preferHere(state: EditorState, lineNo: number, line: string, wordStart: number, word: string): string | null {
    const base = preferredKind(line, wordStart, word)
    if (base) return base
    const after = line.slice(wordStart + word.length)
    if (inPython(state.doc, lineNo) && after.trimStart().startsWith('(')) return 'function'
    return null
  }

  async function symbolHere(lineNo: number, word: string, prefer: string | null): Promise<Symbol | undefined> {
    const path = loadedFile
    if (path) {
      try {
        const found = await api.resolveSymbol(path, lineNo, word, prefer)
        if (found.symbol) return found.symbol
        if (found.hit) return undefined
      } catch {
        // The name table still answers when the project command fails.
      }
    }
    if (prefer === 'function' || prefer === 'class') {
      return symbolsOf(prefer).find((s) => s.name === word)
    }
    return lookupSymbol(word)
  }

  function followWord(view: EditorView, how: 'goto' | 'refs' | 'rename'): boolean {
    const found = wordAt(view.state, view.state.selection.main.head)
    if (!found) return false
    const line = view.state.doc.lineAt(found.from)
    const prefer = preferHere(view.state, line.number, line.text, found.from - line.from, found.text)
    void symbolHere(line.number, found.text, prefer).then(async (sym) => {
      if (!sym) {
        if (how !== 'goto' || !inPython(view.state.doc, line.number)) return
        const hit = await pyDefinition(line.number - 1, found.from - line.from)
        if (hit === 'external') app.notice = 'That definition is outside this project, so it was not opened.'
        else if (hit) ongoto(hit.path, hit.line)
        return
      }
      if (how === 'goto') {
        if (sym.path) ongoto(sym.path, sym.line)
        return
      }
      // `style_prefix` derives names like `name_text`, which a rename cannot follow.
      if (sym.kind === 'style' && how === 'rename') {
        app.notice = 'Styles can be found but not renamed, because a style prefix derives other names.'
        return
      }
      if (how === 'refs') {
        onrefs(sym.kind, sym.name)
        return
      }
      onrename(sym.kind, sym.name)
    })
    return true
  }

  type GitKind = 'added' | 'modified' | 'deleted'
  interface GitMark {
    kind: GitKind
    chunk: number
  }
  interface GitBars {
    index: Text | null
    chunks: readonly Chunk[]
    marks: Map<number, GitMark>
  }

  const setIndex = StateEffect.define<Text | null>()
  const emptyBars: GitBars = { index: null, chunks: [], marks: new Map() }

  function buildMarks(doc: Text, chunks: readonly Chunk[]): Map<number, GitMark> {
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
              if (loadedFile) openDiff(`game/${loadedFile}`, 'INDEX')
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

  function tipButton(label: string, run: () => void): HTMLButtonElement {
    const button = document.createElement('button')
    button.type = 'button'
    button.textContent = label
    button.addEventListener('mousedown', (event) => event.preventDefault())
    button.addEventListener('click', run)
    return button
  }

  function renpyFold(state: EditorState, lineStart: number) {
    const line = state.doc.lineAt(lineStart)
    const header = line.text.trim()
    if (!header.endsWith(':') || header.startsWith('#')) return null
    const indent = line.text.match(/^ */)?.[0].length ?? 0
    let end = line.to
    for (let n = line.number + 1; n <= state.doc.lines; n++) {
      const next = state.doc.line(n)
      const text = next.text.trim()
      // A comment's indent does not end a Ren'Py block. Include it only when
      // a later statement is still inside the block.
      if (!text || text.startsWith('#')) continue
      const ind = next.text.match(/^ */)?.[0].length ?? 0
      if (ind <= indent) break
      end = next.to
    }
    if (end <= line.to) return null
    return { from: line.to, to: end }
  }

  const readOnlyComp = new Compartment()
  const themeComp = new Compartment()
  const wrapComp = new Compartment()
  const spellComp = new Compartment()
  const gutterComp = new Compartment()
  const foldComp = new Compartment()
  const activeLineComp = new Compartment()
  const bracketComp = new Compartment()
  const closeComp = new Compartment()
  const completeComp = new Compartment()
  const indentComp = new Compartment()
  const keymapComp = new Compartment()
  const swatchComp = new Compartment()
  const inlayComp = new Compartment()

  function closeExt() {
    return settings.closeBrackets ? [closeBrackets(), keymap.of(closeBracketsKeymap)] : []
  }
  function completeExt() {
    return settings.autocomplete ? autocompletion({ override: [renpyComplete] }) : []
  }
  function indentExt() {
    return indentUnit.of(' '.repeat(settings.indentWidth))
  }
  function swatchExt() {
    return settings.colorSwatches ? colorSwatches() : []
  }
  function inlayExt() {
    if (!settings.inlayHints) return []
    return inlayHints(
      (name) => {
        const sym = lookupSymbol(name)
        if (!sym || sym.kind !== 'character') return undefined
        const who = sym.detail.split(' · ')[0]?.trim()
        return who && who !== name ? who : undefined
      },
      (name) => {
        const node = nodeByName(name)
        if (!node || node.kind === 'missing' || node.kind === 'compiled') return undefined
        const path = fileOfNode(node)
        return path ? { path, line: node.line } : undefined
      },
    )
  }

  /** Ren'Py rejects tab characters, so Tab inserts spaces. A selection indents instead. */
  function spacesTab(target: EditorView): boolean {
    if (target.state.readOnly) return false
    if (target.state.selection.ranges.some((r) => !r.empty)) return indentMore(target)
    target.dispatch(
      target.state.changeByRange((range) => {
        const col = range.from - target.state.doc.lineAt(range.from).from
        const width = settings.indentWidth
        const insert = ' '.repeat(width - (col % width))
        return {
          changes: { from: range.from, insert },
          range: EditorSelection.cursor(range.from + insert.length),
        }
      }),
    )
    return true
  }

  // ---- highlighted range -------------------------------------------------
  const setRange = StateEffect.define<{ from: number; to: number } | null>()
  const MAX_RANGE_LINES = 4000

  const rangeField = StateField.define<DecorationSet>({
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

  // ---- diagnostic gutter -------------------------------------------------
  const setDiags = StateEffect.define<Map<number, { severity: Severity; message: string }>>()
  const diagField = StateField.define<Map<number, { severity: Severity; message: string }>>({
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
      return other.severity === this.severity && other.message === this.message
    }
    toDOM() {
      const el = document.createElement('span')
      el.className = `diag-dot ${this.severity}`
      el.title = this.message
      return el
    }
  }

  const setLive = StateEffect.define<number | null>()
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

  function openReplace(target: EditorView) {
    openSearchPanel(target)
    requestAnimationFrame(() => {
      const inputs = target.dom.querySelectorAll<HTMLInputElement>('.cm-panel.cm-search input')
      inputs[1]?.focus()
    })
  }

  function jumpToLine(target: EditorView) {
    const current = target.state.doc.lineAt(target.state.selection.main.head).number
    void askText('Go to line', String(current), 'Go').then((value) => {
      if (!value || view !== target) return
      const n = Math.floor(Number(value))
      if (!Number.isFinite(n) || n < 1) return
      const line = target.state.doc.line(Math.min(target.state.doc.lines, n))
      target.dispatch({ selection: { anchor: line.from }, scrollIntoView: true })
      target.focus()
    })
  }

  function renpyIndent(context: { state: EditorState }, pos: number): number {
    const doc = context.state.doc
    const line = doc.lineAt(pos)
    if (line.number <= 1) return 0
    const prev = doc.line(line.number - 1).text
    const base = prev.match(/^ */)?.[0].length ?? 0
    return prev.trimEnd().endsWith(':') ? base + settings.indentWidth : base
  }

  function highlighted(file: string): boolean {
    const ext = file.split('.').pop()?.toLowerCase() ?? ''
    return ext === 'rpy' || ext === 'rpym' || ext === 'py'
  }

  function makeState(doc: string, writable: boolean, script = false): EditorState {
    return EditorState.create({
      doc,
      extensions: [
        keymapComp.of(keymapNow(settings.keymap)),
        changeGutter,
        gutterComp.of(settings.lineNumbers ? lineNumbers() : []),
        drawSelection(),
        EditorState.allowMultipleSelections.of(true),
        rectangularSelection(),
        crosshairCursor(),
        highlightSelectionMatches(),
        closeComp.of(closeExt()),
        liveGutter,
        liveField,
        liveDecoField,
        diagGutter,
        diagField,
        rangeField,
        activeLineComp.of(settings.activeLine ? highlightActiveLine() : []),
        history(),
        indentComp.of(indentExt()),
        indentService.of(renpyIndent),
        signatureHelp(),
        keymap.of([
          // Accept the open suggestion first. Otherwise Tab inserts spaces.
          { key: 'Tab', run: acceptCompletion },
          { key: 'Tab', run: spacesTab },
          { key: 'Shift-Tab', run: indentLess },
          { key: 'Mod-s', run: () => { ctx.save(); return true } },
          { key: 'F12', run: (v) => ctx.goto(v) },
          { key: 'Shift-F12', run: (v) => ctx.refs(v) },
          { key: 'F2', run: (v) => ctx.rename(v) },
          { key: 'Mod-g', run: (v) => { jumpToLine(v); return true } },
          { key: 'Mod-h', run: (v) => { openReplace(v); return true } },
          { key: 'Mod-d', run: selectNextOccurrence, preventDefault: true },
          { key: 'Alt-ArrowUp', run: moveLineUp, shift: copyLineUp },
          { key: 'Alt-ArrowDown', run: moveLineDown, shift: copyLineDown },
          { key: 'Shift-Alt-f', run: (v) => { formatView(v, null); return true } },
          ...foldKeymap,
          ...searchKeymap.filter((binding) => binding.key !== 'Mod-g' && binding.key !== 'Mod-G' && binding.key !== 'Mod-d'),
          ...historyKeymap,
          ...defaultKeymap,
        ]),
        search({ top: true }),
        completeComp.of(completeExt()),
        bracketComp.of(settings.bracketMatching ? bracketMatching() : []),
        codeFolding(),
        foldComp.of(settings.foldGutter ? foldGutter() : []),
        foldService.of(renpyFold),
        swatchComp.of(swatchExt()),
        inlayComp.of(inlayExt()),
        gitBars,
        gitTip,
        hoverTooltip(async (view, pos) => {
          await loadDocs()
          const found = wordAt(view.state, pos)
          const line = view.state.doc.lineAt(pos)
          if (found) {
            const prefer = preferHere(view.state, line.number, line.text, found.from - line.from, found.text)
            const sym = await symbolHere(line.number, found.text, prefer)
            if (sym) {
              const node = sym.kind === 'label' || sym.kind === 'screen' ? nodeByName(sym.kind === 'screen' ? `screen:${sym.name}` : sym.name) : undefined
              return {
                pos: found.from,
                end: found.to,
                above: true,
                create() {
                  const dom = document.createElement('div')
                  dom.className = 'sym-tip'
                  const title = document.createElement('div')
                  title.className = 'sym-title'
                  title.textContent = `${sym.kind} ${sym.name}`
                  dom.append(title)
                  const meta = document.createElement('div')
                  const where = sym.path ? `${sym.path}:${sym.line}` : 'built in'
                  const extra = [sym.detail, where, node ? `${node.inDegree} in` : ''].filter(Boolean).join(' · ')
                  meta.textContent = extra
                  dom.append(meta)
                  if (sym.kind === 'image' && /\.(png|jpe?g|webp|gif)$/i.test(sym.detail)) {
                    const img = document.createElement('img')
                    img.alt = ''
                    dom.append(img)
                    void readAsset(sym.detail).then((buf) => {
                      img.src = URL.createObjectURL(new Blob([buf]))
                    }).catch(() => {})
                  }
                  return { dom }
                },
              }
            }
            // Docs are for code, not dialogue: skip words inside a string, and only let
            // statement names match when they open the line.
            const before = line.text.slice(0, found.from - line.from)
            const inString = ((before.replace(/\\./g, '').match(/["']/g) ?? []).length & 1) === 1
            if (!inString && inPython(view.state.doc, line.number)) {
              const tip = await pyHover(line.number - 1, found.from - line.from)
              if (tip) return docText(tip, found.from, found.to)
            }
            const doc = inString ? undefined : docNow(found.text)
            if (doc && (doc.kind !== 'statement' || /^\s*(\$\s*)?$/.test(before))) {
              return docTip(doc, found.from, found.to)
            }
          }
          const quoted = quotedStringAt(line.text, pos - line.from)
          const path = quoted?.text.trim() ?? ''
          if (!quoted || !isImagePath(path)) return null
          return {
            pos: line.from + quoted.from,
            end: line.from + quoted.to,
            above: true,
            create() {
              const dom = document.createElement('div')
              dom.className = 'sym-tip'
              const title = document.createElement('div')
              title.className = 'sym-title'
              title.textContent = path
              dom.append(title)
              const img = document.createElement('img')
              img.alt = ''
              dom.append(img)
              void readAsset(path).then((buf) => {
                img.src = URL.createObjectURL(new Blob([buf]))
              }).catch(() => {})
              return { dom }
            },
          }
        }),
        EditorView.domEventHandlers({
          click(event, view) {
            if (!event.ctrlKey && !event.metaKey) return false
            const pos = view.posAtCoords({ x: event.clientX, y: event.clientY })
            if (pos == null) return false
            const found = wordAt(view.state, pos)
            if (!found) return false
            const line = view.state.doc.lineAt(found.from)
            const prefer = preferHere(view.state, line.number, line.text, found.from - line.from, found.text)
            void symbolHere(line.number, found.text, prefer).then((sym) => {
              if (sym?.path) ongoto(sym.path, sym.line)
            })
            return true
          },
        }),
        ...(script ? [renpyLanguage, renpyHighlight] : []),
        themeComp.of(editorTheme(!appearance.light)),
        wrapComp.of(appearance.wrap ? EditorView.lineWrapping : []),
        spellComp.of(appearance.spell ? spellSupport() : []),
        readOnlyComp.of(EditorState.readOnly.of(!writable)),
        EditorView.updateListener.of((u) => {
          if (!loadedFile) return
          if (u.docChanged) {
            buffers.set(loadedFile, u.state)
            markDirty(loadedFile, u.state.doc)
            if (u.transactions.some((tr) => tr.isUserEvent('input') || tr.isUserEvent('delete') || tr.isUserEvent('undo') || tr.isUserEvent('redo'))) {
              app.followHold = true
            }
            scheduleSyntax()
            syncDocument(loadedFile, u.state.doc.toString())
            scheduleSpell()
          }
          if (!u.selectionSet) return
          if (!u.transactions.some((tr) => tr.isUserEvent('select'))) return
          const line = u.state.doc.lineAt(u.state.selection.main.head).number
          if (loadedFile === reportedFile && line === reportedLine) return
          reportedFile = loadedFile
          reportedLine = line
          oncursor(loadedFile, line)
        }),
      ],
    })
  }

  onMount(() => {
    view = new EditorView({ parent: host, state: makeState('', false) })
    editorReady = 1
    onDictionaryChange(() => scheduleSpell())
    registerSave(() => ctx.save())
    registerBufferSave((path) => savePath(path))
    registerEditor({
      undo: () => { if (view) undo(view) },
      redo: () => { if (view) redo(view) },
      find: () => { if (view) openSearchPanel(view) },
      replace: () => { if (view) openReplace(view) },
      gotoLine: () => { if (view) jumpToLine(view) },
      selectAll: () => { if (view) selectAll(view) },
      goto: () => { if (view) ctx.goto(view) },
      refs: () => { if (view) ctx.refs(view) },
      rename: () => { if (view) ctx.rename(view) },
      toggleComment: () => { if (view) toggleComment(view) },
      selectNext: () => { if (view) selectNextOccurrence(view) },
      moveLineUp: () => { if (view) moveLineUp(view) },
      moveLineDown: () => { if (view) moveLineDown(view) },
      fold: () => { if (view) foldCode(view) },
      unfold: () => { if (view) unfoldCode(view) },
      format: () => { if (view) formatView(view, null) },
      formatSelection: () => { if (view) formatView(view, view.state.selection.main) },
    })
    return () => {
      onDictionaryChange(null)
      registerSave(null)
      registerBufferSave(null)
      registerEditor(null)
      view?.destroy()
      view = null
    }
  })

  function remember() {
    if (loadedFile && view) buffers.set(loadedFile, view.state)
  }

  function decodePreview(buf: ArrayBuffer): string {
    const text = new TextDecoder('utf-8').decode(buf)
    return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text
  }

  async function load(file: string): Promise<boolean> {
    const token = ++reqToken
    loading = true
    errorMsg = ''
    try {
      const indexed = !!fileInfo(file)
      const raw = indexed ? await readFileText(file) : decodePreview(await readPreview(file))
      if (token !== reqToken || !view) return false
      const state = makeState(raw, indexed && editing, highlighted(file))
      view.setState(state)
      buffers.set(file, state)
      saved.set(file, state.doc)
      reportedFile = file
      reportedLine = -1
      lineCount = view.state.doc.lines
      loadedFile = file
      ondirty(file, false)
      syntax = new Map()
      applyDiagnostics()
      scheduleSpell()
      return true
    } catch (e) {
      if (token === reqToken) errorMsg = errorText(e)
      return false
    } finally {
      if (token === reqToken) loading = false
    }
  }

  function showBuffer(file: string) {
    const cached = buffers.get(file)
    if (!cached || !view) return false
    view.setState(cached)
    view.dispatch({ effects: readOnlyComp.reconfigure(EditorState.readOnly.of(!(editing && fileInfo(file)))) })
    loadedFile = file
    reportedFile = file
    reportedLine = -1
    lineCount = view.state.doc.lines
    applyDiagnostics()
    return true
  }

  function blank() {
    if (!view || !loadedFile) return
    remember()
    reqToken += 1
    loadedFile = null
    lastLoc = null
    lineCount = 0
    errorMsg = ''
    syntax = new Map()
    view.setState(makeState('', false))
  }

  function reveal(l: Loc) {
    if (!view) return
    const doc = view.state.doc
    const line = Math.max(1, Math.min(doc.lines, l.line))
    const pos = doc.line(line).from
    view.dispatch({
      selection: { anchor: pos },
      effects: [
        setRange.of({ from: line, to: Math.max(line, l.endLine) }),
        EditorView.scrollIntoView(pos, { y: 'start', yMargin: 90 }),
      ],
    })
  }

  function docText(text: string, from: number, to: number) {
    return {
      pos: from,
      end: to,
      above: true,
      create() {
        const dom = document.createElement('div')
        dom.className = 'sym-tip'
        const body = document.createElement('div')
        body.textContent = text
        dom.append(body)
        return { dom }
      },
    }
  }

  function docTip(doc: DocEntry, from: number, to: number) {
    return {
      pos: from,
      end: to,
      above: true,
      create() {
        const dom = document.createElement('div')
        dom.className = 'sym-tip'
        const title = document.createElement('div')
        title.className = 'sym-title'
        title.textContent = doc.signature
        dom.append(title)
        const summary = document.createElement('div')
        summary.textContent = doc.summary
        dom.append(summary)
        const link = document.createElement('a')
        link.href = doc.url
        link.textContent = 'Documentation'
        link.target = '_blank'
        link.rel = 'noreferrer'
        dom.append(link)
        return { dom }
      },
    }
  }

  function formatView(target: EditorView, range: { from: number; to: number } | null) {
    if (target.state.readOnly) return
    const doc = target.state.doc
    const end = range && range.to > range.from ? range.to - 1 : range?.to
    const lines = range && end != null
      ? { fromLine: doc.lineAt(range.from).number, toLine: doc.lineAt(end).number }
      : undefined
    const edits = formatEdits(doc.toString(), { indent: settings.indentWidth, quotes: settings.preferredQuote }, lines)
    if (!edits.length) return
    target.dispatch({ changes: edits, userEvent: 'format' })
  }

  async function save() {
    if (!loadedFile) return
    await savePath(loadedFile)
  }

  async function savePath(path: string): Promise<boolean> {
    if (!editing || !fileInfo(path)) return false
    let state = loadedFile === path && view ? view.state : buffers.get(path)
    if (!state) return false
    if (settings.formatOnSave && /\.rpym?$/i.test(path) && !state.readOnly) {
      const edits = formatEdits(state.doc.toString(), { indent: settings.indentWidth, quotes: settings.preferredQuote })
      if (edits.length) {
        if (view && loadedFile === path && view.state === state) {
          view.dispatch({ changes: edits, userEvent: 'format' })
          state = view.state
        } else {
          state = state.update({ changes: edits, userEvent: 'format' }).state
          buffers.set(path, state)
        }
      }
    }
    const doc = state.doc
    const prev = saved.get(path)
    if (prev && doc.eq(prev)) {
      ondirty(path, false)
      return true
    }
    const ok = await onsave(path, doc.toString())
    if (ok) {
      saved.set(path, doc)
      ondirty(path, false)
    }
    return ok
  }

  function markDirty(file: string, doc: Text) {
    const prev = saved.get(file)
    if (!prev || doc.length !== prev.length) {
      ondirty(file, true)
      return
    }
    if (doc.length > DIRTY_FAST_LIMIT) {
      ondirty(file, true)
      if (dirtyTimer) clearTimeout(dirtyTimer)
      dirtyTimer = setTimeout(() => {
        if (!view || loadedFile !== file) return
        const baseline = saved.get(file)
        const current = view.state.doc
        if (!baseline) return
        ondirty(file, current.length !== baseline.length || !current.eq(baseline))
      }, 400)
      return
    }
    ondirty(file, !doc.eq(prev))
  }

  // Files of a few hundred thousand lines are checked on save, not per keystroke.
  const SYNTAX_LIMIT = 500_000

  let spellTimer: ReturnType<typeof setTimeout> | null = null

  function scheduleSpell() {
    if (!appearance.spell || !view || !loadedFile || !highlighted(loadedFile)) return
    if (view.state.doc.length > SYNTAX_LIMIT) return
    const file = loadedFile
    if (spellTimer) clearTimeout(spellTimer)
    spellTimer = setTimeout(() => {
      if (!view || loadedFile !== file || !appearance.spell) return
      const text = view.state.doc.toString()
      if (text.length > SYNTAX_LIMIT) return
      void api.spellCheck(text).then((hits) => {
        if (!view || loadedFile !== file) return
        view.dispatch({ effects: setSpelling.of(hits) })
      }).catch(() => {})
    }, 400)
  }

  function scheduleSyntax() {
    if (!editing || !view || !loadedFile || !highlighted(loadedFile)) return
    if (view.state.doc.length > SYNTAX_LIMIT) return
    const file = loadedFile
    if (syntaxTimer) clearTimeout(syntaxTimer)
    syntaxTimer = setTimeout(() => {
      if (!view || loadedFile !== file) return
      const text = view.state.doc.toString()
      if (text.length > SYNTAX_LIMIT) return
      void api.checkSyntax(text).then((issues) => {
        if (loadedFile !== file) return
        syntax = new Map(issues.map((i) => [i.line, i.message]))
        applyDiagnostics()
      })
    }, 400)
  }

  $effect(() => {
    const l = loc
    if (!view) return
    if (!l) {
      blank()
      return
    }
    if (l === lastLoc) return
    lastLoc = l
    void (async () => {
      if (loadedFile !== l.file) {
        remember()
        if (!showBuffer(l.file) && !(await load(l.file))) return
      }
      reveal(l)
    })()
  })

  // External edits. A dirty buffer is left alone.
  $effect(() => {
    const seq = changeSeq
    if (seq === appliedSeq) return
    appliedSeq = seq
    const file = loadedFile
    if (!file || !view || !changedPaths.includes(file)) return
    if (dirty) {
      onstale(file)
      return
    }
    buffers.delete(file)
    const v = view
    const snap = v.scrollSnapshot()
    void load(file).then((ok) => {
      if (ok && view) {
        view.dispatch({ effects: snap })
        if (lastLoc && lastLoc.file === file) {
          const doc = view.state.doc
          const from = Math.min(lastLoc.line, doc.lines)
          view.dispatch({ effects: setRange.of({ from, to: Math.max(from, lastLoc.endLine) }) })
        }
      }
    })
  })

  // Revert: the backend already wrote the pristine bytes. Show them.
  $effect(() => {
    const req = reloadFile
    if (!req || req.seq === seenReload) return
    seenReload = req.seq
    buffers.delete(req.path)
    saved.delete(req.path)
    if (loadedFile === req.path) void load(req.path)
  })

  // Turning editing off discards unsaved buffers and goes back to the file on disk.
  $effect(() => {
    const on = editing
    const script = !!loadedFile && !!fileInfo(loadedFile)
    if (view) {
      view.dispatch({ effects: readOnlyComp.reconfigure(EditorState.readOnly.of(!(on && script))) })
    }
  })

  $effect(() => {
    const dark = !appearance.light
    view?.dispatch({ effects: themeComp.reconfigure(editorTheme(dark)) })
  })

  $effect(() => {
    const wrap = appearance.wrap
    view?.dispatch({ effects: wrapComp.reconfigure(wrap ? EditorView.lineWrapping : []) })
  })

  $effect(() => {
    const on = appearance.spell
    view?.dispatch({ effects: spellComp.reconfigure(on ? spellSupport() : []) })
    if (on) scheduleSpell()
  })

  $effect(() => {
    const on = settings.lineNumbers
    view?.dispatch({ effects: gutterComp.reconfigure(on ? lineNumbers() : []) })
  })

  $effect(() => {
    const on = settings.foldGutter
    view?.dispatch({ effects: foldComp.reconfigure(on ? foldGutter() : []) })
  })

  $effect(() => {
    const on = settings.activeLine
    view?.dispatch({ effects: activeLineComp.reconfigure(on ? highlightActiveLine() : []) })
  })

  $effect(() => {
    const on = settings.bracketMatching
    view?.dispatch({ effects: bracketComp.reconfigure(on ? bracketMatching() : []) })
  })

  $effect(() => {
    void settings.closeBrackets
    view?.dispatch({ effects: closeComp.reconfigure(closeExt()) })
  })

  $effect(() => {
    void settings.autocomplete
    view?.dispatch({ effects: completeComp.reconfigure(completeExt()) })
  })

  $effect(() => {
    void settings.indentWidth
    view?.dispatch({ effects: indentComp.reconfigure(indentExt()) })
  })

  $effect(() => {
    void settings.colorSwatches
    view?.dispatch({ effects: swatchComp.reconfigure(swatchExt()) })
  })

  $effect(() => {
    void settings.inlayHints
    void app.catalog
    void app.map
    view?.dispatch({ effects: inlayComp.reconfigure(inlayExt()) })
  })

  $effect(() => {
    const mode = settings.keymap
    void editorReady
    void loadedFile
    setKeymapSave(() => ctx.save())
    if (!view) return
    let alive = true
    void keymapExtension(mode).then((ext) => {
      if (!alive || !view || settings.keymap !== mode) return
      view.dispatch({ effects: keymapComp.reconfigure(ext) })
    })
    return () => {
      alive = false
    }
  })

  function applyDiagnostics() {
    if (!view || !loadedFile) return
    const map = new Map<number, { severity: Severity; message: string }>()
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
    if (loadedFile) {
      for (const d of pythonDiagnostics(loadedFile)) {
        if (d.severity === 'info') continue
        if (settings.pythonDiagnostics === 'syntax' && d.code !== 'invalid-syntax') continue
        if (!inPython(view.state.doc, d.line)) continue
        const severity: Severity = d.severity === 'warning' ? 'warning' : 'error'
        const prev = map.get(d.line)
        if (!prev || severity === 'error') map.set(d.line, { severity, message: d.message })
      }
    }
    view.dispatch({ effects: setDiags.of(map) })
  }

  $effect(() => {
    setPyFile(loadedFile ?? '')
    void py.status
    void pyDiags.seq
    void settings.pythonDiagnostics
    applyDiagnostics()
    if (py.status === 'ready' && loadedFile && view) syncDocument(loadedFile, view.state.doc.toString())
  })

  $effect(() => {
    void diagnostics
    void loadedFile
    applyDiagnostics()
  })

  $effect(() => {
    void git.seq
    void editorReady
    const file = loadedFile
    if (!view || !file || !highlighted(file)) {
      view?.dispatch({ effects: setIndex.of(null) })
      return
    }
    let alive = true
    void api.gitShow('INDEX', `game/${file}`).then(
      (text) => {
        if (!alive || !view || loadedFile !== file) return
        // The editor holds LF text without a BOM, whatever the file uses on disk.
        const plain = (text.charCodeAt(0) === 0xfeff ? text.slice(1) : text).replace(/\r\n?/g, '\n')
        view.dispatch({ effects: setIndex.of(Text.of(plain.split('\n'))) })
      },
      () => {
        if (!alive || !view || loadedFile !== file) return
        view.dispatch({ effects: setIndex.of(null) })
      },
    )
    return () => {
      alive = false
    }
  })

  $effect(() => {
    const line = liveLine
    if (!view || !loadedFile) return
    view.dispatch({ effects: setLive.of(line && line > 0 ? line : null) })
  })
</script>

<div class="code">
  <div class="head pane-head">
    {#if loadedFile}
      <span class="dim">
        {lineCount.toLocaleString()} lines
        {#if dirty}· unsaved{:else if editing && fileInfo(loadedFile)}· editing{/if}
        {#if settings.keymap !== 'default'}· {settings.keymap === 'vim' ? 'Vim' : 'Emacs'}{/if}
        {#if modified}· differs from backup{/if}
      </span>
      {#if !(editing && fileInfo(loadedFile))}<span class="ro">Read-only</span>{/if}
      {#if hint}<span class="hint" title={hint}>{hint}</span>{/if}
      {#if editing && modified}
        <button class="revert" onclick={() => loadedFile && onrevert(loadedFile)} title="Restore the copy taken before the first save">
          Revert
        </button>
      {/if}
    {:else}
      <span class="dim">No file open</span>
    {/if}
    {#if loading}<span class="dim">loading…</span>{/if}
  </div>
  {#if errorMsg}<div class="err">{errorMsg}</div>{/if}
  <div class="host" class:off={!loadedFile} bind:this={host}></div>
  {#if !loadedFile && !loading}
    <div class="empty">Open a script from the explorer, or press Ctrl+P.</div>
  {/if}
</div>

<style>
  .code {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--bg-code);
  }
  .head {
    min-width: 0;
    background: var(--bg-code);
    border-bottom-color: var(--line-soft);
  }
  .dim {
    color: var(--dim);
    flex: none;
  }
  .ro {
    flex: none;
    font-size: var(--fs-xs);
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--warning);
    border: 1px solid color-mix(in srgb, var(--warning) 55%, var(--line));
    border-radius: var(--r-sm);
    padding: 0 5px;
  }
  .hint {
    color: var(--dim);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .revert {
    margin-left: auto;
    height: 20px;
    font-size: var(--fs-sm);
    padding: 0 8px;
  }
  .host {
    flex: 1;
    min-height: 0;
  }
  .host.off {
    visibility: hidden;
  }
  .err {
    color: var(--error);
    padding: 8px 12px;
    font-size: var(--fs-md);
  }
  .empty {
    position: absolute;
    inset: 40px 0 0;
    display: grid;
    place-items: center;
    color: var(--dim);
    pointer-events: none;
  }
  :global(.diag-dot) {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin: 0 3px;
    vertical-align: middle;
  }
  :global(.diag-dot.error) {
    background: var(--error);
  }
  :global(.diag-dot.warning) {
    background: var(--warning);
  }
  :global(.diag-dot.info) {
    background: var(--dim);
  }
  :global(.live-mark) {
    color: var(--ok);
    font-size: 9px;
    line-height: 1.55;
  }
  :global(.live-spacer) {
    visibility: hidden;
    font-size: 9px;
  }
  :global(.cm-inlay) {
    margin-left: 0.65em;
    color: var(--dim);
    font-style: italic;
    font-size: 0.92em;
    pointer-events: none;
  }
  :global(.cm-swatch) {
    display: inline-block;
    width: 0.85em;
    height: 0.85em;
    margin-left: 3px;
    border: 1px solid var(--line);
    border-radius: 2px;
    vertical-align: -0.12em;
    cursor: pointer;
  }
  :global(.sym-tip) {
    padding: 6px 8px;
    max-width: 260px;
  }
  :global(.sym-tip a) {
    display: inline-block;
    margin-top: 4px;
    color: var(--accent);
  }
  :global(.sig-tip) {
    padding: 6px 8px;
    max-width: 360px;
  }
  :global(.sig-active) {
    color: var(--accent);
    font-weight: 700;
  }
  :global(.sig-note) {
    margin-top: 4px;
    color: var(--dim);
  }
  :global(.sym-tip img) {
    display: block;
    max-width: 240px;
    max-height: 140px;
    margin-top: 6px;
  }
  :global(.cm-spell) {
    text-decoration: underline wavy var(--error);
    text-underline-offset: 3px;
  }
  :global(.spell-tip) {
    padding: 6px 8px;
    max-width: 240px;
    font-size: var(--fs-md);
  }
  :global(.spell-word) {
    font-weight: 600;
    margin-bottom: 4px;
  }
  :global(.spell-list) {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  :global(.spell-list button),
  :global(.spell-add) {
    text-align: left;
    font-size: var(--fs-md);
  }
  :global(.spell-add) {
    margin-top: 6px;
  }
</style>

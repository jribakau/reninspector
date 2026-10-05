import { redo, redoDepth, selectAll, undo, undoDepth } from '@codemirror/commands'
import { EditorSelection, EditorState } from '@codemirror/state'
import type { EditorView } from '@codemirror/view'
import type { MenuEntry } from '../context.svelte'
import { notify } from '../toast.svelte'

/** Text covered by the selection. Empty ranges are left out, so a bare caret copies nothing. */
export function selectionText(state: EditorState): string {
  return state.selection.ranges
    .filter((range) => !range.empty)
    .map((range) => state.sliceDoc(range.from, range.to))
    .join(state.lineBreak)
}

/** True when `pos` sits inside a selection, including either edge. */
export function selectionCovers(state: EditorState, pos: number): boolean {
  return state.selection.ranges.some((range) => range.from <= pos && pos <= range.to)
}

/** Right-click outside the selection moves the caret there. A click inside it keeps the selection, so Copy still works. */
export function placeCaret(view: EditorView, pos: number | null) {
  if (pos == null || selectionCovers(view.state, pos)) return
  view.dispatch({ selection: EditorSelection.cursor(pos), userEvent: 'select.pointer' })
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    return false
  }
}

async function cutSelection(view: EditorView) {
  const text = selectionText(view.state)
  if (!text || view.state.readOnly) return
  if (!(await writeClipboard(text))) {
    notify('Could not cut.', 'error')
    return
  }
  view.focus()
  view.dispatch(view.state.replaceSelection(''))
}

async function copySelection(view: EditorView) {
  const text = selectionText(view.state)
  if (!text) return
  if (!(await writeClipboard(text))) notify('Could not copy.', 'error')
}

async function pasteSelection(view: EditorView) {
  if (view.state.readOnly) return
  let text: string | null = null
  try {
    text = await navigator.clipboard.readText()
  } catch {
    text = null
  }
  view.focus()
  if (text == null) {
    if (!document.execCommand('paste')) notify('Could not paste.', 'error')
    return
  }
  view.dispatch(view.state.replaceSelection(text))
}

export interface EditorMenuActions {
  goto: () => void
  refs: () => void
  rename: () => void
  find: () => void
  comment: () => void
}

export function editorMenu(view: EditorView, actions: EditorMenuActions): MenuEntry[] {
  const state = view.state
  const copied = selectionText(state)
  const locked = state.readOnly
  return [
    { kind: 'item', label: 'Cut', key: 'Ctrl+X', enabled: !!copied && !locked, run: () => void cutSelection(view) },
    { kind: 'item', label: 'Copy', key: 'Ctrl+C', enabled: !!copied, run: () => void copySelection(view) },
    { kind: 'item', label: 'Paste', key: 'Ctrl+V', enabled: !locked, run: () => void pasteSelection(view) },
    { kind: 'sep' },
    { kind: 'item', label: 'Go to definition', key: 'F12', run: actions.goto },
    { kind: 'item', label: 'Find references', key: 'Shift+F12', run: actions.refs },
    { kind: 'item', label: 'Rename symbol', key: 'F2', enabled: !locked, run: actions.rename },
    { kind: 'sep' },
    { kind: 'item', label: 'Undo', key: 'Ctrl+Z', enabled: undoDepth(state) > 0, run: () => undo(view) },
    { kind: 'item', label: 'Redo', key: 'Ctrl+Y', enabled: redoDepth(state) > 0, run: () => redo(view) },
    { kind: 'sep' },
    { kind: 'item', label: 'Select all', key: 'Ctrl+A', run: () => selectAll(view) },
    { kind: 'item', label: 'Find', key: 'Ctrl+F', run: actions.find },
    { kind: 'item', label: 'Toggle comment', key: 'Ctrl+/', enabled: !locked, run: actions.comment },
  ]
}

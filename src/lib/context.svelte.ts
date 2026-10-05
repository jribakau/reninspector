import { saveBuffer } from './buffers'
import { deleteScript, renameScript, revertFile } from './edit.svelte'
import { inGame, revealEntry, toTreePath } from './fileops.svelte'
import { fileInfo, fileOfNode, labelAt, nodeByName, nodesInFile } from './indexes.svelte'
import { jumpGameHere, replayToCursor } from './live.svelte'
import { app, editorTabId, type EditorTab } from './model.svelte'
import {
  activateEditor,
  bulkCloseCount,
  canReopenClosed,
  closeAllTabs,
  closeEditor,
  closeOtherTabs,
  closeSavedTabs,
  closeTabsToTheLeft,
  closeTabsToTheRight,
  goTo,
  isPinned,
  pinEditor,
  reopenClosedTab,
  unpinEditor,
  openFileHistory,
  showCode,
  openLabelGraph,
  revealInExplorer,
  selectLabel,
  showReferences,
} from './nav.svelte'
import { tabTitle, type BulkClose } from './tabs'
import type { GNode, MapNode, ScriptLine } from './types'

export type MenuEntry =
  | { kind: 'sep' }
  | {
      kind: 'item'
      label: string
      enabled?: boolean
      run: () => void
      /** Tooltip, used to say why an item is off. */
      hint?: string
      /** Shortcut or short note shown on the right. */
      key?: string
      /** Marks the entry that is current, such as the active tab in a list. */
      current?: boolean
    }

export const contextMenu = $state({
  open: false,
  x: 0,
  y: 0,
  items: [] as MenuEntry[],
  /** When set, `x` is where the menu's right edge goes rather than its left. */
  rightEdge: false,
})

export function closeContextMenu() {
  contextMenu.open = false
}

function tidy(items: MenuEntry[]): MenuEntry[] {
  const out: MenuEntry[] = []
  for (const item of items) {
    if (item.kind === 'sep') {
      if (!out.length || out[out.length - 1].kind === 'sep') continue
      out.push(item)
    } else {
      out.push(item)
    }
  }
  while (out[0]?.kind === 'sep') out.shift()
  while (out[out.length - 1]?.kind === 'sep') out.pop()
  return out
}

export function openContextMenu(e: MouseEvent, items: MenuEntry[]) {
  const next = tidy(items)
  if (!next.length) return
  e.preventDefault()
  e.stopPropagation()
  contextMenu.items = next
  contextMenu.x = e.clientX
  contextMenu.y = e.clientY
  contextMenu.rightEdge = false
  contextMenu.open = true
}

/** Opens a menu below an element, such as a toolbar button, lined up with its right edge. */
export function openMenuBelow(el: Element, items: MenuEntry[]) {
  const next = tidy(items)
  if (!next.length) return
  const box = el.getBoundingClientRect()
  contextMenu.items = next
  contextMenu.x = box.right
  contextMenu.y = box.bottom + 2
  contextMenu.rightEdge = true
  contextMenu.open = true
}

export function copyText(text: string) {
  const value = text.trim()
  if (!value) return
  const done = () => {
    app.notice = 'Copied.'
  }
  if (navigator.clipboard?.writeText) {
    void navigator.clipboard.writeText(value).then(done).catch(() => copyFallback(value, done))
    return
  }
  copyFallback(value, done)
}

function copyFallback(value: string, done: () => void) {
  const area = document.createElement('textarea')
  area.value = value
  area.setAttribute('readonly', '')
  area.style.position = 'fixed'
  area.style.left = '-9999px'
  document.body.appendChild(area)
  area.select()
  document.execCommand('copy')
  area.remove()
  done()
}

function playAt(file: string, line: number, endLine: number, replay: boolean) {
  app.cursor = { file, line }
  goTo(file, line, endLine, { activate: false, flow: false })
  if (replay) void replayToCursor()
  else void jumpGameHere()
}

/** Absolute path of a project path, for the clipboard. */
function absolutePath(treePath: string): string {
  const root = (app.info?.root ?? '').replaceAll('\\', '/').replace(/\/+$/, '')
  return root ? `${root}/${treePath}` : treePath
}

/** The script a diff tab's repository path stands for, or null when it is not an open-able script. */
function scriptOfDiff(path: string): string | null {
  if (fileInfo(path)) return path
  const rel = inGame(path)
  return rel && fileInfo(rel) ? rel : null
}

/** A bulk close entry, disabled with a reason when it would close nothing. */
function bulkItem(label: string, mode: BulkClose, anchor: string | null, none: string, run: () => void): MenuEntry {
  const count = bulkCloseCount(mode, anchor)
  const pinnedLeft = app.pinnedTabs.length > 0 ? ' Pinned tabs stay open.' : ''
  return { kind: 'item', label, enabled: count > 0, hint: count > 0 ? undefined : none + pinnedLeft, run }
}

function closeGroup(id: string | null): MenuEntry[] {
  const items: MenuEntry[] = []
  if (id) {
    items.push(
      { kind: 'item', label: 'Close', key: 'Ctrl+W', run: () => void closeEditor(id) },
      bulkItem('Close Others', 'others', id, 'No other tabs to close.', () => void closeOtherTabs(id)),
      bulkItem('Close to the Right', 'right', id, 'No tabs to the right.', () => void closeTabsToTheRight(id)),
      bulkItem('Close to the Left', 'left', id, 'No tabs to the left.', () => void closeTabsToTheLeft(id)),
    )
  }
  items.push(
    bulkItem('Close Saved', 'saved', null, 'No saved tabs to close.', () => void closeSavedTabs()),
    bulkItem('Close All', 'all', null, 'No tabs to close.', () => void closeAllTabs()),
  )
  return [
    ...items,
    { kind: 'sep' },
    {
      kind: 'item',
      label: 'Reopen Closed Editor',
      key: 'Ctrl+Shift+T',
      enabled: canReopenClosed(),
      hint: canReopenClosed() ? undefined : 'Nothing has been closed yet.',
      run: reopenClosedTab,
    },
  ]
}

/** Right-click menu for the empty part of the tab bar. */
export function tabBarItems(): MenuEntry[] {
  return closeGroup(null)
}

/** Every open tab, for the overflow button. */
export function tabListItems(): MenuEntry[] {
  const items: MenuEntry[] = app.editorTabs.map((tab) => {
    const id = editorTabId(tab)
    const notes: string[] = []
    if (isPinned(id)) notes.push('Pinned')
    if (tab.kind === 'file' && app.dirtyFiles.includes(tab.path)) notes.push('Unsaved')
    if (app.live.running && ((tab.kind === 'graph' && app.live.label === tab.name) || (tab.kind === 'file' && app.live.file === tab.path))) notes.push('Live')
    return {
      kind: 'item',
      label: tabTitle(tab),
      key: notes.join(' · ') || undefined,
      current: app.activeEditor === id,
      hint: tab.kind === 'file' ? tab.path : undefined,
      run: () => activateEditor(id),
    }
  })
  if (!items.length) return items
  return [...items, { kind: 'sep' }, ...closeGroup(null)]
}

export function tabItems(tab: EditorTab): MenuEntry[] {
  const id = editorTabId(tab)
  const pinned = isPinned(id)
  const items: MenuEntry[] = [
    ...closeGroup(id),
    { kind: 'sep' },
    pinned
      ? { kind: 'item', label: 'Unpin Tab', run: () => unpinEditor(id) }
      : { kind: 'item', label: 'Pin Tab', run: () => pinEditor(id) },
    { kind: 'sep' },
  ]
  if (tab.kind === 'file') {
    const path = tab.path
    const info = fileInfo(path)
    const dirty = app.dirtyFiles.includes(path)
    const modified = app.modifiedFiles.includes(path)
    const tree = toTreePath(path)
    const archived = info?.origin === 'archived'
    const here = app.cursor?.file === path ? labelAt(path, app.cursor.line) : undefined
    const usable = (n: MapNode | null | undefined): n is MapNode => !!n && n.kind !== 'missing' && n.kind !== 'screen' && n.kind !== 'compiled'
    const flow = usable(here) ? here : nodesInFile(path).find(usable)
    const editable = info?.editable !== false
    if (info?.reasons.length) {
      const reasons = info.reasons.join('\n')
      items.push({
        kind: 'item',
        label: 'Copy decompile reasons',
        run: () => copyText(reasons),
      })
    }
    items.push(
      {
        kind: 'item',
        label: 'Save',
        key: 'Ctrl+S',
        enabled: dirty && editable && !app.busy,
        hint: dirty ? undefined : 'No unsaved changes.',
        run: () => void saveBuffer(path),
      },
      {
        kind: 'item',
        label: 'Revert File',
        enabled: modified && !app.busy,
        hint: modified ? 'Restore the copy saved before the IDE first changed it.' : 'The IDE has not changed this file.',
        run: () => void revertFile(path),
      },
      { kind: 'sep' },
      {
        kind: 'item',
        label: 'Open Flow',
        enabled: !!flow,
        hint: flow ? undefined : 'This script has no label to draw.',
        run: () => flow && openLabelGraph(flow.id, { open: true }),
      },
      { kind: 'item', label: 'Open File History', run: () => openFileHistory(path) },
      { kind: 'sep' },
      { kind: 'item', label: 'Copy Path', run: () => copyText(absolutePath(tree)) },
      { kind: 'item', label: 'Copy Relative Path', run: () => copyText(tree) },
      { kind: 'item', label: 'Reveal in Explorer', run: () => revealInExplorer(path) },
      {
        kind: 'item',
        label: 'Reveal in File Manager',
        enabled: !archived,
        hint: archived ? `Inside ${info?.archive ?? 'an archive'}, so there is no loose file.` : undefined,
        run: () => revealEntry(tree),
      },
    )
  } else if (tab.kind === 'graph') {
    const node = nodeByName(tab.name)
    const file = node ? fileOfNode(node) : null
    const canSource = !!file && !!node && node.kind !== 'missing' && node.kind !== 'compiled'
    items.push(
      { kind: 'item', label: 'Open Source', enabled: canSource, hint: canSource ? undefined : 'No script defines this label.', run: () => selectLabel(tab.name, { open: true }) },
      { kind: 'item', label: 'Find References', run: () => showReferences(tab.name.startsWith('screen:') ? 'screen' : 'label', tab.name.startsWith('screen:') ? tab.name.slice(7) : tab.name) },
      { kind: 'sep' },
      { kind: 'item', label: 'Copy Name', run: () => copyText(tab.name.startsWith('screen:') ? tab.name.slice(7) : tab.name) },
    )
    if (file) {
      items.push(
        { kind: 'item', label: 'Reveal in Explorer', run: () => revealInExplorer(file) },
        { kind: 'item', label: 'Reveal in File Manager', run: () => revealEntry(toTreePath(file)) },
      )
    }
  } else if (tab.kind === 'diff') {
    const script = scriptOfDiff(tab.path)
    items.push(
      { kind: 'item', label: 'Open File', enabled: !!script, hint: script ? undefined : 'This file is not a script in the project.', run: () => script && goTo(script, 1, 1, { flow: true, open: true }) },
      { kind: 'item', label: 'Open File History', enabled: !!script, run: () => script && openFileHistory(script) },
      { kind: 'sep' },
      { kind: 'item', label: 'Copy Path', run: () => copyText(absolutePath(tab.path)) },
      { kind: 'item', label: 'Copy Relative Path', run: () => copyText(tab.path) },
      { kind: 'item', label: 'Reveal in Explorer', run: () => revealInExplorer(tab.path) },
      { kind: 'item', label: 'Reveal in File Manager', run: () => revealEntry(tab.path) },
    )
  }
  return items
}

export function labelItems(id: string): MenuEntry[] {
  const node = nodeByName(id)
  const file = node ? fileOfNode(node) : null
  const canSource = !!node && !!file && node.kind !== 'missing' && node.kind !== 'compiled'
  const canFlow = canSource && node?.kind !== 'screen'
  const warpOff = app.live.running && !app.live.canWarp
  const items: MenuEntry[] = [
    { kind: 'item', label: 'Open source', enabled: canSource, run: () => selectLabel(id, { open: true }) },
    { kind: 'item', label: 'Open flow', enabled: canFlow, run: () => openLabelGraph(id, { open: true }) },
  ]
  if (canSource && node && file && node.kind !== 'screen') {
    items.push(
      { kind: 'item', label: 'Run from here', enabled: !warpOff, run: () => playAt(file, node.line, node.endLine, false) },
      { kind: 'item', label: 'Replay to line', run: () => playAt(file, node.line, node.endLine, true) },
    )
  }
  items.push({ kind: 'sep' })
  if (file) items.push({ kind: 'item', label: 'Reveal in explorer', run: () => revealInExplorer(file) })
  if (node && (node.kind === 'label' || node.kind === 'menu')) {
    items.push({ kind: 'item', label: 'Find references', run: () => showReferences('label', id) })
  } else if (node?.kind === 'screen') {
    items.push({ kind: 'item', label: 'Find references', run: () => showReferences('screen', id.slice(7)) })
  }
  const shown = node?.kind === 'screen' ? id.slice(7) : id
  items.push({ kind: 'item', label: 'Copy name', run: () => copyText(shown) })
  return items
}

export function scriptFileItems(path: string): MenuEntry[] {
  return [
    { kind: 'item', label: 'Open', run: () => goTo(path, 1, 1, { flow: true, open: true }) },
    { kind: 'item', label: 'Open file history', run: () => openFileHistory(path) },
    { kind: 'item', label: 'Copy path', run: () => copyText(path) },
    { kind: 'item', label: 'Reveal in explorer', run: () => revealInExplorer(path) },
    { kind: 'sep' },
    { kind: 'item', label: 'Rename', run: () => void renameScript(path) },
    { kind: 'item', label: 'Delete', run: () => void deleteScript(path) },
  ]
}

/** What the open flow can do to a card. Absent entries are left out of the menu. */
export interface FlowActions {
  edit?: () => void
  insert?: (side: 'above' | 'below') => void
  canInsert?: (side: 'above' | 'below') => boolean
  duplicate?: () => void
  move?: (dir: -1 | 1) => void
  remove?: () => void
}

export function flowNodeItems(file: string | null, n: GNode, actions?: FlowActions): MenuEntry[] {
  const items: MenuEntry[] = []
  if (actions) {
    if (actions.edit) items.push({ kind: 'item', label: 'Edit', run: actions.edit })
    const insert = actions.insert
    if (insert) {
      items.push(
        { kind: 'item', label: 'Add above', enabled: actions.canInsert?.('above') ?? true, run: () => insert('above') },
        { kind: 'item', label: 'Add below', enabled: actions.canInsert?.('below') ?? true, run: () => insert('below') },
      )
    }
    const move = actions.move
    if (actions.duplicate) items.push({ kind: 'item', label: 'Duplicate', run: actions.duplicate })
    if (move) {
      items.push(
        { kind: 'item', label: 'Move up', run: () => move(-1) },
        { kind: 'item', label: 'Move down', run: () => move(1) },
      )
    }
    if (actions.remove) items.push({ kind: 'item', label: 'Delete', run: actions.remove })
    items.push({ kind: 'sep' })
  }
  if (file && n.line) {
    items.push({
      kind: 'item',
      label: 'Open in code',
      run: () => {
        showCode()
        goTo(file, n.line, n.endLine, { open: true })
      },
    })
    const warpOff = app.live.running && !app.live.canWarp
    items.push(
      { kind: 'item', label: 'Run from here', enabled: !warpOff, run: () => playAt(file, n.line, n.endLine, false) },
      { kind: 'item', label: 'Replay to line', run: () => playAt(file, n.line, n.endLine, true) },
    )
  }
  if (n.target && !n.dynamic) {
    const screenId = `screen:${n.target}`
    const target = n.kind === 'screen' && nodeByName(screenId) ? screenId : n.target
    const known = nodeByName(target)
    items.push({
      kind: 'item',
      label: 'Open target',
      enabled: !!known && known.kind !== 'missing',
      run: () => openLabelGraph(target, { open: true }),
    })
  }
  items.push({ kind: 'item', label: 'Copy', run: () => copyText(n.title || n.target || n.kind) })
  return items
}

export function sceneLineItems(file: string, line: ScriptLine): MenuEntry[] {
  const text = line.kind === 'say' && line.speaker ? `${line.speaker}: ${line.text}` : line.text
  const warpOff = app.live.running && !app.live.canWarp
  return [
    {
      kind: 'item',
      label: 'Open in code',
      run: () => {
        showCode()
        app.cursor = { file, line: line.line }
        goTo(file, line.line, line.endLine, { open: true })
      },
    },
    { kind: 'item', label: 'Copy line', run: () => copyText(text) },
    { kind: 'sep' },
    {
      kind: 'item',
      label: 'Run from here',
      enabled: !warpOff,
      run: () => playAt(file, line.line, line.endLine, false),
    },
    { kind: 'item', label: 'Replay to line', run: () => playAt(file, line.line, line.endLine, true) },
  ]
}

export function placeItems(path: string, line: number, text: string): MenuEntry[] {
  return [
    { kind: 'item', label: 'Open', enabled: !!path, run: () => path && goTo(path, line, line, { open: true }) },
    { kind: 'item', label: 'Copy', enabled: !!text.trim(), run: () => copyText(text) },
    { kind: 'item', label: 'Reveal in explorer', enabled: !!path, run: () => path && revealInExplorer(path) },
  ]
}

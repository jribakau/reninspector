import { git } from './git.svelte'
import { saveBuffer } from './buffers'
import { askSaveDiscard } from './dialog.svelte'
import { fileOfNode, labelAt, nodeByName, nodesInFile } from './indexes.svelte'
import { visibleProblems } from './problems.svelte'
import { settings } from './settings.svelte'
import { bulkTargets, moveTab, pinTab, restorePins, settle, unpinTab, type BulkClose, type TabOrder } from './tabs'
import {
  app,
  editorTabId,
  type Activity,
  type BottomTab,
  type EditorTab,
  type RenpySection,
} from './model.svelte'

let locSeq = 0
let refSeq = 0
let searchSeq = 0
let revealSeq = 0

interface NavSpot {
  file: string
  line: number
}

const NAV_LIMIT = 50
const CLOSED_LIMIT = 20
const spots = $state({ back: [] as NavSpot[], forward: [] as NavSpot[] })
const closedEditors = $state<EditorTab[]>([])
/** Set while Back or Forward calls goTo, so that jump is not recorded again. */
let navigating = false

export function canGoBack(): boolean {
  return spots.back.length > 0
}

export function canGoForward(): boolean {
  return spots.forward.length > 0
}

export function canReopenClosed(): boolean {
  return closedEditors.length > 0
}

function rememberSpot(from: NavSpot) {
  const last = spots.back[spots.back.length - 1]
  if (last && last.file === from.file && last.line === from.line) return
  spots.back.push(from)
  if (spots.back.length > NAV_LIMIT) spots.back.splice(0, spots.back.length - NAV_LIMIT)
  spots.forward = []
}

function rememberClosed(tabs: EditorTab[]) {
  if (!tabs.length) return
  closedEditors.push(...tabs)
  if (closedEditors.length > CLOSED_LIMIT) closedEditors.splice(0, closedEditors.length - CLOSED_LIMIT)
}

/** Applies a tab list, keeping pinned tabs first and dropping pins whose tab is gone. */
function setTabs(next: EditorTab[]) {
  const byId = new Map(next.map((t) => [editorTabId(t), t]))
  applyOrder(settle([...byId.keys()], app.pinnedTabs), byId)
}

function applyOrder(order: TabOrder, byId = new Map(app.editorTabs.map((t) => [editorTabId(t), t]))) {
  app.editorTabs = order.ids.map((id) => byId.get(id)).filter((t): t is EditorTab => !!t)
  app.pinnedTabs = order.pinned
}

/** Renames tab ids after files moved, so pins follow their tabs. */
export function replaceTabs(moved: Map<string, EditorTab>) {
  app.pinnedTabs = app.pinnedTabs.map((id) => {
    const tab = moved.get(id)
    return tab ? editorTabId(tab) : id
  })
  setTabs(app.editorTabs.map((t) => moved.get(editorTabId(t)) ?? t))
}

export function isPinned(id: string): boolean {
  return app.pinnedTabs.includes(id)
}

export function pinEditor(id: string) {
  const ids = app.editorTabs.map(editorTabId)
  if (!ids.includes(id)) return
  applyOrder(pinTab(ids, app.pinnedTabs, id))
  saveSession()
}

export function unpinEditor(id: string) {
  const ids = app.editorTabs.map(editorTabId)
  if (!ids.includes(id)) return
  applyOrder(unpinTab(ids, app.pinnedTabs, id))
  saveSession()
}

const sessionKey = (root: string) => `vnide.session.${root}`
const ACTIVITIES: Activity[] = ['explorer', 'story', 'search', 'renpy', 'git']
const BOTTOM: BottomTab[] = ['problems', 'log', 'live', 'build']
const RENPY: RenpySection[] = ['characters', 'images', 'screens', 'variables', 'languages', 'assets', 'archives']

function saveSession() {
  if (!app.info) return
  localStorage.setItem(
    sessionKey(app.info.root),
    JSON.stringify({
      tabs: app.editorTabs,
      pinned: app.pinnedTabs,
      active: app.activeEditor,
      recentFiles: app.recentFiles,
      file: app.loc?.file ?? null,
      line: app.loc?.line ?? 1,
      activity: app.activity,
      sidebarOpen: app.sidebarOpen,
      bottomTab: app.bottomTab,
      bottomOpen: app.bottomOpen,
      flowOpen: app.flowOpen,
      stageOpen: app.stageOpen,
      flowDetail: app.flowDetail,
      sceneExpanded: app.sceneExpanded,
      renpySection: app.renpySection,
    }),
  )
}

function isActivity(value: string): value is Activity {
  return (ACTIVITIES as string[]).includes(value)
}

function isBottom(value: string): value is BottomTab {
  return (BOTTOM as string[]).includes(value)
}

function isRenpy(value: string): value is RenpySection {
  return (RENPY as string[]).includes(value)
}

/** Older sessions stored a single sidebar tab. Map those onto the new chrome. */
function applyLegacySidebar(sidebar: string) {
  if (sidebar === 'labels') app.activity = 'story'
  else if (sidebar === 'files') app.activity = 'explorer'
  else if (sidebar === 'search') app.activity = 'search'
  else if (sidebar === 'git') app.activity = 'git'
  else if (sidebar === 'problems') {
    app.bottomTab = 'problems'
    app.bottomOpen = true
  } else if (sidebar === 'console') {
    app.bottomTab = 'log'
    app.bottomOpen = true
  } else if (sidebar === 'live') {
    app.bottomTab = 'live'
    app.bottomOpen = true
  } else if (sidebar === 'archives') {
    app.activity = 'renpy'
    app.renpySection = 'archives'
  } else if (sidebar === 'assets') {
    app.activity = 'renpy'
    app.renpySection = 'assets'
  } else if (sidebar === 'vars') {
    app.activity = 'renpy'
    app.renpySection = 'variables'
  } else if (sidebar === 'translate') {
    app.activity = 'renpy'
    app.renpySection = 'languages'
  }
}

interface SavedSession {
  tabs?: unknown
  pinned?: unknown
  active?: string | null
  file?: string | null
  line?: number
  activity?: string
  sidebarOpen?: boolean
  bottomTab?: string
  bottomOpen?: boolean
  flowOpen?: boolean
  stageOpen?: boolean
  flowDetail?: boolean
  linesOpen?: boolean
  /** Kept so a session saved before the modes were removed still loads. */
  mode?: string
  codeOpen?: boolean
  sceneExpanded?: boolean
  renpySection?: string
  fileTabs?: string[]
  sidebar?: string
  recentFiles?: string[]
}

function savedTab(raw: unknown, known: Set<string>): EditorTab | null {
  if (!raw || typeof raw !== 'object') return null
  const t = raw as { kind?: string; path?: string; name?: string; rev?: string }
  if (t.kind === 'file' && typeof t.path === 'string' && known.has(t.path)) return { kind: 'file', path: t.path }
  if (t.kind === 'graph' && typeof t.name === 'string') {
    const node = nodeByName(t.name)
    if (node && node.kind !== 'missing') return { kind: 'graph', name: t.name }
  }
  if (t.kind === 'map') return { kind: 'map' }
  if (t.kind === 'diff' && typeof t.path === 'string' && typeof t.rev === 'string' && t.rev && t.path) {
    return { kind: 'diff', path: t.path, rev: t.rev }
  }
  return null
}

export function hasSavedSession(root: string): boolean {
  return localStorage.getItem(sessionKey(root)) !== null
}

/** Restores layout. False when this project has no saved session. */
export function restoreSession(root: string): boolean {
  try {
    const raw = localStorage.getItem(sessionKey(root))
    if (!raw) return false
    const saved = JSON.parse(raw) as SavedSession
    const known = new Set(app.info?.files.map((f) => f.path) ?? [])
    if (saved.activity && isActivity(saved.activity)) app.activity = saved.activity
    if (typeof saved.sidebarOpen === 'boolean') app.sidebarOpen = saved.sidebarOpen
    if (saved.bottomTab && isBottom(saved.bottomTab)) app.bottomTab = saved.bottomTab
    if (typeof saved.bottomOpen === 'boolean') app.bottomOpen = saved.bottomOpen
    if (typeof saved.flowOpen === 'boolean') app.flowOpen = saved.flowOpen
    if (typeof saved.stageOpen === 'boolean') app.stageOpen = saved.stageOpen
    if (typeof saved.flowDetail === 'boolean') app.flowDetail = saved.flowDetail
    else if (saved.mode === 'write') app.flowDetail = true
    // Write mode used to put the scene in the middle whenever the code was hidden.
    if (typeof saved.sceneExpanded === 'boolean') app.sceneExpanded = saved.sceneExpanded
    else app.sceneExpanded = saved.mode === 'write' && saved.codeOpen === false
    if (saved.renpySection && isRenpy(saved.renpySection)) app.renpySection = saved.renpySection
    if (saved.sidebar && !saved.activity) applyLegacySidebar(saved.sidebar)

    const tabs: EditorTab[] = []
    if (Array.isArray(saved.tabs)) {
      for (const rawTab of saved.tabs) {
        const tab = savedTab(rawTab, known)
        if (tab) tabs.push(tab)
      }
    } else {
      for (const path of saved.fileTabs ?? []) {
        if (known.has(path)) tabs.push({ kind: 'file', path })
      }
    }
    const restored = tabs.slice(-settings.maxTabs)
    app.pinnedTabs = restorePins(restored.map(editorTabId), saved.pinned).pinned
    setTabs(restored)
    if (Array.isArray(saved.recentFiles)) {
      app.recentFiles = saved.recentFiles.filter((path): path is string => typeof path === 'string' && known.has(path)).slice(0, 8)
    }
    const active = typeof saved.active === 'string' ? saved.active : null
    const restoreActive = active !== null && app.editorTabs.some((t) => editorTabId(t) === active)
    if (restoreActive) app.activeEditor = active
    if (saved.file && known.has(saved.file)) goTo(saved.file, saved.line ?? 1, saved.line ?? 1, { activate: !restoreActive, flow: false })
    else if (restoreActive) saveSession()
    return true
  } catch {
    /* a bad session blob should not block opening the project */
    return false
  }
}

export function showActivity(next: Activity) {
  if (app.sidebarOpen && app.activity === next) app.sidebarOpen = false
  else {
    app.activity = next
    app.sidebarOpen = true
  }
  saveSession()
}

export function openActivity(next: Activity) {
  app.activity = next
  app.sidebarOpen = true
  saveSession()
}

/** Opens the explorer and asks it to expand and scroll this file or folder into view. */
export function revealInExplorer(path: string) {
  revealSeq += 1
  app.explorerReveal = { path, seq: revealSeq }
  openActivity('explorer')
}

export function openRenpy(section: RenpySection) {
  app.renpySection = section
  openActivity('renpy')
}

export function toggleSidebar() {
  app.sidebarOpen = !app.sidebarOpen
  saveSession()
}

/** Opens a bottom tab. A second call for the same tab leaves it open. */
export function openBottom(tab: BottomTab) {
  app.bottomTab = tab
  app.bottomOpen = true
  saveSession()
}

export function showBottom(tab: BottomTab) {
  openBottom(tab)
}

export function toggleBottom() {
  app.bottomOpen = !app.bottomOpen
  saveSession()
}

export function toggleFlow() {
  app.flowOpen = !app.flowOpen
  saveSession()
}

export function toggleStage() {
  app.stageOpen = !app.stageOpen
  saveSession()
}

export function toggleFlowDetail() {
  app.flowDetail = !app.flowDetail
  saveSession()
}

/** The flow fills the middle. Detail comes on with it, the way the scene used to show it. */
export function toggleScene() {
  app.sceneExpanded = !app.sceneExpanded
  if (app.sceneExpanded) app.flowDetail = true
  saveSession()
}

/** Puts the code back in the middle. */
export function showCode() {
  if (!app.sceneExpanded) return
  app.sceneExpanded = false
  saveSession()
}

export function closeActiveTab() {
  if (app.palette) {
    app.palette = null
    return
  }
  if (app.activeEditor) void closeEditor(app.activeEditor)
}

function tabLabel(tab: EditorTab): string {
  if (tab.kind === 'map') return 'Project map'
  if (tab.kind === 'graph') return tab.name.startsWith('screen:') ? tab.name.slice(7) : tab.name
  if (tab.kind === 'diff') return `${tab.path.split('/').pop() ?? tab.path} (${tab.rev})`
  return tab.path.split('/').pop() ?? tab.path
}

function rememberFile(file: string) {
  app.recentFiles = [file, ...app.recentFiles.filter((path) => path !== file)].slice(0, 8)
}

/** Keeps the newest tab and drops the oldest clean tabs first when the strip is full. */
function adoptTabs(next: EditorTab[]) {
  const MAX_TABS = settings.maxTabs
  if (next.length <= MAX_TABS) {
    setTabs(next)
    return
  }
  const opened = next[next.length - 1]
  const older = next.slice(0, -1)
  const room = MAX_TABS - 1
  const drop = older.length - room
  const kept: EditorTab[] = []
  const evicted: string[] = []
  let dropped = 0
  for (const tab of older) {
    const dirty = tab.kind === 'file' && app.dirtyFiles.includes(tab.path)
    if (dropped < drop && !dirty && !isPinned(editorTabId(tab))) {
      dropped += 1
      evicted.push(tabLabel(tab))
      continue
    }
    kept.push(tab)
  }
  while (kept.length > room) {
    const at = kept.findIndex((t) => !isPinned(editorTabId(t)))
    // Only pinned tabs are left: let the strip run long rather than lose one.
    if (at < 0) break
    evicted.push(tabLabel(kept[at]))
    kept.splice(at, 1)
  }
  setTabs([...kept, opened])
  if (evicted.length) app.notice = `Closed ${evicted.join(', ')}. The editor keeps ${MAX_TABS} tabs.`
}

async function confirmDirty(path: string): Promise<boolean> {
  const name = path.split('/').pop() ?? path
  const choice = await askSaveDiscard(`Save changes to ${name}?`, 'This file has unsaved edits.')
  if (choice === 'cancel') return false
  if (choice === 'save') return saveBuffer(path)
  return true
}

export function cycleTabs(dir: 1 | -1) {
  const tabs = app.editorTabs
  if (!tabs.length) return
  const i = tabs.findIndex((t) => editorTabId(t) === app.activeEditor)
  const next = (Math.max(0, i) + dir + tabs.length) % tabs.length
  activateEditor(editorTabId(tabs[next]))
}

/** Jump to the next or previous error or warning from the caret. */
export function nextProblem(dir: 1 | -1) {
  const rows = visibleProblems()
  if (!rows.length) {
    app.notice = problemNotice()
    return
  }
  const file = app.cursor?.file ?? ''
  const line = app.cursor?.line ?? 0
  let idx = -1
  if (dir > 0) {
    idx = rows.findIndex((d) => d.path > file || (d.path === file && d.line > line))
    if (idx < 0) idx = 0
  } else {
    for (let i = rows.length - 1; i >= 0; i--) {
      const d = rows[i]
      if (d.path < file || (d.path === file && d.line < line)) {
        idx = i
        break
      }
    }
    if (idx < 0) idx = rows.length - 1
  }
  const hit = rows[idx]
  app.problemCursor = { path: hit.path, line: hit.line, seq: (app.problemCursor?.seq ?? 0) + 1 }
  openBottom('problems')
  goTo(hit.path, hit.line)
}

function problemNotice(): string {
  const items = app.diag?.items ?? []
  if (!items.length) return 'No problems.'
  const hiddenInfo = items.some((d) => d.severity === 'info')
  return hiddenInfo ? 'No problems match. Info messages are hidden until you turn that filter on.' : 'No problems match the current filters.'
}

export function openMapTab() {
  if (!app.editorTabs.some((t) => t.kind === 'map')) {
    adoptTabs([...app.editorTabs, { kind: 'map' }])
  }
  app.activeEditor = 'map'
  saveSession()
}

/** A label or menu the flow pane can draw for this script. */
function flowLabelIn(file: string, line: number, fallback: boolean): string | null {
  const at = labelAt(file, line)
  if (at && at.kind !== 'missing' && at.kind !== 'screen' && at.kind !== 'compiled') return at.id
  if (!fallback) return null
  const node = nodesInFile(file).find((n) => n.kind === 'label' || n.kind === 'menu')
  return node?.id ?? null
}

function showFileFlow(file: string, line: number, open: boolean, fallback: boolean) {
  const id = flowLabelIn(file, line, fallback)
  if (!id) {
    if (fallback) app.selectedLabel = null
    return
  }
  app.selectedLabel = id
  if (open) app.flowOpen = true
}

export function activateEditor(id: string) {
  const tab = app.editorTabs.find((t) => editorTabId(t) === id)
  if (!tab) return
  if (tab.kind === 'file') {
    if (app.loc?.file === tab.path) {
      app.activeEditor = id
      showCode()
      showFileFlow(tab.path, app.loc.line, false, true)
      saveSession()
      return
    }
    goTo(tab.path, 1, 1, { flow: true })
    showCode()
    saveSession()
    return
  }
  app.activeEditor = id
  if (tab.kind === 'graph') app.selectedLabel = tab.name
  saveSession()
}

export function showReferences(kind: string, name: string) {
  refSeq += 1
  app.refRequest = { kind, name, seq: refSeq }
  openActivity('search')
}

export function searchDialogue(query: string) {
  const q = query.trim()
  if (!q) return
  searchSeq += 1
  app.searchRequest = { query: q, dialogueOnly: true, seq: searchSeq }
  openActivity('search')
}

/** Opens Source Control on one script's history. Without a path, the open file's. */
export function openFileHistory(path = '') {
  git.historyFile = path
  openActivity('git')
  git.timeline += 1
}

export function goTo(file: string, line: number, endLine = line, opts: { activate?: boolean; flow?: boolean } = {}) {
  const prev = app.loc
  const fileChanged = prev?.file !== file
  if (!navigating && prev?.file && (fileChanged || Math.abs(prev.line - line) > 10)) {
    rememberSpot({ file: prev.file, line: prev.line })
  }
  locSeq += 1
  app.loc = { file, line, endLine: Math.max(line, endLine), seq: locSeq }
  if (file && !app.editorTabs.some((t) => t.kind === 'file' && t.path === file)) {
    adoptTabs([...app.editorTabs, { kind: 'file', path: file }])
  }
  if (file) rememberFile(file)
  if (file && opts.activate !== false) app.activeEditor = editorTabId({ kind: 'file', path: file })
  if (file) showFileFlow(file, line, opts.flow ?? fileChanged, fileChanged || opts.flow === true)
  saveSession()
}

export function navBack() {
  const cur = app.loc
  if (!cur?.file || !spots.back.length) return
  const spot = spots.back.pop()
  if (!spot) return
  spots.forward.push({ file: cur.file, line: cur.line })
  if (spots.forward.length > NAV_LIMIT) spots.forward.splice(0, spots.forward.length - NAV_LIMIT)
  navigating = true
  goTo(spot.file, spot.line)
  navigating = false
}

export function navForward() {
  const cur = app.loc
  if (!cur?.file || !spots.forward.length) return
  const spot = spots.forward.pop()
  if (!spot) return
  spots.back.push({ file: cur.file, line: cur.line })
  if (spots.back.length > NAV_LIMIT) spots.back.splice(0, spots.back.length - NAV_LIMIT)
  navigating = true
  goTo(spot.file, spot.line)
  navigating = false
}

export function reopenClosedTab() {
  while (closedEditors.length) {
    const tab = closedEditors.pop()
    if (!tab) return
    if (tab.kind === 'file') {
      if (!app.info?.files.some((f) => f.path === tab.path)) continue
      goTo(tab.path, 1)
      return
    }
    if (tab.kind === 'graph') {
      const node = nodeByName(tab.name)
      if (!node || node.kind === 'missing') continue
      openLabelGraph(tab.name)
      return
    }
    if (tab.kind === 'diff') {
      openDiff(tab.path, tab.rev)
      return
    }
    openMapTab()
    return
  }
}

/** Opens a read-only diff of `path` against `rev`. A rev ending in `^` compares that commit with its parent. */
export function openDiff(path: string, rev: string) {
  const tab: EditorTab = { kind: 'diff', path, rev }
  const id = editorTabId(tab)
  if (!app.editorTabs.some((t) => editorTabId(t) === id)) {
    adoptTabs([...app.editorTabs, tab])
  }
  app.activeEditor = id
  saveSession()
}

/** Writes every unsaved buffer, then reports how many were saved. */
export async function saveAll(options: { quiet?: boolean } = {}) {
  const files = [...app.dirtyFiles]
  if (!files.length) {
    if (!options.quiet) app.notice = 'Nothing to save.'
    return
  }
  let saved = 0
  for (const path of files) {
    if (!(await saveBuffer(path))) break
    saved += 1
  }
  if (!saved || options.quiet) return
  app.notice = saved === files.length
    ? `Saved ${saved} file${saved === 1 ? '' : 's'}.`
    : `Saved ${saved} of ${files.length} files.`
}

export async function closeEditor(id: string) {
  const tabs = app.editorTabs
  const i = tabs.findIndex((t) => editorTabId(t) === id)
  if (i < 0) return
  const closing = tabs[i]
  if (closing.kind === 'file' && app.dirtyFiles.includes(closing.path)) {
    if (!(await confirmDirty(closing.path))) return
  }
  rememberClosed([closing])
  const nextTabs = tabs.filter((_, idx) => idx !== i)
  setTabs(nextTabs)
  if (app.activeEditor !== id) {
    saveSession()
    return
  }
  const next = nextTabs[Math.min(i, nextTabs.length - 1)]
  if (!next) {
    app.activeEditor = null
    clearOpenDocument()
    saveSession()
    return
  }
  if (next.kind === 'file') {
    if (app.loc?.file === next.path) {
      app.activeEditor = editorTabId(next)
      saveSession()
    } else goTo(next.path, 1)
    return
  }
  app.activeEditor = editorTabId(next)
  if (next.kind === 'graph') app.selectedLabel = next.name
  saveSession()
}

export function closeFile(path: string) {
  closeEditor(editorTabId({ kind: 'file', path }))
}

async function confirmClosing(tabs: EditorTab[]): Promise<boolean> {
  for (const tab of tabs) {
    if (tab.kind === 'file' && app.dirtyFiles.includes(tab.path)) {
      if (!(await confirmDirty(tab.path))) return false
    }
  }
  return true
}

const dirtyIds = () => app.dirtyFiles.map((path) => editorTabId({ kind: 'file', path }))

/** The tabs a bulk close would drop right now. Pinned tabs and the anchor stay. */
export function bulkCloseCount(mode: BulkClose, anchor: string | null): number {
  const ids = app.editorTabs.map(editorTabId)
  return bulkTargets(ids, app.pinnedTabs, dirtyIds(), mode, anchor).length
}

async function closeBulk(mode: BulkClose, anchor: string | null) {
  const before = app.editorTabs
  const ids = before.map(editorTabId)
  const drop = new Set(bulkTargets(ids, app.pinnedTabs, dirtyIds(), mode, anchor))
  if (!drop.size) return
  const closing = before.filter((t) => drop.has(editorTabId(t)))
  if (!(await confirmClosing(closing))) return
  rememberClosed(closing)
  const keep = before.filter((t) => !drop.has(editorTabId(t)))
  const active = app.activeEditor
  setTabs(keep)
  if (active === null || !drop.has(active)) {
    saveSession()
    return
  }
  if (!keep.length) {
    app.activeEditor = null
    clearOpenDocument()
    saveSession()
    return
  }
  // Land on the anchor when there is one, otherwise the nearest tab that stayed.
  const anchorTab = anchor === null ? undefined : keep.find((t) => editorTabId(t) === anchor)
  const from = ids.indexOf(active)
  const after = keep.find((t) => ids.indexOf(editorTabId(t)) > from)
  activateEditor(editorTabId(anchorTab ?? after ?? keep[keep.length - 1]))
}

export function closeOtherTabs(id: string) {
  return closeBulk('others', id)
}

export function closeTabsToTheRight(id: string) {
  return closeBulk('right', id)
}

export function closeTabsToTheLeft(id: string) {
  return closeBulk('left', id)
}

export function closeSavedTabs() {
  return closeBulk('saved', null)
}

/** Drop the file, caret, and scene so a closed editor does not keep showing them. */
function clearOpenDocument() {
  app.loc = null
  app.cursor = null
  app.selectedLabel = null
}

/** Closes every tab except the pinned ones. */
export function closeAllTabs() {
  return closeBulk('all', null)
}

/** Moves a tab so it sits before the tab now at `slot`. Dropping across the pinned block pins or unpins it. */
export function moveEditor(id: string, slot: number) {
  const ids = app.editorTabs.map(editorTabId)
  if (!ids.includes(id)) return
  applyOrder(moveTab(ids, app.pinnedTabs, id, slot))
  saveSession()
}

/** Drop flow tabs whose label disappeared after a reload. */
export function pruneEditors() {
  const next = app.editorTabs.filter((t) => {
    if (t.kind !== 'graph') return true
    const node = nodeByName(t.name)
    return !!node && node.kind !== 'missing'
  })
  if (next.length !== app.editorTabs.length) setTabs(next)
  if (app.activeEditor && !next.some((t) => editorTabId(t) === app.activeEditor)) {
    const fallback = next[next.length - 1]
    app.activeEditor = fallback ? editorTabId(fallback) : null
  }
}

/** Select a label: highlight it everywhere and show its source. */
export function selectLabel(name: string, opts: { reveal?: boolean; activate?: boolean } = {}) {
  const node = nodeByName(name)
  app.selectedLabel = name
  if (!node || node.kind === 'missing') return
  if (opts.reveal !== false) {
    const file = fileOfNode(node)
    if (file) goTo(file, node.line, node.endLine, { activate: opts.activate !== false })
  }
}

/** Follow a jump, call, or fall in the flow pane, without opening a second copy as a tab. */
export function followFlowLabel(name: string) {
  const node = nodeByName(name)
  if (!node || node.kind === 'missing') {
    app.notice = `\`${name}\` is not defined anywhere in the project.`
    return
  }
  if (node.kind === 'compiled') {
    app.selectedLabel = name
    app.notice = `\`${name}\` comes from a compiled script or archive, so there is no source to draw.`
    return
  }
  if (node.kind !== 'screen') app.flowOpen = true
  selectLabel(name)
}

export function openLabelGraph(name: string) {
  const node = nodeByName(name)
  if (!node || node.kind === 'missing') {
    app.notice = `\`${name}\` is not defined anywhere in the project.`
    return
  }
  if (node.kind === 'screen') {
    selectLabel(name)
    return
  }
  if (node.kind === 'compiled') {
    app.selectedLabel = name
    app.notice = `\`${name}\` comes from a compiled script or archive, so there is no source to draw.`
    return
  }
  if (!app.editorTabs.some((t) => t.kind === 'graph' && t.name === name)) {
    adoptTabs([...app.editorTabs, { kind: 'graph', name }])
  }
  app.activeEditor = editorTabId({ kind: 'graph', name })
  selectLabel(name, { activate: false })
}

/** Called when the user moves the caret in the code view. */
export function cursorMoved(file: string, line: number) {
  const cur = app.cursor
  if (cur && cur.file === file && cur.line === line) return
  app.cursor = { file, line }
  const node = labelAt(file, line)
  if (node && node.id !== app.selectedLabel) app.selectedLabel = node.id
}

import { setSetting, settings } from './settings.svelte'
import type { CatalogView, DiagReport, EditImpact, LiveState, ProjectInfo, ProjectMap, RenamePreview } from './types'

export type Activity = 'explorer' | 'story' | 'search' | 'renpy' | 'git'

export type BottomTab = 'problems' | 'log' | 'live' | 'build'

export type RenpySection = 'game' | 'saves' | 'characters' | 'images' | 'screens' | 'variables' | 'languages' | 'assets' | 'archives'

export type PaletteMode = 'commands' | 'files' | 'symbols' | null

export type EditorTab =
  | { kind: 'file'; path: string }
  | { kind: 'graph'; name: string }
  | { kind: 'map' }
  | { kind: 'diff'; path: string; rev: string }
  | { kind: 'rebase'; path: string }

export function editorTabId(tab: EditorTab): string {
  if (tab.kind === 'file') return `file:${tab.path}`
  if (tab.kind === 'graph') return `graph:${tab.name}`
  if (tab.kind === 'diff') return `diff:${tab.rev}:${tab.path}`
  if (tab.kind === 'rebase') return `rebase:${tab.path}`
  return 'map'
}

export interface Loc {
  file: string
  line: number
  endLine: number
  /** Bumped on every request so the code view re-reveals even for the same target. */
  seq: number
}

export interface LiveView extends LiveState {
  receivedAt: number
}

export function emptyLive(): LiveView {
  return {
    running: false,
    file: '',
    line: 0,
    label: '',
    showing: [],
    speaker: '',
    vars: {},
    canWarp: false,
    canReload: false,
    replay: '',
    replayReason: '',
    note: '',
    warpNotes: [],
    receivedAt: 0,
  }
}

class AppModel {
  /** Large payloads stay raw so scans do not pay a proxy per field. */
  info = $state.raw<ProjectInfo | null>(null)
  map = $state.raw<ProjectMap | null>(null)
  diag = $state.raw<DiagReport | null>(null)
  busy = $state('')
  /** Project path while that project is opening. Other busy work leaves this empty. */
  opening = $state<string | null>(null)
  error = $state('')
  notice = $state('')
  activity = $state<Activity>('explorer')
  sidebarOpen = $state(true)
  bottomTab = $state<BottomTab>('problems')
  bottomOpen = $state(false)
  /** Bottom panel fills most of the window. */
  bottomMax = $state(false)
  /** Set by F8 so the Problems list can scroll to the same row. */
  problemCursor = $state<{ path: string; line: number; seq: number } | null>(null)
  /** Right-hand flow of the label under the caret. */
  flowOpen = $state(false)
  /** Stage preview beside the code, above the flow when both are open. */
  stageOpen = $state(false)
  /** Dialogue and choices of the label under the caret. Kept so older sessions still load. */
  linesOpen = $state(false)
  /** Flow chart shows one card per spoken line instead of a folded run. */
  flowDetail = $state(false)
  /** The flow fills the middle of the window instead of sitting beside the code. */
  sceneExpanded = $state(false)
  /** Line a flow edit just inserted or moved, so the graph can select it after the reload. */
  flowFocus = $state<{ file: string; line: number; seq: number; open: boolean } | null>(null)
  sceneCanUndo = $state(false)
  sceneCanRedo = $state(false)
  /** Labels the live game has stood in during this session. */
  visitedLabels = $state<string[]>([])
  /** Script, label-flow, and project-map tabs. The editor stays mounted under them. */
  editorTabs = $state<EditorTab[]>([])
  /** Ids of pinned tabs. They sit at the left and survive the bulk close commands. */
  pinnedTabs = $state<string[]>([])
  activeEditor = $state<string | null>(null)
  renpySection = $state<RenpySection>('characters')
  selectedLabel = $state<string | null>(null)
  loc = $state<Loc | null>(null)
  cursor = $state<{ file: string; line: number } | null>(null)
  launcher = $state<string | null>(null)
  recent = $state<string[]>([])
  /** Files opened in this project, newest first. */
  recentFiles = $state<string[]>([])
  /** Bumped when files changed on disk. */
  changeSeq = $state(0)
  changedPaths = $state<string[]>([])
  /** Open files whose buffer differs from the last load or save. */
  dirtyFiles = $state<string[]>([])
  /** Files that differ from the pristine backup. */
  modifiedFiles = $state<string[]>([])
  /** Saved edits that were decompiled from a `.rpyc`. */
  decompiledEdits = $state<string[]>([])
  /** Set after a save or revert. */
  impact = $state<EditImpact | null>(null)
  /** Ask the code view to reload one file from disk (a revert). */
  reloadFile = $state<{ path: string; seq: number } | null>(null)
  catalog = $state.raw<CatalogView | null>(null)
  palette = $state<PaletteMode>(null)
  autoreload = $state(false)
  refRequest = $state<{ kind: string; name: string; seq: number } | null>(null)
  /** Dialogue or text search requested from another panel. */
  searchRequest = $state<{ query: string; dialogueOnly: boolean; seq: number } | null>(null)
  /** Rename waiting for Apply. Nothing is written until then. */
  renamePreview = $state<RenamePreview | null>(null)
  /** Last report from a live game, plus when it arrived. */
  live = $state<LiveView>(emptyLive())
  /** When on, the editor cursor follows the running game. */
  get followGame(): boolean {
    return settings.followGame
  }
  set followGame(value: boolean) {
    setSetting('followGame', value)
  }
  /** Store names the live game should report. */
  watchVars = $state<string[]>([])
  /** Conditions the current replay is counting on. */
  replayAssumptions = $state<string[]>([])
  /** Set when replay could not plan a path, so the panel can offer a quick jump. */
  replayBlocked = $state(false)
  /** Explorer path to expand and scroll into view. Not saved with the session. */
  explorerReveal = $state<{ path: string; seq: number } | null>(null)
}

export const app = new AppModel()

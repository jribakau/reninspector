import { api, errorText } from './api'
import { fileOfNode, hasFile, labelAt, nodeByName } from './indexes.svelte'
import { app, emptyLive } from './model.svelte'
import { goTo, openBottom } from './nav.svelte'
import { stageScriptsReloaded } from './stage.svelte'
import { notify } from './toast.svelte'
import { ensureTrusted } from './trust.svelte'
import type { LiveState } from './types'

const watchKey = (root: string) => `vnide.watch.${root}`

export function loadWatch(root: string): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(watchKey(root)) ?? '[]') as unknown
    return Array.isArray(raw) ? raw.filter((n): n is string => typeof n === 'string') : []
  } catch {
    return []
  }
}

export function applyLive(s: LiveState) {
  const was = app.live.running
  let label = s.label
  if (!label && s.file && s.line > 0 && hasFile(s.file)) {
    const node = labelAt(s.file, s.line)
    if (node && (node.kind === 'label' || node.kind === 'menu' || node.kind === 'screen')) label = node.id
  }
  app.live = {
    ...s,
    label,
    vars: s.vars ?? {},
    showing: s.showing ?? [],
    warpNotes: s.warpNotes ?? [],
    receivedAt: Date.now(),
  }
  if (s.running && !was) app.visitedLabels = []
  if (s.running && label && !app.visitedLabels.includes(label)) {
    app.visitedLabels = [...app.visitedLabels, label]
  }
  if (s.note && (s.running || was)) app.notice = s.note
  if (!s.running || !app.followGame || !s.file || s.line < 1 || !hasFile(s.file)) return
  const node = labelAt(s.file, s.line)
  const onFile = !app.activeEditor || app.activeEditor.startsWith('file:')
  // A flow, map, or diff tab stays where the user put it. Markers still update above.
  if (!onFile) {
    if (node && node.id !== app.selectedLabel) app.selectedLabel = node.id
    return
  }
  if (followPaused()) return
  if (app.loc?.file !== s.file || app.loc.line !== s.line) {
    const seq = app.loc?.seq
    goTo(s.file, s.line, s.line, { flow: false, history: false, evict: false })
    if (app.loc?.seq === seq) return
  }
  if (node && node.id !== app.selectedLabel) app.selectedLabel = node.id
}

/** Lets follow move the editor again after typing paused it, and catches up to the game's line. */
export function resumeFollow() {
  app.followHold = false
  const s = app.live
  if (s.running && s.file && s.line > 0) applyLive(s)
}

/** Follow is waiting: the user is typing, or the file they are in has unsaved edits. */
export function followPaused(): boolean {
  if (!app.followGame || !app.live.running) return false
  if (app.followHold) return true
  const active = app.activeEditor
  if (active?.startsWith('file:')) {
    const path = active.slice('file:'.length)
    if (app.dirtyFiles.includes(path)) return true
  }
  return false
}

export async function runGame() {
  if (!app.info || !(await ensureTrusted())) return
  app.error = ''
  app.busy = 'Starting game…'
  try {
    const r = await api.launchGame(app.launcher)
    notify(['Started game.', ...r.notes].join(' '), 'ok')
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

function cursorTarget(): { file: string; line: number } | null {
  let file = app.cursor?.file
  let line = app.cursor?.line
  if (!file || !line) {
    const node = app.selectedLabel ? nodeByName(app.selectedLabel) : undefined
    file = node ? (fileOfNode(node) ?? undefined) : undefined
    line = node?.line
  }
  if (!file || !line) return null
  return { file, line }
}

export async function playFromCursor() {
  await jumpGameHere()
}

/** Run the story from the start and stop at the cursor, answering menus on the way. */
export async function replayToCursor() {
  if (!app.info) return
  const at = cursorTarget()
  if (!at) {
    app.error = 'Click a line first.'
    return
  }
  if (!(await ensureTrusted())) return
  app.error = ''
  app.replayBlocked = false
  app.busy = 'Planning a path from the start…'
  try {
    const r = await api.liveReplay(at.file, at.line, app.launcher, app.watchVars)
    app.replayAssumptions = r.assumptions ?? []
    app.live = {
      ...app.live,
      running: true,
      replay: 'pending',
      replayReason: '',
      note: r.notes.join(' '),
      receivedAt: Date.now(),
    }
    app.notice = r.notes.join(' ')
    openBottom('live')
  } catch (e) {
    app.replayBlocked = true
    app.replayAssumptions = []
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function toggleLive() {
  if (!app.info) return
  if (app.live.running) {
    try {
      await api.liveStop()
      app.live = { ...emptyLive(), receivedAt: Date.now() }
      app.replayAssumptions = []
      app.replayBlocked = false
      app.notice = 'Live preview stopped.'
    } catch (e) {
      app.error = errorText(e)
    }
    return
  }
  await startLive(null)
}

export async function jumpGameHere() {
  if (!app.info) return
  const at = cursorTarget()
  if (!at) {
    app.error = 'Click a line first.'
    return
  }
  if (app.live.running && !app.live.canWarp) {
    app.error = 'This session cannot warp to a line. Stop live, or use Replay to line.'
    return
  }
  if (!app.live.running) {
    await startLive(at)
    return
  }
  app.error = ''
  app.replayBlocked = false
  try {
    const r = await api.liveJump(at.file, at.line)
    app.replayAssumptions = []
    app.notice = r.notes.join(' ')
  } catch (e) {
    app.error = errorText(e)
  }
}

export async function jumpLiveLabel(name: string) {
  const next = name.trim()
  if (!app.live.running || !next) return
  app.error = ''
  try {
    const r = await api.liveJumpLabel(next)
    app.replayAssumptions = []
    app.notice = r.notes.join(' ')
  } catch (e) {
    app.error = errorText(e)
  }
}

export async function reloadLive() {
  if (!app.live.running || !app.live.canReload) return
  app.error = ''
  try {
    app.notice = await api.liveReload()
    stageScriptsReloaded()
  } catch (e) {
    app.error = errorText(e)
  }
}

async function startLive(at: { file: string; line: number } | null) {
  if (!(await ensureTrusted())) return
  app.error = ''
  app.busy = 'Starting live preview…'
  try {
    const r = await api.liveStart(app.launcher, at?.file ?? null, at?.line ?? null, app.watchVars)
    app.live = { ...app.live, running: true, note: r.notes.join(' '), receivedAt: Date.now() }
    app.notice = r.notes.join(' ')
    openBottom('live')
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export function toggleFollowGame() {
  app.followGame = !app.followGame
}

export async function setWatchVars(names: string[]) {
  const trimmed = names.map((n) => n.trim()).filter(Boolean)
  const unique = [...new Set(trimmed.filter((n) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(n)))]
  if (unique.length !== new Set(trimmed).size) {
    app.error = 'Watch names are letters, digits and underscores.'
  }
  app.watchVars = unique
  if (app.info) localStorage.setItem(watchKey(app.info.root), JSON.stringify(unique))
  if (!app.live.running) return
  try {
    await api.liveSetWatch(unique)
  } catch (e) {
    app.error = errorText(e)
  }
}

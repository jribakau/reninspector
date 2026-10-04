import { api, errorText } from './api'
import { impactSummary, reloadEditor } from './edit.svelte'
import { app } from './model.svelte'
import { pullEditStatus, reloadAll } from './reload.svelte'
import { stageScriptsReloaded } from './stage.svelte'
import type { SceneReport, StmtSpec } from './types'

export interface SceneChange {
  op: string
  path?: string
  line?: number
  speaker?: string
  text?: string
  target?: string
  targetLine?: number
  name?: string
  color?: string
  source?: string
  newLabel?: boolean
  place?: 'before' | 'after' | 'into'
  dir?: -1 | 1
  cond?: string
  how?: 'jump' | 'call' | 'choice' | 'return'
  spec?: StmtSpec
}

let focusSeq = 0

/** Ops after which the flow reopens its editor on the line they touched. */
function opensEditor(op: string): boolean {
  return op === 'add-stmt' || op === 'add-choice' || op === 'duplicate-stmt' || op === 'add-say'
}

/** Refresh the project after a scene write, then say what happened. */
async function afterWrite(report: SceneReport, verb: string, open: boolean) {
  app.impact = report.impact
  app.sceneCanUndo = report.canUndo
  app.sceneCanRedo = report.canRedo
  const info = await api.projectInfo()
  if (info) await reloadAll(info)
  await pullEditStatus()
  reloadEditor(report.path)
  if (report.focusLine > 0) app.flowFocus = { file: report.path, line: report.focusLine, seq: ++focusSeq, open }
  let extra = ''
  if (app.live.running) {
    try {
      extra = ` ${await api.liveReload()}`
      stageScriptsReloaded()
    } catch (e) {
      app.error = errorText(e)
    }
  }
  app.notice = `${verb} ${report.path}. ${impactSummary(report.impact)}${extra}`
}

/** Write one scene change into a script and refresh the project. */
export async function sceneEditReport(change: SceneChange): Promise<SceneReport | null> {
  const path = change.path ?? ''
  if (path && app.dirtyFiles.includes(path)) {
    app.error = `${path} has unsaved edits in the code view. Save or revert them first.`
    return null
  }
  app.error = ''
  app.busy = 'Saving…'
  try {
    const report = await api.sceneEdit(change)
    await afterWrite(report, 'Saved', opensEditor(change.op))
    return report
  } catch (e) {
    app.error = errorText(e)
    return null
  } finally {
    app.busy = ''
  }
}

export async function sceneEdit(change: SceneChange): Promise<boolean> {
  return (await sceneEditReport(change)) !== null
}

async function step(undo: boolean): Promise<boolean> {
  if (app.busy) return false
  if (app.dirtyFiles.length) {
    app.error = 'Save or revert the unsaved edits in the code view first.'
    return false
  }
  app.error = ''
  app.busy = undo ? 'Undoing…' : 'Redoing…'
  try {
    const report = await (undo ? api.sceneUndo() : api.sceneRedo())
    await afterWrite(report, undo ? 'Undid the last flow edit in' : 'Redid the flow edit in', false)
    return true
  } catch (e) {
    app.error = errorText(e)
    await refreshSceneHistory()
    return false
  } finally {
    app.busy = ''
  }
}

export const sceneUndo = () => step(true)
export const sceneRedo = () => step(false)

export async function refreshSceneHistory() {
  try {
    const h = await api.sceneHistory()
    app.sceneCanUndo = h.canUndo
    app.sceneCanRedo = h.canRedo
  } catch {
    app.sceneCanUndo = false
    app.sceneCanRedo = false
  }
}

let audioRoot = ''
let audioCache: Promise<string[]> | null = null

/** Audio files of the project (paths relative to game/), for the staging pickers. */
export function audioFiles(): Promise<string[]> {
  const root = app.info?.root ?? ''
  if (!audioCache || audioRoot !== root) {
    audioRoot = root
    audioCache = api
      .assetReport()
      .then((r) => r.files.filter((f) => f.kind === 'audio').map((f) => f.path).sort())
      .catch(() => [])
  }
  return audioCache
}

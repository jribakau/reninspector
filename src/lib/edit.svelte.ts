import { confirm as confirmDialog } from '@tauri-apps/plugin-dialog'
import { api, errorText, readFileText } from './api'
import { ask, askText } from './dialog.svelte'
import { app, editorTabId } from './model.svelte'
import { createEntry, deleteEntry, gameTreePath, renameEntry } from './fileops.svelte'
import { goTo, promoteTab } from './nav.svelte'
import { pullEditStatus, reloadAll } from './reload.svelte'
import { stageScriptsReloaded } from './stage.svelte'
import type { EditImpact } from './types'

import { registerBufferSave, saveBuffer } from './buffers'
export { registerBufferSave, saveBuffer }

let saveHandler: (() => void) | null = null
let reloadSeq = 0

export interface EditorCommands {
  undo: () => void
  redo: () => void
  find: () => void
  replace: () => void
  gotoLine: () => void
  selectAll: () => void
  goto: () => void
  refs: () => void
  rename: () => void
  toggleComment: () => void
  selectNext: () => void
  moveLineUp: () => void
  moveLineDown: () => void
  fold: () => void
  unfold: () => void
  format: () => void
  formatSelection: () => void
}

let editorCommands = $state<EditorCommands | null>(null)

export function registerSave(fn: (() => void) | null) {
  saveHandler = fn
}

export function registerEditor(cmds: EditorCommands | null) {
  editorCommands = cmds
}

/** True while a script buffer is the active tab. */
export function hasEditor(): boolean {
  return editorCommands !== null && !!app.loc?.file && !!app.activeEditor?.startsWith('file:')
}

export function runEditor(name: keyof EditorCommands) {
  editorCommands?.[name]()
}

export function requestSave() {
  saveHandler?.()
}

/** False when the user keeps unsaved buffers instead of discarding them. */
export async function confirmDiscard(): Promise<boolean> {
  if (!app.dirtyFiles.length) return true
  // The dialog plugin, not window.confirm. A browser confirm inside the
  // window's close handler never returns, so the window stays open.
  return confirmDialog(`You have unsaved changes in ${app.dirtyFiles.join(', ')}. Discard them?`, {
    title: "Ren'Inspector",
    kind: 'warning',
  })
}

export function setDirty(file: string, dirty: boolean) {
  const has = app.dirtyFiles.includes(file)
  if (dirty && !has) app.dirtyFiles = [...app.dirtyFiles, file]
  else if (!dirty && has) {
    app.dirtyFiles = app.dirtyFiles.filter((f) => f !== file)
    // Saving or reverting ends the edit, so follow stops waiting on it.
    app.followHold = false
  }
  if (dirty) promoteTab(editorTabId({ kind: 'file', path: file }))
}

export function reloadEditor(path: string) {
  reloadSeq += 1
  app.reloadFile = { path, seq: reloadSeq }
}

export function impactSummary(impact: EditImpact): string {
  const parts: string[] = []
  if (impact.added.length) {
    const first = impact.added.find((d) => d.severity === 'error') ?? impact.added[0]
    parts.push(
      `${impact.added.length} new problem${impact.added.length === 1 ? '' : 's'}: ${first.message}`,
    )
  }
  if (impact.removed.length) {
    parts.push(`${impact.removed.length} problem${impact.removed.length === 1 ? '' : 's'} fixed`)
  }
  if (impact.labelsAdded.length) parts.push(`added ${impact.labelsAdded.join(', ')}`)
  if (impact.labelsRemoved.length) parts.push(`removed ${impact.labelsRemoved.join(', ')}`)
  if (impact.becameUnreachable.length) {
    parts.push(`${impact.becameUnreachable.length} now unreachable`)
  }
  if (impact.becameReachable.length) parts.push(`${impact.becameReachable.length} now reachable`)
  return parts.length ? parts.join(' · ') : 'Nothing else in the project changed.'
}

export async function saveFile(path: string, text: string): Promise<boolean> {
  app.error = ''
  app.busy = 'Saving…'
  try {
    const impact = await api.writeFile(path, text)
    app.impact = impact
    const info = await api.projectInfo()
    if (info) await reloadAll(info)
    await pullEditStatus()
    setDirty(path, false)
    let extra = ''
    if (app.live.running) {
      try {
        extra = ` ${await api.liveReload()}`
        stageScriptsReloaded()
      } catch (e) {
        app.error = errorText(e)
      }
    }
    app.notice = `Saved ${path}. ${impactSummary(impact)}${extra}`
    return true
  } catch (e) {
    app.error = errorText(e)
    return false
  } finally {
    app.busy = ''
  }
}

export async function revertFile(path: string) {
  if (!confirm(`Revert ${path} to the copy saved before the IDE first changed it?`)) return
  app.error = ''
  app.busy = 'Reverting…'
  try {
    const impact = await api.revertFile(path)
    app.impact = impact
    const info = await api.projectInfo()
    if (info) await reloadAll(info)
    await pullEditStatus()
    setDirty(path, false)
    reloadSeq += 1
    app.reloadFile = { path, seq: reloadSeq }
    app.notice = `Reverted ${path}. ${impactSummary(impact)}`
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

/** Scripted revert (`VN_IDE_ACTION=revert`, path in `VN_IDE_REVERT`). No confirm dialog. */
export async function revertStartup() {
  const path = await api.initialRevert().catch(() => null)
  if (!path) return
  app.busy = 'Reverting…'
  try {
    const impact = await api.revertFile(path)
    app.impact = impact
    const info = await api.projectInfo()
    if (info) await reloadAll(info)
    await pullEditStatus()
    reloadSeq += 1
    app.reloadFile = { path, seq: reloadSeq }
    app.notice = `Reverted ${path}. ${impactSummary(impact)}`
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function applyStartupPatch() {
  const patch = await api.initialPatch().catch(() => null)
  if (!patch) return
  const text = await readFileText(patch.path)
  if (!text.includes(patch.find)) {
    app.error = `Could not find the text to patch in ${patch.path}.`
    return
  }
  await saveFile(patch.path, text.replace(patch.find, patch.replace))
}

export function noteStale(file: string) {
  app.notice = `${file} changed on disk, but it has unsaved edits, so the editor was not reloaded.`
}

export async function createScript() {
  if (!app.info) return
  const got = await ask({
    title: 'New script',
    ok: 'Create',
    fields: [{ label: 'Path inside game/', placeholder: 'chapter2.rpy' }],
  })
  const name = got?.[0]?.trim().replaceAll('\\', '/')
  if (!name) return
  app.error = ''
  const err = await createEntry(gameTreePath(name), false)
  if (err) app.error = err
  else goTo(name, 1, 1, { open: true })
}

export async function renameScript(path: string) {
  const next = await askText(`Rename ${path} to (path inside game/)`, path, 'Rename')
  if (!next || next === path) return
  app.error = ''
  const err = await renameEntry(gameTreePath(path), gameTreePath(next.trim().replaceAll('\\', '/')), false)
  if (err) app.error = err
}

export async function deleteScript(path: string) {
  app.error = ''
  const err = await deleteEntry(gameTreePath(path), false)
  if (err) app.error = err
}

export async function renameSymbol(kind: string, oldName: string) {
  const next = await askText(`Rename ${kind} ${oldName} to`, oldName, 'Preview')
  if (!next || next.trim() === oldName) return
  app.busy = 'Checking rename…'
  app.error = ''
  try {
    const preview = await api.previewRename(kind, oldName, next.trim())
    if (!preview.hits.length && !preview.truncated) {
      app.notice = `No lines would change for ${oldName}.`
      return
    }
    app.renamePreview = {
      kind,
      oldName,
      newName: next.trim(),
      hits: preview.hits,
      fileCount: preview.fileCount,
      truncated: preview.truncated,
    }
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

/** Writes files whose text was built from a search replace. Each `before` must still match disk. */
export async function applyReplace(changes: { path: string; before: string; text: string }[]) {
  if (!changes.length) {
    app.notice = 'Nothing to replace.'
    return
  }
  app.busy = 'Replacing…'
  app.error = ''
  try {
    const impact = await api.replaceText(changes)
    app.impact = impact
    const info = await api.projectInfo()
    if (info) await reloadAll(info)
    app.changedPaths = changes.map((c) => c.path)
    app.changeSeq += 1
    app.notice = `Replaced text in ${changes.length} file${changes.length === 1 ? '' : 's'}. ${impactSummary(impact)}`
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export function cancelRename() {
  app.renamePreview = null
}

export async function applyRename() {
  const preview = app.renamePreview
  if (!preview || preview.truncated) return
  app.busy = 'Renaming…'
  app.error = ''
  try {
    const impact = await api.renameSymbol(preview.kind, preview.oldName, preview.newName)
    app.impact = impact
    const info = await api.projectInfo()
    if (info) await reloadAll(info)
    app.changedPaths = app.info?.files.map((f) => f.path) ?? []
    app.changeSeq += 1
    app.renamePreview = null
    app.notice = `Renamed ${preview.oldName} to ${preview.newName}. ${impactSummary(impact)}`
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

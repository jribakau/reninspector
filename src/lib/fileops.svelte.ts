// Create, rename, move and delete from the explorer. Each returns an error message, or null when it worked.
// Script files go through the script commands, which keep the project index and backups in step.

import { api, errorText } from './api'
import { fileInfo } from './indexes.svelte'
import { app, editorTabId, type EditorTab } from './model.svelte'
import { closeEditor, goTo, replaceTabs } from './nav.svelte'
import { reloadAll } from './reload.svelte'
import { notify } from './toast.svelte'
import { baseName, isScriptName, isUnder, joinPath, tabFate, type TabFate } from './treepaths'

/** The `game/` folder as a path inside the project, or '' when the project folder is the game folder. */
export function gameFolder(): string {
  const info = app.info
  if (!info) return 'game'
  const root = info.root.replaceAll('\\', '/').replace(/\/+$/, '')
  const game = info.gameDir.replaceAll('\\', '/').replace(/\/+$/, '')
  if (game.toLowerCase() === root.toLowerCase()) return ''
  const prefix = `${root}/`
  if (game.toLowerCase().startsWith(prefix.toLowerCase())) return game.slice(prefix.length)
  return 'game'
}

/** A tree path as a path inside game/, or null when it lies outside. */
export function inGame(path: string): string | null {
  const folder = gameFolder()
  if (!folder) return path
  if (path.toLowerCase() === folder.toLowerCase()) return ''
  const prefix = `${folder}/`
  if (path.toLowerCase().startsWith(prefix.toLowerCase())) return path.slice(prefix.length)
  return null
}

/** A path inside game/ as a tree path. */
export function gameTreePath(rel: string): string {
  const folder = gameFolder()
  return folder ? `${folder}/${rel}` : rel
}

/** A script or tree path as a tree path. */
export function toTreePath(path: string): string {
  const folder = gameFolder()
  if (!folder) return path
  const low = path.toLowerCase()
  const pref = folder.toLowerCase()
  if (low === pref || low.startsWith(`${pref}/`)) return path
  if (fileInfo(path)) return `${folder}/${path}`
  return path
}

async function afterTreeChange(notice: string) {
  const info = await api.projectInfo()
  if (info) await reloadAll(info)
  notify(notice, 'ok')
}

async function run(busy: string, work: () => Promise<void>): Promise<string | null> {
  app.busy = busy
  try {
    await work()
    return null
  } catch (e) {
    return errorText(e)
  } finally {
    app.busy = ''
  }
}

/** An unsaved script under `path`, which would be left pointing at a moved file. */
function dirtyBlock(path: string): string | null {
  const game = gameFolder()
  const hit = app.dirtyFiles.find((file) => isUnder(joinPath(game, file), path))
  return hit ? `Save or discard your changes to ${hit} first.` : null
}

interface TabPlan {
  tab: EditorTab
  fate: TabFate
}

/** Decide, before the tree changes, what each open tab becomes. */
function planTabs(from: string, to: string | null): TabPlan[] {
  const game = gameFolder()
  const plans: TabPlan[] = []
  for (const tab of app.editorTabs) {
    if (tab.kind !== 'file' && tab.kind !== 'diff') continue
    const isScript = tab.kind === 'file' && !!fileInfo(tab.path)
    const fate = tabFate(tab.path, isScript, game, from, to)
    if (fate.kind !== 'keep') plans.push({ tab, fate })
  }
  return plans
}

/** Apply the plan. Returns the new script path of the active tab when it moved. */
async function applyTabs(plans: TabPlan[]): Promise<string | null> {
  const active = app.activeEditor
  let movedActive: string | null = null
  const moved = new Map<string, EditorTab>()
  for (const { tab, fate } of plans) {
    if (fate.kind !== 'move') continue
    const next: EditorTab = tab.kind === 'diff' ? { ...tab, path: fate.path } : { kind: 'file', path: fate.path }
    moved.set(editorTabId(tab), next)
    if (editorTabId(tab) === active) {
      app.activeEditor = editorTabId(next)
      if (next.kind === 'file' && isScriptName(next.path)) movedActive = next.path
    }
  }
  if (moved.size) replaceTabs(moved)
  for (const { tab, fate } of plans) {
    if (fate.kind === 'close') await closeEditor(editorTabId(tab))
  }
  return movedActive
}

export async function createEntry(path: string, dir: boolean): Promise<string | null> {
  const rel = inGame(path)
  return run('Creating…', async () => {
    if (!dir && rel && isScriptName(path)) await api.createScript(rel, null)
    else await api.fsCreate(path, dir)
    await afterTreeChange(`Created ${path}.`)
  })
}

export async function renameEntry(from: string, to: string, isDir: boolean): Promise<string | null> {
  const blocked = dirtyBlock(from)
  if (blocked) return blocked
  const relFrom = inGame(from)
  const relTo = inGame(to)
  const script = !isDir && !!relFrom && !!relTo && isScriptName(from) && isScriptName(to) && !!fileInfo(relFrom)
  const plans = planTabs(from, to)
  const line = app.loc?.line ?? 1
  let reopen: string | null = null
  const err = await run('Renaming…', async () => {
    if (script && relFrom && relTo) await api.renameScript(relFrom, relTo)
    else await api.fsRename(from, to)
    reopen = await applyTabs(plans)
    await afterTreeChange(`Renamed ${from}.`)
  })
  if (!err && reopen) goTo(reopen, line, line, { open: true })
  return err
}

export async function moveEntries(paths: string[], destDir: string): Promise<string | null> {
  for (const path of paths) {
    const blocked = dirtyBlock(path)
    if (blocked) return blocked
  }
  const plans = paths.flatMap((path) => planTabs(path, joinPath(destDir, baseName(path))))
  const line = app.loc?.line ?? 1
  let reopen: string | null = null
  const err = await run('Moving…', async () => {
    await api.fsMove(paths, destDir)
    reopen = await applyTabs(plans)
    await afterTreeChange(paths.length === 1 ? `Moved ${paths[0]}.` : `Moved ${paths.length} items.`)
  })
  if (!err && reopen) goTo(reopen, line, line, { open: true })
  return err
}

export async function deleteEntry(path: string, isDir: boolean): Promise<string | null> {
  const blocked = dirtyBlock(path)
  if (blocked) return blocked
  const what = isDir ? `${path} and everything in it` : path
  if (!confirm(`Delete ${what}? A copy is kept in the IDE data folder.`)) return null
  const rel = inGame(path)
  const script = !isDir && !!rel && isScriptName(path) && !!fileInfo(rel)
  const plans = planTabs(path, null)
  return run('Deleting…', async () => {
    if (script && rel) await api.deleteScript(rel)
    else await api.fsDelete(path)
    await applyTabs(plans)
    await afterTreeChange(`Deleted ${path}.`)
  })
}

export function revealEntry(path: string) {
  api.fsReveal(path).catch((e) => {
    app.error = errorText(e)
  })
}

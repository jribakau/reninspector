import { api, errorText } from './api'
import { setCatalog, setDiag, setInfo, setMap } from './indexes.svelte'
import { app } from './model.svelte'
import { pruneEditors } from './nav.svelte'
import type { ProjectInfo } from './types'

export async function pullCatalog() {
  try {
    setCatalog(await api.catalog())
  } catch {
    setCatalog(null)
  }
}

export async function pullEditStatus() {
  try {
    const status = await api.editStatus()
    app.modifiedFiles = status.modified
    app.decompiledEdits = status.decompiled
  } catch {
    app.modifiedFiles = []
    app.decompiledEdits = []
  }
}

export async function reloadAll(info: ProjectInfo) {
  // A save refreshes the map. It is not an external disk change, so the code
  // view must not reload its buffer from this.
  app.changedPaths = []
  const [map, diag] = await Promise.all([api.projectMap(), api.diagnostics()])
  setInfo(info)
  setMap(map)
  setDiag(diag)
  void pullCatalog()
  pruneEditors()
  app.changeSeq += 1
}

export async function refresh(paths: string[]) {
  if (!app.info) return
  try {
    const [info, map, diag] = await Promise.all([
      api.projectInfo(),
      api.projectMap(),
      api.diagnostics(),
    ])
    if (!info) return
    setInfo(info)
    setMap(map)
    setDiag(diag)
    void pullCatalog()
    pruneEditors()
    app.changeSeq += 1
    app.notice = `Reloaded ${paths.join(', ')}`
  } catch (e) {
    app.error = errorText(e)
  }
}

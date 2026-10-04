import { api, errorText, onBuildLine, onLiveShot, onLiveState, onProjectChanged, onRpaProgress, onSdkProgress, pickFolder, pickProjectFolder, pickSdk } from './api'
import { pushBuildLine } from './build.svelte'
import { ask } from './dialog.svelte'
import { applyStartupPatch, confirmDiscard, revertStartup } from './edit.svelte'
import { checkWithEngine, runEngineLint } from './engine.svelte'
import { nodeByName, setDiag, setInfo, setMap } from './indexes.svelte'
import { applyLive, loadWatch, playFromCursor, runGame } from './live.svelte'
import { app, emptyLive } from './model.svelte'
import { openBottom, openLabelGraph, restoreSession, selectLabel } from './nav.svelte'
import { pullCatalog, pullEditStatus, refresh } from './reload.svelte'
import { installVersion, loadCatalog, noteProgress, refreshSdks, resolveEngine, sdk } from './sdk.svelte'
import { startAutosave } from './autosave.svelte'
import { resolved, setSetting, settings } from './settings.svelte'
import { noteLiveShot } from './stage.svelte'
import { markTrusted } from './trust.svelte'

const RECENT_KEY = 'vnide.recent'

let unlisten: (() => void) | null = null
let reloadTimer: ReturnType<typeof setTimeout> | null = null

function loadRecent() {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]') as string[]
    app.recent = list.slice(0, settings.maxRecent)
  } catch {
    app.recent = []
  }
}

function rememberRecent(path: string) {
  const list = [path, ...app.recent.filter((p) => p !== path)].slice(0, settings.maxRecent)
  app.recent = list
  localStorage.setItem(RECENT_KEY, JSON.stringify(list))
}

/** Read-only view of the appearance settings, so menu checks update when they change. */
export const appearance = {
  get light() {
    return resolved.light
  },
  get wrap() {
    return settings.wrap
  },
  get spell() {
    return settings.spell
  },
}

export function applyTheme(theme: 'dark' | 'light') {
  setSetting('theme', theme)
}

export function applyFontSize(px: number) {
  setSetting('fontSize', px)
}

export function toggleTheme() {
  applyTheme(resolved.light ? 'dark' : 'light')
}

export function bumpFont(delta: number) {
  applyFontSize(settings.fontSize + delta)
}

export function toggleWrap() {
  setSetting('wrap', !settings.wrap)
}

export function toggleSpell() {
  setSetting('spell', !settings.spell)
}

export async function bootstrap() {
  loadRecent()
  startAutosave()
  if (!unlisten) {
    unlisten = await onProjectChanged((payload) => {
      app.changedPaths = payload.paths
      if (reloadTimer) clearTimeout(reloadTimer)
      reloadTimer = setTimeout(() => void refresh(payload.paths), 150)
    })
    void onRpaProgress((p) => {
      app.busy = p.label
    })
    void onLiveState((s) => applyLive(s))
    void onLiveShot((shot) => noteLiveShot(shot))
    void onSdkProgress((p) => noteProgress(p))
    void onBuildLine((line) => pushBuildLine(line))
  }
  const initial = await api.initialProject().catch(() => null)
  if (initial) {
    await openProject(initial)
    const label = await api.initialLabel().catch(() => null)
    if (label) openLabelGraph(label)
    const actions = await api.initialActions().catch(() => [] as string[])
    for (const action of actions) {
      if (action === 'engine') await checkWithEngine()
      else if (action === 'lint') await runEngineLint()
      else if (action === 'problems') openBottom('problems')
      else if (action === 'run') await runGame()
      else if (action === 'play') await playFromCursor()
      else if (action === 'patch') await applyStartupPatch()
      else if (action === 'revert') await revertStartup()
    }
  }
}

export async function chooseAndOpenProject() {
  if (!confirmDiscard()) return
  const path = await pickProjectFolder()
  if (path) await openProject(path)
}

export async function openProject(path: string) {
  if (app.info && !confirmDiscard()) return
  app.opening = path
  app.busy = 'Parsing project…'
  app.error = ''
  app.notice = ''
  try {
    const info = await api.openProject(path)
    const [map, diag] = await Promise.all([api.projectMap(), api.diagnostics()])
    setInfo(info)
    setMap(map)
    setDiag(diag)
    app.editorTabs = []
    app.pinnedTabs = []
    app.activeEditor = null
    app.flowOpen = false
    app.flowDetail = false
    app.sceneExpanded = false
    app.flowFocus = null
    app.sceneCanUndo = false
    app.sceneCanRedo = false
    app.cursor = null
    app.loc = null
    app.selectedLabel = null
    await pullCatalog()
    app.autoreload = await api.autoreloadEnabled().catch(() => false)
    await resolveEngine(info)
    app.live = emptyLive()
    app.visitedLabels = []
    app.watchVars = loadWatch(info.root)
    app.dirtyFiles = []
    app.impact = null
    app.reloadFile = null
    await pullEditStatus()
    rememberRecent(path)
    const restored = settings.restoreSession && restoreSession(info.root)
    if (!restored) {
      app.activity = 'story'
      app.sidebarOpen = true
      app.flowOpen = true
    }
    if (!app.loc) {
      const start = nodeByName('start') ?? map.nodes.find((n) => n.kind !== 'missing')
      if (start) selectLabel(start.id)
    }
    let notice = `Parsed ${info.stats.lines.toLocaleString()} lines in ${info.parseMs} ms`
    if (!localStorage.getItem('vnide-edit-notice')) {
      localStorage.setItem('vnide-edit-notice', '1')
      notice += '. Editing is on. A save writes a .rpy in the game folder, and the original is kept in the IDE data folder first.'
    }
    app.notice = notice
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
    app.opening = null
  }
}

export async function createNewProject() {
  const parent = await pickFolder('Folder that will contain the new project')
  if (!parent) return
  try {
    await refreshSdks()
  } catch (e) {
    app.error = errorText(e)
    return
  }
  if (!sdk.catalog.length) void loadCatalog()
  const options = [
    { value: '', label: 'No SDK (script only)' },
    ...sdk.items.map((item) => ({
      value: item.exe,
      label: item.version ? `Ren'Py ${item.version}` : item.path,
    })),
  ]
  if (!sdk.items.length) options.push({ value: '__install__', label: 'Install latest' })
  const got = await ask({
    title: 'New project',
    note: `In ${parent}. With a Ren'Py SDK the project gets the standard GUI and runs right away.`,
    ok: 'Create',
    fields: [
      { label: 'Game name', placeholder: 'My Game' },
      { label: 'First scene', value: 'start' },
      {
        label: "Ren'Py SDK",
        value: sdk.items[0]?.exe ?? (sdk.items.length ? '' : '__install__'),
        optional: true,
        browse: pickSdk,
        options,
      },
    ],
  })
  if (!got) return
  const [name, scene, picked] = got
  app.error = ''
  try {
    let sdkExe = picked
    if (sdkExe === '__install__') {
      if (!sdk.catalog.length) await loadCatalog()
      const version = sdk.catalog[0]
      if (!version) throw new Error(sdk.catalogError || "Could not read the Ren'Py release list.")
      const installed = await installVersion(version)
      sdkExe = installed.exe
    }
    app.busy = sdkExe ? 'Creating project with the Ren\u2019Py SDK…' : 'Creating project…'
    const root = await api.createProject(parent, name, scene, sdkExe || null)
    if (sdkExe) {
      const known = sdk.items.find((item) => item.exe === sdkExe)
      const registered = known ?? (await api.sdkAdd(sdkExe))
      localStorage.setItem(`vnide.engine.${root}`, `sdk:${registered.path}`)
    }
    app.busy = ''
    await openProject(root)
    if (app.info) markTrusted(app.info.root)
    selectLabel(scene)
    app.notice = sdkExe
      ? `Created ${name}. The first scene is open, and Run uses the SDK.`
      : `Created ${name}. The first scene is open. Install a Ren'Py SDK to run it.`
  } catch (e) {
    app.error = errorText(e)
    app.busy = ''
  }
}

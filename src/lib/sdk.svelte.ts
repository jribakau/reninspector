import { api, errorText, pickFolder, pickLauncher, pickSdk } from './api'
import { ask } from './dialog.svelte'
import { app } from './model.svelte'
import type { ProjectInfo } from './types'

export interface SdkInfo {
  path: string
  version: string | null
  exe: string
  hasWeb: boolean
  source: string
  unverified: boolean
}

export const sdk = $state({
  open: false,
  folder: '',
  items: [] as SdkInfo[],
  catalog: [] as string[],
  catalogError: '',
  progress: null as { label: string; done: number; total: number } | null,
  installing: false,
  /** How this project asked to be run: auto, bundled, sdk:<path>, or custom:<path>. */
  choice: 'auto',
  kind: 'none' as 'none' | 'sdk' | 'bundled' | 'custom' | 'missing',
  label: '',
  mismatch: false,
  prompt: '',
  using: null as string | null,
})

const prompted = new Set<string>()

const engineKey = (root: string) => `vnide.engine.${root}`
const legacyKey = (root: string) => `vnide.launcher.${root}`

export function samePath(a: string, b: string) {
  return a.replaceAll('\\', '/').replace(/\/+$/, '').toLowerCase() === b.replaceAll('\\', '/').replace(/\/+$/, '').toLowerCase()
}

export function projectVersion(info: ProjectInfo): string | null {
  return info.scriptVersion ?? info.engineVersion
}

export function majorMinor(version: string | null): string | null {
  if (!version) return null
  const [major, minor] = version.split('.')
  if (!major || !minor) return null
  return `${major}.${minor}`
}

function versionKey(version: string): number[] {
  return version.split('.').map((part) => Number(part)).filter((n) => Number.isFinite(n))
}

function newest(items: SdkInfo[]): SdkInfo | null {
  const ranked = items.filter((item) => item.version)
  ranked.sort((a, b) => {
    const pa = versionKey(a.version ?? '')
    const pb = versionKey(b.version ?? '')
    const n = Math.max(pa.length, pb.length)
    for (let i = 0; i < n; i++) {
      const diff = (pb[i] ?? 0) - (pa[i] ?? 0)
      if (diff) return diff
    }
    return 0
  })
  return ranked[0] ?? items[0] ?? null
}

/** Exact version, then the same major.minor, then the newest installed SDK. */
export function matchSdk(items: SdkInfo[], version: string | null): SdkInfo | null {
  if (!items.length) return null
  if (!version) return newest(items)
  const exact = items.find((item) => item.version === version)
  if (exact) return exact
  const want = majorMinor(version)
  const same = items.filter((item) => majorMinor(item.version) === want)
  if (same.length) return newest(same)
  return newest(items)
}

export function sdkForBuild(): SdkInfo | null {
  if (sdk.kind === 'sdk' && sdk.using) {
    return sdk.items.find((item) => samePath(item.path, sdk.using ?? '')) ?? null
  }
  if (sdk.kind === 'custom' && app.launcher) {
    const custom = sdk.items.find((item) => samePath(item.exe, app.launcher ?? ''))
    if (custom) return custom
  }
  return matchSdk(sdk.items, app.info ? projectVersion(app.info) : null)
}

/** Paths already checked this session that are not an SDK launcher. */
const probedLaunchers = new Set<string>()

/**
 * Older builds stored the launcher exe at `vnide.launcher.<root>`.
 * A path that belongs to an SDK already in the list becomes `sdk:<root>`.
 * Only that legacy key is registered with `sdkAdd`. A chosen custom launcher stays custom
 * unless it already matches a listed SDK, so opening a project never adds an SDK by itself.
 */
async function storedChoice(root: string): Promise<string> {
  const stored = localStorage.getItem(engineKey(root))
  if (stored && !stored.startsWith('custom:')) return stored
  const customPath = stored?.startsWith('custom:') ? stored.slice('custom:'.length) : null
  const legacy = localStorage.getItem(legacyKey(root))
  const path = customPath ?? legacy
  if (!path) return stored ?? 'auto'
  const known = sdk.items.find((item) => samePath(item.exe, path) || samePath(item.path, path))
  if (known) {
    const choice = `sdk:${known.path}`
    localStorage.setItem(engineKey(root), choice)
    return choice
  }
  if (customPath || probedLaunchers.has(path)) return stored ?? `custom:${path}`
  try {
    const added = await api.sdkAdd(path)
    if (!sdk.items.some((item) => samePath(item.path, added.path))) {
      sdk.items = [...sdk.items, added]
    }
    const choice = `sdk:${added.path}`
    localStorage.setItem(engineKey(root), choice)
    return choice
  } catch (e) {
    const message = errorText(e)
    if (!message.includes("not a Ren'Py SDK")) return stored ?? `custom:${path}`
    probedLaunchers.add(path)
    const choice = `custom:${path}`
    localStorage.setItem(engineKey(root), choice)
    return choice
  }
}

export function applyList(list: { folder: string; items: SdkInfo[] }) {
  sdk.folder = list.folder
  sdk.items = list.items
}

export async function refreshSdks() {
  applyList(await api.sdkList())
}

export async function loadCatalog() {
  try {
    sdk.catalog = await api.sdkCatalog()
    sdk.catalogError = ''
  } catch (e) {
    sdk.catalogError = errorText(e)
  }
}

export async function openSdkManager() {
  sdk.open = true
  try {
    await refreshSdks()
  } catch (e) {
    app.error = errorText(e)
  }
  void loadCatalog()
}

export function closeSdkManager() {
  sdk.open = false
}

function useSdk(item: SdkInfo, version: string | null) {
  app.launcher = item.exe
  sdk.kind = 'sdk'
  sdk.using = item.path
  sdk.label = `Ren'Py ${item.version ?? 'SDK'}`
  const want = majorMinor(version)
  const have = majorMinor(item.version)
  sdk.mismatch = !!(want && have && want !== have)
}

/** Picks the launcher for this project and stores it on `app.launcher`. */
export async function resolveEngine(info: ProjectInfo) {
  let listFailed = false
  try {
    await refreshSdks()
  } catch (e) {
    sdk.items = []
    app.error = errorText(e)
    listFailed = true
  }
  const choice = listFailed
    ? (localStorage.getItem(engineKey(info.root)) ?? 'auto')
    : await storedChoice(info.root)
  sdk.choice = choice
  sdk.prompt = ''
  sdk.mismatch = false
  sdk.using = null
  if (listFailed) {
    app.launcher = null
    sdk.kind = 'missing'
    sdk.label = 'SDK list unreadable'
    return
  }
  const version = projectVersion(info)

  const auto = () => {
    if (info.bundledEngine) {
      app.launcher = null
      sdk.kind = 'bundled'
      sdk.label = `Game engine ${info.engineVersion ?? 'bundled'}`
      return
    }
    const match = matchSdk(sdk.items, version)
    if (match) {
      useSdk(match, version)
      return
    }
    app.launcher = null
    sdk.kind = 'missing'
    sdk.label = 'No Ren\'Py SDK'
    sdk.prompt = version
      ? `This project needs Ren'Py ${version}. Install it to run and build.`
      : 'This project has no engine of its own. Install a Ren\'Py SDK to run and build.'
  }

  if (choice.startsWith('sdk:')) {
    const item = sdk.items.find((entry) => samePath(entry.path, choice.slice(4)))
    if (item) useSdk(item, version)
    else {
      localStorage.removeItem(engineKey(info.root))
      sdk.choice = 'auto'
      sdk.prompt = 'The SDK chosen for this project is no longer installed.'
      auto()
    }
  } else if (choice === 'bundled' && info.bundledEngine) {
    app.launcher = null
    sdk.kind = 'bundled'
    sdk.label = `Game engine ${info.engineVersion ?? 'bundled'}`
  } else if (choice.startsWith('custom:')) {
    app.launcher = choice.slice('custom:'.length)
    sdk.kind = 'custom'
    sdk.label = 'Custom launcher'
  } else {
    if (choice !== 'auto') {
      localStorage.removeItem(engineKey(info.root))
      sdk.choice = 'auto'
    }
    auto()
  }

  if (sdk.kind === 'missing' && !prompted.has(info.root)) {
    prompted.add(info.root)
    sdk.open = true
    void loadCatalog()
  }
}

export async function setProjectEngine(choice: string) {
  if (!app.info) return
  if (choice === 'auto') localStorage.removeItem(engineKey(app.info.root))
  else localStorage.setItem(engineKey(app.info.root), choice)
  sdk.choice = choice
  try {
    await resolveEngine(app.info)
  } catch (e) {
    app.error = errorText(e)
  }
}

export async function chooseCustomLauncher() {
  const path = await pickLauncher()
  if (!path || !app.info) return
  await setProjectEngine(`custom:${path}`)
  app.notice = `Launcher set to ${path}`
}

export async function installVersion(version: string): Promise<SdkInfo> {
  sdk.installing = true
  sdk.progress = { label: `Downloading Ren'Py ${version}`, done: 0, total: 0 }
  app.error = ''
  try {
    const info = await api.sdkInstall(version)
    await refreshSdks()
    app.notice = info.unverified
      ? `Installed Ren'Py ${info.version ?? version}. This release had no published checksum.`
      : `Installed Ren'Py ${info.version ?? version}.`
    if (app.info && sdk.kind === 'missing') await setProjectEngine(`sdk:${info.path}`)
    return info
  } finally {
    sdk.installing = false
    sdk.progress = null
  }
}

export async function installWeb(path: string) {
  const item = sdk.items.find((entry) => samePath(entry.path, path))
  const version = item?.version ?? 'SDK'
  sdk.installing = true
  sdk.progress = { label: `Downloading Web support for ${version}`, done: 0, total: 0 }
  app.error = ''
  try {
    await api.sdkInstallWeb(path)
    await refreshSdks()
    app.notice = `Web support is installed for Ren'Py ${version}.`
  } finally {
    sdk.installing = false
    sdk.progress = null
  }
}

export async function cancelInstall() {
  await api.sdkCancel()
}

export async function changeSdkFolder() {
  const folder = await pickFolder("Folder for downloaded Ren'Py SDKs")
  if (!folder) return
  applyList(await api.sdkSetFolder(folder))
  app.notice = `SDKs are kept in ${folder}`
}

export async function addExistingSdk() {
  const path = await pickSdk()
  if (!path) return
  const info = await api.sdkAdd(path)
  await refreshSdks()
  app.notice = `Added Ren'Py ${info.version ?? info.path}`
}

export async function removeSdk(item: SdkInfo) {
  const deleting = item.source === 'downloaded'
  const got = await ask({
    title: `Remove Ren'Py ${item.version ?? 'SDK'}?`,
    note: deleting
      ? 'The SDK files in the SDK folder will be deleted.'
      : 'The IDE forgets this SDK. The files stay where they are.',
    ok: 'Remove',
    fields: [],
  })
  if (!got) return
  applyList(await api.sdkRemove(item.path, deleting))
  if (app.info) await resolveEngine(app.info)
}

export function noteProgress(progress: { label: string; done: number; total: number }) {
  sdk.progress = progress
}

export function installProgressText(progress: { label: string; done: number; total: number }) {
  if (!progress.total) return progress.label
  const pct = Math.min(100, Math.round((progress.done / progress.total) * 100))
  return `${progress.label} ${pct}%`
}

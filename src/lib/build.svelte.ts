import { api, errorText, pickFolder } from './api'
import { app } from './model.svelte'
import { openBottom } from './nav.svelte'
import { installWeb, loadCatalog, majorMinor, projectVersion, sdk, sdkForBuild } from './sdk.svelte'
import { notify } from './toast.svelte'
import { ensureTrusted } from './trust.svelte'

export interface BuildLine {
  stream: string
  text: string
}

export const buildUi = $state({
  dialog: false,
  lines: [] as BuildLine[],
  running: false,
  status: '',
  dest: '',
  pc: true,
  mac: false,
  web: false,
  launch: false,
  output: '',
  message: '',
  /** Bumped on every output line, including progress lines replaced in place. */
  rev: 0,
  progress: null as { label: string; cur: number; total: number } | null,
})

export function openBuildDialog() {
  if (!app.info) return
  buildUi.message = ''
  buildUi.dialog = true
}

export function closeBuildDialog() {
  buildUi.dialog = false
}

/** "Writing the pc zip package. - 13 of 1553" and the same shape for other packages. */
const progressLine = /^(.*) - (\d+) of (\d+)\s*$/

export function pushBuildLine(line: BuildLine) {
  const match = line.text.match(progressLine)
  const last = buildUi.lines.at(-1)
  if (match && last && last.stream === line.stream) {
    const previous = last.text.match(progressLine)
    if (previous && previous[1] === match[1]) {
      buildUi.lines = [...buildUi.lines.slice(0, -1), line]
      buildUi.progress = { label: match[1], cur: Number(match[2]), total: Number(match[3]) }
      buildUi.rev += 1
      return
    }
  }
  if (match) buildUi.progress = { label: match[1], cur: Number(match[2]), total: Number(match[3]) }
  buildUi.rev += 1
  const next = buildUi.lines.length > 4000 ? buildUi.lines.slice(-3500) : buildUi.lines
  buildUi.lines = [...next, line]
}

export function defaultDistFolder(): string {
  if (!app.info) return ''
  const root = app.info.root.replace(/[\\/]+$/, '')
  const slash = Math.max(root.lastIndexOf('/'), root.lastIndexOf('\\'))
  const parent = slash >= 0 ? root.slice(0, slash) : root
  const name = slash >= 0 ? root.slice(slash + 1) : root
  const sep = root.includes('\\') ? '\\' : '/'
  return `${parent}${sep}${name}-dists`
}

export function buildTarget() {
  return sdkForBuild()
}

export function buildMismatch(): boolean {
  const target = sdkForBuild()
  if (!target || !app.info) return false
  const want = majorMinor(projectVersion(app.info))
  const have = majorMinor(target.version)
  return !!(want && have && want !== have)
}

function lastLine(text: string) {
  const lines = text.split('\n').map((line) => line.trim()).filter(Boolean)
  return lines[lines.length - 1] || 'Build finished.'
}

export async function startBuild() {
  const target = sdkForBuild()
  if (!target) {
    app.error = "Install a Ren'Py SDK before building."
    buildUi.dialog = false
    sdk.open = true
    void loadCatalog()
    return
  }
  const packages: string[] = []
  if (buildUi.pc) packages.push('pc')
  if (buildUi.mac) packages.push('mac')
  if (!packages.length && !buildUi.web) {
    buildUi.message = 'Choose what to build.'
    return
  }
  buildUi.dialog = false
  if (!(await ensureTrusted())) return
  buildUi.lines = []
  buildUi.running = true
  buildUi.status = 'Building…'
  buildUi.output = buildUi.dest.trim() || defaultDistFolder()
  app.error = ''
  openBottom('build')
  const dest = buildUi.dest.trim() || null
  let failed = false
  try {
    if (buildUi.web && !target.hasWeb) {
      buildUi.status = 'Downloading Web support…'
      await installWeb(target.path)
    }
    if (packages.length) {
      buildUi.status = 'Building…'
      const text = await api.buildPc(target.path, dest, packages)
      buildUi.status = lastLine(text)
    }
    if (buildUi.web) {
      buildUi.status = buildUi.launch ? 'Serving the web build. Stop ends it.' : 'Building for web…'
      const text = await api.buildWeb(target.path, dest, buildUi.launch)
      buildUi.status = lastLine(text)
    }
  } catch (e) {
    failed = true
    const message = errorText(e)
    buildUi.status = message.split('\n').find((line) => line.trim()) || 'Build failed.'
    app.error = message.split('\n').slice(-4).join(' ')
  } finally {
    buildUi.running = false
  }
  if (!failed) {
    notify(buildUi.status || 'Build finished.', 'ok', {
      action: { label: 'Show log', run: () => openBottom('build') },
    })
  }
}

export async function stopBuild() {
  buildUi.status = 'Stopping…'
  await api.buildCancel()
}

export function clearBuildLog() {
  if (buildUi.running) return
  buildUi.lines = []
  buildUi.progress = null
  buildUi.rev += 1
}

export async function showBuildFolder() {
  const path = buildUi.output || buildUi.dest.trim() || defaultDistFolder()
  if (!path) return
  try {
    await api.revealPath(path)
  } catch (e) {
    app.error = errorText(e)
  }
}

export async function browseBuildDest() {
  const folder = await pickFolder('Folder for the built files')
  if (folder) buildUi.dest = folder
}

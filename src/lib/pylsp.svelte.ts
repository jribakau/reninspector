import { listen } from '@tauri-apps/api/event'
import { api, errorText } from './api'
import { PY2_COMPAT_IMPORT, toVirtual } from './editor/pyvirtual'
import { app } from './model.svelte'
import { settings } from './settings.svelte'
import { notify as toast } from './toast.svelte'

export type PyStatus = 'off' | 'missing' | 'installing' | 'starting' | 'ready' | 'failed'

export const py = $state({
  status: 'off' as PyStatus,
  detail: '',
  installed: false,
  version: '',
  progress: null as { phase: string; done: number; total: number; label: string } | null,
})

export interface PyDiag {
  line: number
  message: string
  severity: 'error' | 'warning' | 'info'
  code: string
}

export interface PyItem {
  label: string
  detail: string
  type: string
  info?: string
}

export interface PySignature {
  label: string
  active: number
  params: string[]
}

/** Bumped when ty publishes diagnostics, so the editor re-reads them. */
export const pyDiags = $state({ seq: 0 })

const diags = new Map<string, PyDiag[]>()
const open = new Map<string, { uri: string; version: number; text: string }>()
const waiters = new Map<number, (result: unknown) => void>()
let nextId = 1
let current = ''
let env: { pythonVersion: string | null; sdkRoot: string | null; python2: boolean; compatDir: string } | null = null
let bound = false
let chain: Promise<void> = Promise.resolve()
/** Text waiting for the debounce, so a request can send it first. */
const pending = new Map<string, string>()
const timers = new Map<string, ReturnType<typeof setTimeout>>()
/** The download is tried once per time the setting is switched on, not on every refresh. */
let installTried = false
/** The project root the running server was started for. */
let startedRoot = ''

function forget() {
  open.clear()
  diags.clear()
  pending.clear()
  for (const timer of timers.values()) clearTimeout(timer)
  timers.clear()
  for (const resolve of waiters.values()) resolve(null)
  waiters.clear()
  pyDiags.seq += 1
}

const KIND: Record<number, string> = {
  2: 'method',
  3: 'function',
  4: 'function',
  5: 'property',
  6: 'variable',
  7: 'class',
  9: 'namespace',
  14: 'keyword',
}

export function setPyFile(path: string) {
  current = path
}

export function pythonDiagnostics(file: string): PyDiag[] {
  return diags.get(norm(file)) ?? []
}

export async function bindPython() {
  if (bound) return
  bound = true
  await listen('pylsp:message', (event) => onMessage(event.payload))
  await listen<{ phase: string; done: number; total: number; label: string }>('pylsp:progress', (event) => {
    py.progress = event.payload
  })
  await listen<{ state: string; detail?: string }>('pylsp:status', (event) => {
    if (event.payload.state === 'failed') {
      py.status = 'failed'
      py.detail = event.payload.detail ?? 'The language server stopped.'
    } else if (event.payload.state === 'restarted') {
      py.status = 'starting'
      syncPython(true, true)
    }
  })
}

/**
 * Start, stop or reconfigure to match the setting and the open project.
 * `restarted` means the backend already relaunched ty, so only the handshake is left.
 */
export function syncPython(force = false, restarted = false) {
  chain = chain.then(() => runSync(force, restarted)).catch((e) => {
    py.status = 'failed'
    py.detail = errorText(e)
  })
}

export async function installPython(force = false) {
  installTried = true
  // A running ty.exe cannot be replaced on Windows, and an update starts it fresh anyway.
  await api.pylspStop()
  forget()
  py.status = 'installing'
  py.detail = ''
  py.progress = { phase: 'download', done: 0, total: 0, label: 'Starting the download' }
  try {
    await api.pylspInstall(force)
    py.installed = true
    py.progress = null
    toast('Python language server installed.', 'ok')
    syncPython(true)
  } catch (e) {
    py.progress = null
    const message = errorText(e)
    if (message.toLowerCase().includes('cancel')) {
      py.status = py.installed ? 'off' : 'missing'
      py.detail = 'Download cancelled.'
      return
    }
    py.status = 'failed'
    py.detail = message
  }
}

export async function removePython() {
  await api.pylspStop()
  await api.pylspRemove()
  forget()
  installTried = true
  py.installed = false
  py.detail = ''
  py.status = settings.pythonServer === 'on' ? 'missing' : 'off'
}

export function cancelPython() {
  void api.pylspCancel()
}

/** Debounced full sync of the Python-only copy of `rel`. */
export function syncDocument(rel: string, text: string) {
  if (py.status !== 'ready' || text.length > 500_000 || !/\.rpym?$/i.test(rel)) return
  const prev = timers.get(rel)
  if (prev) clearTimeout(prev)
  pending.set(rel, text)
  timers.set(rel, setTimeout(() => {
    timers.delete(rel)
    const latest = pending.get(rel)
    pending.delete(rel)
    if (latest != null) pushDocument(rel, latest)
  }, 250))
}

/** Send any text still waiting for the debounce, so an answer matches what is on screen. */
function flushDocuments() {
  for (const [rel, text] of pending) {
    const timer = timers.get(rel)
    if (timer) clearTimeout(timer)
    timers.delete(rel)
    pushDocument(rel, text)
  }
  pending.clear()
}

export async function pyComplete(line: number, character: number): Promise<PyItem[]> {
  if (py.status !== 'ready' || !current) return []
  const result = await request('textDocument/completion', {
    textDocument: { uri: uriFor(current) },
    position: { line, character },
  })
  const items = Array.isArray(result) ? result : (result as { items?: unknown[] } | null)?.items ?? []
  return items.slice(0, 80).map((raw) => {
    const item = raw as { label?: unknown; detail?: unknown; kind?: number; documentation?: unknown }
    const info = textOf(item.documentation)
    return {
      label: String(item.label ?? ''),
      detail: item.detail ? String(item.detail) : 'python',
      type: KIND[item.kind ?? 0] ?? 'text',
      info: info || undefined,
    }
  }).filter((item) => item.label)
}

export async function pyHover(line: number, character: number): Promise<string | null> {
  if (py.status !== 'ready' || !current) return null
  const result = await request('textDocument/hover', {
    textDocument: { uri: uriFor(current) },
    position: { line, character },
  })
  if (!result || typeof result !== 'object') return null
  return textOf((result as { contents?: unknown }).contents)
}

export async function pySignature(line: number, character: number): Promise<PySignature | null> {
  if (py.status !== 'ready' || !current) return null
  const result = await request('textDocument/signatureHelp', {
    textDocument: { uri: uriFor(current) },
    position: { line, character },
  }) as { signatures?: { label?: string; parameters?: { label?: string | number[] }[] }[]; activeParameter?: number } | null
  const sig = result?.signatures?.[0]
  if (!sig?.label) return null
  const params = (sig.parameters ?? []).map((p) => Array.isArray(p.label) ? sig.label!.slice(p.label[0], p.label[1]) : String(p.label ?? ''))
  return { label: sig.label, active: result?.activeParameter ?? 0, params }
}

/** A project file, or `external` when the definition is in the SDK or the standard library. */
export async function pyDefinition(line: number, character: number): Promise<null | 'external' | { path: string; line: number }> {
  if (py.status !== 'ready' || !current) return null
  const result = await request('textDocument/definition', {
    textDocument: { uri: uriFor(current) },
    position: { line, character },
  })
  const loc = Array.isArray(result) ? result[0] : result
  const uri = loc && typeof loc === 'object' ? String((loc as { uri?: string }).uri ?? '') : ''
  const range = loc && typeof loc === 'object' ? (loc as { range?: { start?: { line?: number } }; targetUri?: string; targetRange?: { start?: { line?: number } } }) : null
  const target = uri || String(range?.targetUri ?? '')
  const start = range?.range?.start?.line ?? range?.targetRange?.start?.line
  if (!target || start == null) return null
  const abs = uriToPath(target)
  if (!abs) return 'external'
  const rel = inGame(abs)
  if (rel == null) return 'external'
  return { path: rel, line: start + 1 }
}

async function runSync(force: boolean, restarted: boolean) {
  await bindPython()
  if (settings.pythonServer !== 'on') {
    installTried = false
    await api.pylspStop()
    forget()
    py.status = 'off'
    py.detail = ''
    return
  }
  const status = await api.pylspStatus()
  py.installed = status.installed
  py.version = status.version
  if (!status.installed) {
    py.status = 'missing'
    py.detail = ''
    // Download once per switch-on. A failed or cancelled download waits for the button.
    if (!installTried) await installPython(false)
    return
  }
  if (!app.info) {
    await api.pylspStop()
    forget()
    startedRoot = ''
    py.status = 'off'
    py.detail = 'Open a project to use it.'
    return
  }
  const root = fileUri(app.info.root)
  const sameProject = root === startedRoot
  // A failure is retried by a project change or the buttons, not by every re-analysis.
  if (!force && !restarted && sameProject && py.status === 'failed') return
  if (!force && !restarted && sameProject && py.status === 'ready' && status.running) return
  const got = await api.pylspEnv()
  if (got.unsupported) {
    py.status = 'failed'
    py.detail = got.reason
    startedRoot = root
    return
  }
  env = {
    pythonVersion: got.pythonVersion,
    sdkRoot: got.sdkRoot,
    python2: got.python2,
    compatDir: got.compatDir,
  }
  const dialect = got.python2 ? got.reason : ''
  py.status = 'starting'
  py.detail = ''
  startedRoot = root
  // A second `initialize` is an error, so a running server is replaced rather than reused.
  if (status.running && !restarted) await api.pylspStop()
  forget()
  await api.pylspStart(tySettings())
  const init = await request('initialize', {
    processId: null,
    rootUri: root,
    workspaceFolders: [{ uri: root, name: app.info.name || 'game' }],
    capabilities: {
      workspace: { configuration: true },
      textDocument: {
        hover: { contentFormat: ['plaintext'] },
        completion: { completionItem: { documentationFormat: ['plaintext'] } },
        signatureHelp: { signatureInformation: { documentationFormat: ['plaintext'] } },
        definition: {},
      },
    },
    initializationOptions: { untrustedWorkspace: true, logLevel: 'info' },
  })
  if (!init) {
    await api.pylspStop()
    py.status = 'failed'
    py.detail = 'The language server did not answer.'
    return
  }
  notify('initialized', {})
  py.status = 'ready'
  py.detail = dialect
}

function tySettings() {
  return {
    configuration: {
      environment: {
        'python-version': env?.pythonVersion ?? '3.12',
        'extra-paths': [env?.sdkRoot, env?.compatDir].filter((p): p is string => !!p),
      },
      rules: {
        'unresolved-reference': 'ignore',
        'unused-parameter': 'ignore',
      },
    },
    diagnosticMode: 'openFilesOnly',
    showSyntaxErrors: true,
  }
}

function pushDocument(rel: string, text: string) {
  if (py.status !== 'ready') return
  const virt = toVirtual(text, env?.python2 ?? false)
  const uri = uriFor(rel)
  const prev = open.get(rel)
  if (prev?.text === virt) return
  const version = (prev?.version ?? 0) + 1
  open.set(rel, { uri, version, text: virt })
  if (!prev) {
    notify('textDocument/didOpen', { textDocument: { uri, languageId: 'python', version, text: virt } })
  } else {
    notify('textDocument/didChange', {
      textDocument: { uri, version },
      contentChanges: [{ text: virt }],
    })
  }
}

function onMessage(payload: unknown) {
  const msg = payload as { id?: number; method?: string; result?: unknown; params?: { uri?: string; diagnostics?: LspDiag[] } }
  if (msg.id != null && waiters.has(msg.id)) {
    const resolve = waiters.get(msg.id)!
    waiters.delete(msg.id)
    resolve(msg.result ?? null)
    return
  }
  if (msg.method === 'textDocument/publishDiagnostics' && msg.params?.uri) {
    const abs = uriToPath(msg.params.uri)
    const key = abs ? inGame(abs) ?? norm(abs) : ''
    if (!key) return
    diags.set(key, (msg.params.diagnostics ?? []).map(toDiag).filter((d) => keepDiag(key, d)))
    pyDiags.seq += 1
  }
}

interface LspDiag {
  message?: string
  severity?: number
  code?: string | { value?: string }
  range?: { start?: { line?: number } }
}

/** Drop diagnostics that only exist because a Python 2 line was blanked or the compat import could not be resolved. */
function keepDiag(rel: string, d: PyDiag): boolean {
  if (!env?.python2) return true
  const virt = open.get(rel)?.text.split('\n')[d.line - 1] ?? ''
  if (virt.length > 0 && virt.trim() === '') return false
  if (virt.includes(PY2_COMPAT_IMPORT)) return false
  const msg = d.message.toLowerCase()
  if (msg.includes('renpy7_compat')) return false
  if (msg.includes('print') && (msg.includes('parenthes') || msg.includes('statement'))) return false
  return true
}

function toDiag(d: LspDiag): PyDiag {
  const code = typeof d.code === 'string' ? d.code : d.code?.value ?? ''
  const severity = d.severity === 1 ? 'error' : d.severity === 2 ? 'warning' : 'info'
  return { line: (d.range?.start?.line ?? 0) + 1, message: d.message ?? '', severity, code }
}

function request(method: string, params: unknown): Promise<unknown> {
  if (method.startsWith('textDocument/')) flushDocuments()
  const id = nextId++
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      if (waiters.delete(id)) resolve(null)
    }, 8000)
    waiters.set(id, (result) => {
      clearTimeout(timer)
      resolve(result)
    })
    void api.pylspSend({ jsonrpc: '2.0', id, method, params }).catch(() => {
      clearTimeout(timer)
      waiters.delete(id)
      resolve(null)
    })
  })
}

function notify(method: string, params: unknown) {
  void api.pylspSend({ jsonrpc: '2.0', method, params }).catch(() => {})
}

function uriFor(rel: string): string {
  return fileUri(absPath(rel))
}

function absPath(rel: string): string {
  if (/^[A-Za-z]:[\\/]/.test(rel) || rel.startsWith('/')) return rel
  const game = (app.info?.gameDir ?? '').replace(/[\\/]$/, '')
  return `${game}/${rel.replaceAll('\\', '/')}`
}

export function fileUri(path: string): string {
  const normed = path.replaceAll('\\', '/')
  const prefixed = /^[A-Za-z]:\//.test(normed) ? `/${normed}` : normed.startsWith('/') ? normed : `/${normed}`
  // encodeURI leaves `#` and `?` alone, which would cut the path short.
  return `file://${encodeURI(prefixed).replaceAll('#', '%23').replaceAll('?', '%3F')}`
}

function uriToPath(uri: string): string | null {
  if (!uri.startsWith('file://')) return null
  let path = decodeURIComponent(uri.slice('file://'.length))
  if (/^\/[A-Za-z]:/.test(path)) path = path.slice(1)
  return path
}

function inGame(abs: string): string | null {
  const game = app.info?.gameDir
  if (!game) return null
  const a = abs.replaceAll('\\', '/').replace(/\/+$/, '')
  const g = game.replaceAll('\\', '/').replace(/\/+$/, '')
  if (a.toLowerCase() === g.toLowerCase()) return ''
  if (!a.toLowerCase().startsWith(`${g.toLowerCase()}/`)) return null
  return a.slice(g.length + 1)
}

function norm(path: string): string {
  return path.replaceAll('\\', '/').replace(/^\.\//, '')
}

function textOf(value: unknown): string | null {
  if (!value) return null
  if (typeof value === 'string') return value
  if (Array.isArray(value)) return value.map(textOf).filter(Boolean).join('\n') || null
  if (typeof value === 'object' && 'value' in value) return textOf((value as { value: unknown }).value)
  return null
}

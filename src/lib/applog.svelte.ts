import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { app } from './model.svelte'

export interface AppLogInput {
  seq: number
  tsMs: number
  level: string
  source: string
  message: string
}

export interface AppEvent extends AppLogInput {
  count: number
}

export const MAX_EVENTS = 2000

export const applog = $state({
  items: [] as AppEvent[],
  unseenErrors: 0,
})

const seen = new Set<number>()
let started = false

/** Fold one entry into the list. Returns null when `seq` was already applied. */
export function reduceEntries(items: AppEvent[], seenSeqs: Set<number>, entry: AppLogInput): AppEvent[] | null {
  if (seenSeqs.has(entry.seq)) return null
  seenSeqs.add(entry.seq)
  const last = items.at(-1)
  if (last && last.level === entry.level && last.source === entry.source && last.message === entry.message) {
    const next = items.slice()
    next[next.length - 1] = { ...last, count: last.count + 1, tsMs: entry.tsMs, seq: entry.seq }
    return next
  }
  const next = [...items, { ...entry, count: 1 }]
  return next.length > MAX_EVENTS ? next.slice(next.length - MAX_EVENTS) : next
}

export function noteEntry(entry: AppLogInput) {
  const next = reduceEntries(applog.items, seen, entry)
  if (!next) return
  applog.items = next
  if (entry.level === 'error' && !(app.bottomOpen && app.bottomTab === 'events')) {
    applog.unseenErrors += 1
  }
}

export function markEventsSeen() {
  applog.unseenErrors = 0
}

export function logUi(level: 'info' | 'warn' | 'error', source: string, message: string) {
  const text = message.trim()
  if (!text) return
  void invoke('applog_write', { level, source, message: text }).catch(() => {})
}

export async function loadApplog() {
  try {
    const rows = await invoke<AppLogInput[]>('applog_read')
    for (const row of rows) noteEntry(row)
  } catch {
    // The listener still receives entries written after this.
  }
}

export async function clearApplog() {
  applog.items = []
  applog.unseenErrors = 0
  seen.clear()
  await invoke('applog_clear').catch(() => {})
}

function reasonText(reason: unknown): string {
  if (typeof reason === 'string') return reason
  if (reason instanceof Error) return reason.message
  return 'Unhandled error'
}

export async function startApplog() {
  if (started) return
  started = true
  await listen<AppLogInput>('applog:entry', (event) => noteEntry(event.payload))
  await loadApplog()
  window.addEventListener('error', (event) => {
    logUi('error', 'ui', event.message || 'Script error')
  })
  window.addEventListener('unhandledrejection', (event) => {
    logUi('error', 'ui', reasonText(event.reason))
  })
}

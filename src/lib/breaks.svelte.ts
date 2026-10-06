import { api, errorText } from './api'
import { linesFor, togglePoint } from './breaks'
import { app } from './model.svelte'

export { linesFor }

export const breakPoints = $state({ list: [] as string[] })

const storageKey = (root: string) => `vnide.breaks.${root}`

export function loadBreaks(root: string) {
  try {
    const raw = JSON.parse(localStorage.getItem(storageKey(root)) ?? '[]') as unknown
    breakPoints.list = Array.isArray(raw) ? raw.filter((n): n is string => typeof n === 'string') : []
  } catch {
    breakPoints.list = []
  }
}

function remember() {
  if (app.info) localStorage.setItem(storageKey(app.info.root), JSON.stringify(breakPoints.list))
}

export async function syncBreaks() {
  if (!app.live.running) return
  try {
    await api.liveSetBreaks(breakPoints.list)
  } catch (e) {
    app.error = errorText(e)
  }
}

export async function toggleBreakpoint(file: string, line: number) {
  if (line < 1 || !file) return
  breakPoints.list = togglePoint(breakPoints.list, file, line)
  remember()
  await syncBreaks()
}

export async function stepLive() {
  try {
    await api.liveStep()
  } catch (e) {
    app.error = errorText(e)
  }
}

export async function resumeLive() {
  try {
    await api.liveResume()
  } catch (e) {
    app.error = errorText(e)
  }
}

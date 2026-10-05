import { app } from './model.svelte'

export type ToastKind = 'info' | 'ok' | 'warn' | 'error'

export interface ToastAction {
  label: string
  run: () => void
}

export interface Toast {
  id: number
  kind: ToastKind
  text: string
  action?: ToastAction
}

export interface NotifyOptions {
  action?: ToastAction
  /** How long the toast stays. Errors stay until dismissed when this is omitted. */
  ttl?: number
  /** Update the status bar only. Autosave uses this so it does not cover the window. */
  quiet?: boolean
}

export const toasts = $state({ items: [] as Toast[] })

const MAX_TOASTS = 3
const DEFAULT_TTL = 4000

let seq = 0
const timers = new Map<number, ReturnType<typeof setTimeout>>()
const remaining = new Map<number, number>()
const startedAt = new Map<number, number>()

function clearTimer(id: number) {
  const timer = timers.get(id)
  if (timer) clearTimeout(timer)
  timers.delete(id)
}

function arm(id: number, ms: number) {
  clearTimer(id)
  remaining.set(id, ms)
  startedAt.set(id, Date.now())
  timers.set(id, setTimeout(() => dismissToast(id), ms))
}

/** Ephemeral feedback. Also writes `app.notice`, so the status bar keeps the latest line. */
export function notify(text: string, kind: ToastKind = 'info', options: NotifyOptions = {}) {
  const message = text.trim()
  if (!message) return
  app.notice = message
  app.noticeAction = options.action
    ? { text: message, label: options.action.label, run: options.action.run }
    : null
  if (options.quiet) return

  const id = ++seq
  const toast: Toast = { id, kind, text: message, action: options.action }
  const overflow = [...toasts.items, toast]
  for (const old of overflow.slice(0, Math.max(0, overflow.length - MAX_TOASTS))) dismissToast(old.id)
  toasts.items = [...toasts.items.filter((item) => item.id !== id), toast].slice(-MAX_TOASTS)

  const ttl = options.ttl ?? (kind === 'error' ? 0 : DEFAULT_TTL)
  if (ttl > 0) arm(id, ttl)
}

export function dismissToast(id: number) {
  clearTimer(id)
  remaining.delete(id)
  startedAt.delete(id)
  if (!toasts.items.some((item) => item.id === id)) return
  toasts.items = toasts.items.filter((item) => item.id !== id)
}

/** Hover pauses the countdown so a toast can be read. */
export function pauseToast(id: number) {
  if (!timers.has(id)) return
  const began = startedAt.get(id) ?? Date.now()
  const left = remaining.get(id) ?? DEFAULT_TTL
  remaining.set(id, Math.max(400, left - (Date.now() - began)))
  clearTimer(id)
}

export function resumeToast(id: number) {
  if (!toasts.items.some((item) => item.id === id) || timers.has(id)) return
  const left = remaining.get(id)
  if (left == null) return
  arm(id, left)
}

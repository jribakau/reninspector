import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { app } from './model.svelte'
import { dismissToast, notify, pauseToast, resumeToast, toasts } from './toast.svelte'

function clearToasts() {
  for (const item of [...toasts.items]) dismissToast(item.id)
  app.notice = ''
  app.noticeAction = null
}

beforeEach(() => {
  vi.useFakeTimers()
  clearToasts()
})

afterEach(() => {
  clearToasts()
  vi.useRealTimers()
})

describe('notify', () => {
  it('keeps the latest three toasts and writes the status line', () => {
    notify('one')
    notify('two', 'ok')
    notify('three', 'warn')
    notify('four')
    expect(toasts.items.map((item) => item.text)).toEqual(['two', 'three', 'four'])
    expect(app.notice).toBe('four')
  })

  it('leaves errors until they are dismissed', () => {
    notify('boom', 'error')
    vi.advanceTimersByTime(20_000)
    expect(toasts.items.map((item) => item.text)).toEqual(['boom'])
    dismissToast(toasts.items[0].id)
    expect(toasts.items).toEqual([])
  })

  it('drops a toast after its ttl', () => {
    notify('saved', 'ok')
    vi.advanceTimersByTime(3999)
    expect(toasts.items).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(toasts.items).toEqual([])
  })

  it('pauses the countdown on hover and keeps at least a short remainder', () => {
    notify('saved', 'ok')
    vi.advanceTimersByTime(3900)
    pauseToast(toasts.items[0].id)
    vi.advanceTimersByTime(10_000)
    expect(toasts.items).toHaveLength(1)
    resumeToast(toasts.items[0].id)
    vi.advanceTimersByTime(399)
    expect(toasts.items).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(toasts.items).toEqual([])
  })

  it('updates the status bar without a toast when quiet', () => {
    notify('Autosaved.', 'info', { quiet: true })
    expect(toasts.items).toEqual([])
    expect(app.notice).toBe('Autosaved.')
    vi.advanceTimersByTime(10_000)
    expect(app.notice).toBe('Autosaved.')
  })

  it('ignores a blank message', () => {
    notify('kept', 'ok')
    notify('   ')
    expect(toasts.items.map((item) => item.text)).toEqual(['kept'])
    expect(app.notice).toBe('kept')
  })

  it('keeps an action on the toast and the status bar', () => {
    let ran = false
    notify('Build finished.', 'ok', { action: { label: 'Show log', run: () => (ran = true) } })
    expect(toasts.items[0].action?.label).toBe('Show log')
    expect(app.noticeAction?.label).toBe('Show log')
    app.noticeAction?.run()
    expect(ran).toBe(true)
  })
})

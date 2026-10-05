/**
 * Pure tab-strip rules: pinned ordering and which tabs a bulk close drops.
 * Tabs are plain ids here, so none of this needs the app state.
 */

import type { EditorTab } from './model.svelte'

/** What a tab shows as its name. */
export function tabTitle(t: EditorTab): string {
  if (t.kind === 'map') return 'Project map'
  if (t.kind === 'graph') return t.name.startsWith('screen:') ? t.name.slice(7) : t.name
  if (t.kind === 'diff') return `${t.path.split('/').pop() ?? t.path} (${t.rev === 'HEAD' ? 'diff' : t.rev.slice(0, 7)})`
  if (t.kind === 'rebase') return `${t.path.split('/').pop() ?? t.path} (rebase)`
  return t.path.split('/').pop() ?? t.path
}

export type TabOrder = { ids: string[]; pinned: string[] }

export type BulkClose = 'others' | 'right' | 'left' | 'saved' | 'all'

/** Pinned tabs first, keeping the relative order inside each block. Drops pins with no tab. */
export function settle(ids: string[], pinned: readonly string[]): TabOrder {
  const have = new Set(ids)
  const pins = new Set(pinned.filter((id) => have.has(id)))
  const head = ids.filter((id) => pins.has(id))
  const tail = ids.filter((id) => !pins.has(id))
  return { ids: [...head, ...tail], pinned: head }
}

/** Reads the pinned ids out of a saved session blob. */
export function restorePins(ids: string[], saved: unknown): TabOrder {
  const list = Array.isArray(saved) ? saved.filter((id): id is string => typeof id === 'string') : []
  return settle(ids, list)
}

/** Pins the tab and puts it at the end of the pinned block. */
export function pinTab(ids: string[], pinned: readonly string[], id: string): TabOrder {
  if (!ids.includes(id)) return settle(ids, pinned)
  const rest = ids.filter((x) => x !== id)
  const block = pinned.filter((x) => x !== id && rest.includes(x))
  const at = block.length
  const next = [...rest]
  next.splice(at, 0, id)
  return settle(next, [...block, id])
}

/** Unpins the tab and puts it first among the loose tabs. */
export function unpinTab(ids: string[], pinned: readonly string[], id: string): TabOrder {
  if (!ids.includes(id)) return settle(ids, pinned)
  const block = pinned.filter((x) => x !== id && ids.includes(x))
  const rest = ids.filter((x) => x !== id)
  const next = [...rest]
  next.splice(block.length, 0, id)
  return settle(next, block)
}

/**
 * Moves a tab so it lands before `ids[slot]` (`slot === ids.length` means the end).
 * Dropping inside the pinned block pins the tab, dropping outside it unpins the tab.
 * A drop exactly on the boundary keeps the tab on its own side.
 */
export function moveTab(ids: string[], pinned: readonly string[], id: string, slot: number): TabOrder {
  const from = ids.indexOf(id)
  if (from < 0) return settle(ids, pinned)
  const have = new Set(ids)
  const block = pinned.filter((x) => have.has(x))
  const count = block.length
  const wasPinned = block.includes(id)
  const clamped = Math.max(0, Math.min(ids.length, slot))
  const pin = wasPinned ? clamped <= count : clamped < count
  const next = ids.filter((x) => x !== id)
  const at = clamped > from ? clamped - 1 : clamped
  next.splice(Math.max(0, Math.min(next.length, at)), 0, id)
  const pins = block.filter((x) => x !== id)
  if (pin) pins.push(id)
  return settle(next, pins)
}

/** The ids a bulk close would drop. Pinned tabs are never dropped, and neither is the anchor. */
export function bulkTargets(
  ids: string[],
  pinned: readonly string[],
  dirty: readonly string[],
  mode: BulkClose,
  anchor: string | null,
): string[] {
  const at = anchor === null ? -1 : ids.indexOf(anchor)
  const pins = new Set(pinned)
  const unsaved = new Set(dirty)
  return ids.filter((id, i) => {
    if (pins.has(id) || id === anchor) return false
    switch (mode) {
      case 'others':
        return at >= 0
      case 'right':
        return at >= 0 && i > at
      case 'left':
        return at >= 0 && i < at
      case 'saved':
        return !unsaved.has(id)
      case 'all':
        return true
    }
  })
}

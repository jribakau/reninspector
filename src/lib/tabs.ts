/**
 * Pure tab-strip rules: pinned ordering, the single preview slot, recency,
 * and which tabs a bulk close drops. Tabs are plain ids here, so none of this
 * needs the app state.
 */

/** How a peek can land without closing a tab the user asked to keep. */
export type PreviewRoom = 'have' | 'replace' | 'append' | 'full'

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

/**
 * Where a peek of `newId` would land.
 * `have` — that tab is already open, so the strip stays put.
 * `replace` — it takes the current preview's place.
 * `append` — there is room for a new preview.
 * `full` — a new preview would have to close some other tab.
 * Pinned and locked tabs are never the preview.
 */
export function previewRoom(
  ids: readonly string[],
  pinned: readonly string[],
  previewId: string | null,
  newId: string,
  locked: readonly string[],
  max: number,
): PreviewRoom {
  if (ids.includes(newId)) return 'have'
  if (canReplacePreview(ids, pinned, previewId, locked)) return 'replace'
  if (ids.length < max) return 'append'
  return 'full'
}

function canReplacePreview(
  ids: readonly string[],
  pinned: readonly string[],
  previewId: string | null,
  locked: readonly string[],
): boolean {
  if (!previewId || !ids.includes(previewId)) return false
  const pins = new Set(pinned)
  const lock = new Set(locked)
  return !pins.has(previewId) && !lock.has(previewId)
}

/**
 * Puts `newId` in the preview slot. An existing preview is replaced in place.
 * A tab that is already open for real is left alone, and so is its preview.
 * Pinned and locked tabs are never the preview: a locked preview is kept and
 * the new tab is appended instead.
 */
export function placePreview(
  ids: readonly string[],
  pinned: readonly string[],
  previewId: string | null,
  newId: string,
  locked: readonly string[] = [],
): { ids: string[]; preview: string | null } {
  const pins = new Set(pinned)
  const lock = new Set(locked)
  if (ids.includes(newId) && newId !== previewId) {
    const preview = canReplacePreview(ids, pinned, previewId, locked) ? previewId : null
    return { ids: [...ids], preview }
  }
  if (ids.includes(newId)) {
    if (pins.has(newId) || lock.has(newId)) return { ids: [...ids], preview: null }
    return { ids: [...ids], preview: newId }
  }
  if (canReplacePreview(ids, pinned, previewId, locked)) {
    return { ids: ids.map((id) => (id === previewId ? newId : id)), preview: newId }
  }
  return { ids: [...ids, newId], preview: newId }
}

/**
 * Moves `id` to the front. Ids that are no longer open are dropped.
 * Open tabs the list has never seen stay at the end, so they count as oldest.
 */
export function touchMru(mru: readonly string[], open: readonly string[], id: string): string[] {
  const have = new Set(open)
  const next: string[] = []
  if (have.has(id)) next.push(id)
  for (const x of mru) {
    if (x !== id && have.has(x) && !next.includes(x)) next.push(x)
  }
  for (const x of open) {
    if (!next.includes(x)) next.push(x)
  }
  return next
}

/** The least recently used candidate. Tabs missing from `mru` are older than the ones it lists. */
export function leastRecent(mru: readonly string[], candidates: readonly string[]): string | null {
  if (!candidates.length) return null
  const want = new Set(candidates)
  const ranked = new Set(mru)
  for (const id of candidates) {
    if (!ranked.has(id)) return id
  }
  for (let i = mru.length - 1; i >= 0; i--) {
    if (want.has(mru[i])) return mru[i]
  }
  return null
}

/** The most recently used open tab, skipping `skip` when it is still open. */
export function mostRecent(mru: readonly string[], open: readonly string[], skip: string | null = null): string | null {
  const have = new Set(open)
  for (const id of mru) {
    if (id !== skip && have.has(id)) return id
  }
  for (const id of open) {
    if (id !== skip) return id
  }
  return null
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

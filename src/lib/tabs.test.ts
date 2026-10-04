import { describe, expect, it } from 'vitest'
import { bulkTargets, moveTab, pinTab, restorePins, settle, unpinTab } from './tabs'

const ids = ['a', 'b', 'c', 'd', 'e']

describe('pinned order', () => {
  it('keeps pinned tabs first and drops stale pins', () => {
    expect(settle(ids, ['c', 'x', 'a'])).toEqual({ ids: ['a', 'c', 'b', 'd', 'e'], pinned: ['a', 'c'] })
  })

  it('restores pins from a session blob', () => {
    expect(restorePins(ids, ['d', 7, 'gone'])).toEqual({ ids: ['d', 'a', 'b', 'c', 'e'], pinned: ['d'] })
    expect(restorePins(ids, undefined)).toEqual({ ids, pinned: [] })
  })

  it('pins to the end of the pinned block', () => {
    const first = pinTab(ids, [], 'c')
    expect(first).toEqual({ ids: ['c', 'a', 'b', 'd', 'e'], pinned: ['c'] })
    expect(pinTab(first.ids, first.pinned, 'e')).toEqual({ ids: ['c', 'e', 'a', 'b', 'd'], pinned: ['c', 'e'] })
  })

  it('unpins to the front of the loose tabs', () => {
    const pinned = pinTab(pinTab(ids, [], 'c').ids, ['c'], 'e')
    expect(unpinTab(pinned.ids, pinned.pinned, 'c')).toEqual({ ids: ['e', 'c', 'a', 'b', 'd'], pinned: ['e'] })
  })
})

describe('moving tabs', () => {
  it('reorders loose tabs', () => {
    expect(moveTab(ids, [], 'a', 3).ids).toEqual(['b', 'c', 'a', 'd', 'e'])
    expect(moveTab(ids, [], 'e', 0).ids).toEqual(['e', 'a', 'b', 'c', 'd'])
    expect(moveTab(ids, [], 'b', 5).ids).toEqual(['a', 'c', 'd', 'e', 'b'])
  })

  it('pins a tab dropped inside the pinned block', () => {
    const out = moveTab(ids, ['a', 'b'], 'd', 1)
    expect(out).toEqual({ ids: ['a', 'd', 'b', 'c', 'e'], pinned: ['a', 'd', 'b'] })
  })

  it('unpins a tab dropped past the pinned block', () => {
    const out = moveTab(ids, ['a', 'b'], 'a', 4)
    expect(out.pinned).toEqual(['b'])
    expect(out.ids).toEqual(['b', 'c', 'd', 'a', 'e'])
  })

  it('keeps a tab on its side when dropped on the boundary', () => {
    expect(moveTab(ids, ['a', 'b'], 'a', 2).pinned).toEqual(['b', 'a'])
    expect(moveTab(ids, ['a', 'b'], 'c', 2)).toEqual({ ids, pinned: ['a', 'b'] })
  })

  it('ignores unknown ids', () => {
    expect(moveTab(ids, [], 'zz', 1).ids).toEqual(ids)
  })
})

describe('bulk close', () => {
  const pinned = ['a']

  it('closes to the right and left of the anchor', () => {
    expect(bulkTargets(ids, [], [], 'right', 'c')).toEqual(['d', 'e'])
    expect(bulkTargets(ids, [], [], 'left', 'c')).toEqual(['a', 'b'])
    expect(bulkTargets(ids, [], [], 'right', 'e')).toEqual([])
  })

  it('closes others but keeps the anchor', () => {
    expect(bulkTargets(ids, [], [], 'others', 'c')).toEqual(['a', 'b', 'd', 'e'])
    expect(bulkTargets(ids, [], [], 'others', null)).toEqual([])
  })

  it('closes saved tabs only', () => {
    expect(bulkTargets(ids, [], ['b', 'd'], 'saved', null)).toEqual(['a', 'c', 'e'])
  })

  it('never drops pinned tabs', () => {
    expect(bulkTargets(ids, pinned, [], 'all', null)).toEqual(['b', 'c', 'd', 'e'])
    expect(bulkTargets(ids, pinned, [], 'left', 'c')).toEqual(['b'])
    expect(bulkTargets(ids, pinned, [], 'others', 'c')).toEqual(['b', 'd', 'e'])
    expect(bulkTargets(ids, pinned, [], 'saved', null)).toEqual(['b', 'c', 'd', 'e'])
  })
})

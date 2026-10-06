import { describe, expect, it } from 'vitest'
import { MAX_EVENTS, reduceEntries, type AppEvent, type AppLogInput } from './applog.svelte'

function entry(seq: number, message: string, level = 'info'): AppLogInput {
  return { seq, tsMs: seq * 1000, level, source: 'ui', message }
}

describe('reduceEntries', () => {
  it('drops the oldest entries past the cap', () => {
    const seen = new Set<number>()
    let items: AppEvent[] = []
    for (let i = 1; i <= MAX_EVENTS + 5; i++) items = reduceEntries(items, seen, entry(i, `n${i}`)) ?? items
    expect(items).toHaveLength(MAX_EVENTS)
    expect(items[0].message).toBe('n6')
    expect(items.at(-1)?.message).toBe(`n${MAX_EVENTS + 5}`)
  })

  it('ignores a sequence number it has already applied', () => {
    const seen = new Set<number>()
    const first = reduceEntries([], seen, entry(1, 'opened'))
    expect(first).toHaveLength(1)
    expect(reduceEntries(first ?? [], seen, entry(1, 'opened again'))).toBeNull()
  })

  it('collapses identical consecutive messages', () => {
    const seen = new Set<number>()
    let items = reduceEntries([], seen, entry(1, 'Autosaved.')) ?? []
    items = reduceEntries(items, seen, entry(2, 'Autosaved.')) ?? []
    items = reduceEntries(items, seen, entry(3, 'Autosaved.')) ?? []
    expect(items).toHaveLength(1)
    expect(items[0].count).toBe(3)
    expect(items[0].seq).toBe(3)
    items = reduceEntries(items, seen, entry(4, 'Saved script.rpy')) ?? []
    expect(items).toHaveLength(2)
    expect(items[1].count).toBe(1)
  })
})

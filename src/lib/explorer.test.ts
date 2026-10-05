import { describe, expect, it } from 'vitest'
import { flattenTree } from './explorer'
import { dropFolder } from './treedrag'

const file = (path: string, dir = false) => ({ path, name: path.split('/').pop() ?? path, dir })

describe('explorer tree', () => {
  it('lists open folders and the new-name row', () => {
    const rows = flattenTree(
      { '': [file('game', true), file('README')], game: [file('game/script.rpy')] },
      new Set(['game']),
      { mode: 'new', dir: false, parent: 'game' },
      '\0new',
    )
    expect(rows.map((row) => row.entry.path)).toEqual(['game', '\0new', 'game/script.rpy', 'README'])
    expect(rows[1].isNew).toBe(true)
    expect(rows[2].depth).toBe(1)
  })

  it('refuses a drop onto the current parent or into itself', () => {
    expect(dropFolder({ path: 'game/a', dir: false }, 'game')).toBeNull()
    expect(dropFolder({ path: 'game/a', dir: true }, 'game/a/b')).toBeNull()
    expect(dropFolder({ path: 'game/a', dir: false }, 'other')).toBe('other')
  })
})
import { expect, it } from 'vitest'
import { decorOf } from './git.svelte'
import type { GitChange, GitStatus } from './types'

function status(changes: GitChange[]): GitStatus {
  return {
    branch: 'main',
    upstream: null,
    ahead: 0,
    behind: 0,
    hasRemote: false,
    hasIgnore: true,
    outsideStaged: [],
    headPushed: false,
    pushRemote: null,
    changes,
  }
}

it('lets an unstaged letter win, maps new files to U, and marks every parent folder', () => {
  const decor = decorOf(
    status([
      { path: 'game/scripts/a.rpy', status: 'M', staged: true },
      { path: 'game/scripts/a.rpy', status: 'D', staged: false },
      { path: 'game/new.rpy', status: '?', staged: false },
    ]),
  )
  expect(decor.files.get('game/scripts/a.rpy')).toBe('D')
  expect(decor.files.get('game/new.rpy')).toBe('U')
  expect(decor.dirs.has('game')).toBe(true)
  expect(decor.dirs.has('game/scripts')).toBe(true)
  expect(decor.dirs.has('game/scripts/a.rpy')).toBe(false)
})

it('returns no decorations when nothing changed', () => {
  expect(decorOf(null).files.size).toBe(0)
  expect(decorOf(status([])).dirs.size).toBe(0)
})

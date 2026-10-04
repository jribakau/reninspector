import { expect, it } from 'vitest'
import { filesUnder, treeOf } from './git-tree'
import type { GitChange } from './types'

function change(path: string): GitChange {
  return { path, status: 'M', staged: false }
}

it('merges a chain of folders that contain a single child', () => {
  const tree = treeOf([change('game/scripts/a.rpy'), change('game/scripts/b.rpy')])
  expect(tree).toHaveLength(1)
  expect(tree[0].name).toBe('game/scripts')
  expect(tree[0].path).toBe('game/scripts')
  expect(tree[0].files.map((file) => file.path)).toEqual(['game/scripts/a.rpy', 'game/scripts/b.rpy'])
  expect(filesUnder(tree[0]).map((file) => file.path)).toEqual(['game/scripts/a.rpy', 'game/scripts/b.rpy'])
})

it('keeps files at the repository root beside the folders', () => {
  const tree = treeOf([change('readme.txt'), change('game/a.rpy')])
  expect(tree).toHaveLength(2)
  expect(tree[0].name).toBe('')
  expect(tree[0].files.map((file) => file.path)).toEqual(['readme.txt'])
  expect(tree[1].name).toBe('game')
  expect(filesUnder(tree[1]).map((file) => file.path)).toEqual(['game/a.rpy'])
})

it('does not merge a folder that has both files and a child', () => {
  const tree = treeOf([change('game/root.rpy'), change('game/scripts/a.rpy')])
  expect(tree).toHaveLength(1)
  expect(tree[0].name).toBe('game')
  expect(tree[0].files.map((file) => file.path)).toEqual(['game/root.rpy'])
  expect(tree[0].children).toHaveLength(1)
  expect(tree[0].children[0].name).toBe('scripts')
  expect(filesUnder(tree[0])).toHaveLength(2)
})

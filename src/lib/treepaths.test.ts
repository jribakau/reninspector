import { describe, expect, it } from 'vitest'
import { baseName, isUnder, joinPath, movedPath, parentOf, tabFate } from './treepaths'

describe('tree paths', () => {
  it('splits and joins', () => {
    expect(baseName('game/a/b.rpy')).toBe('b.rpy')
    expect(parentOf('game/a/b.rpy')).toBe('game/a')
    expect(parentOf('b.rpy')).toBe('')
    expect(joinPath('', 'x')).toBe('x')
    expect(joinPath('game', 'x')).toBe('game/x')
  })

  it('knows what is under a folder', () => {
    expect(isUnder('game/a/b', 'game/a')).toBe(true)
    expect(isUnder('game/a', 'game/a')).toBe(true)
    expect(isUnder('game/ab', 'game/a')).toBe(false)
    expect(isUnder('Game/A/b', 'game/a')).toBe(true)
  })

  it('moves a path with its folder', () => {
    expect(movedPath('game/a/b.rpy', 'game/a', 'game/c')).toBe('game/c/b.rpy')
    expect(movedPath('game/z.rpy', 'game/a', 'game/c')).toBe('game/z.rpy')
  })
})

describe('tabFate', () => {
  it('keeps unrelated tabs', () => {
    expect(tabFate('other.rpy', true, 'game', 'game/a', 'game/b')).toEqual({ kind: 'keep' })
  })

  it('renames a script tab, which is relative to game/', () => {
    expect(tabFate('a/one.rpy', true, 'game', 'game/a', 'game/b')).toEqual({ kind: 'move', path: 'b/one.rpy' })
    expect(tabFate('one.rpy', true, 'game', 'game/one.rpy', 'game/two.rpy')).toEqual({ kind: 'move', path: 'two.rpy' })
  })

  it('works when the project folder is the game folder', () => {
    expect(tabFate('a/one.rpy', true, '', 'a', 'b')).toEqual({ kind: 'move', path: 'b/one.rpy' })
  })

  it('turns a script moved out of game/ into a tree path tab', () => {
    expect(tabFate('one.rpy', true, 'game', 'game/one.rpy', 'tools/one.rpy')).toEqual({ kind: 'move', path: 'tools/one.rpy' })
  })

  it('handles non-script tabs as tree paths', () => {
    expect(tabFate('game/log.txt', false, 'game', 'game/log.txt', 'game/old.txt')).toEqual({ kind: 'move', path: 'game/old.txt' })
  })

  it('closes tabs of deleted entries', () => {
    expect(tabFate('a/one.rpy', true, 'game', 'game/a', null)).toEqual({ kind: 'close' })
    expect(tabFate('b/one.rpy', true, 'game', 'game/a', null)).toEqual({ kind: 'keep' })
  })
})

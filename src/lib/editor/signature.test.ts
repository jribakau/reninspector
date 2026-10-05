import { describe, expect, it } from 'vitest'
import { activeParam, callContext } from './signature'

describe('callContext', () => {
  it('finds the call and the active argument', () => {
    expect(callContext('Character(')).toMatchObject({ name: 'Character', activeArg: 0, keyword: null })
    expect(callContext('Character("Eileen", ')).toMatchObject({ name: 'Character', activeArg: 1 })
    expect(callContext('Character(name=')).toMatchObject({ name: 'Character', keyword: 'name' })
    expect(callContext('Fade(0.5, 0.0, ')).toMatchObject({ name: 'Fade', activeArg: 2 })
    expect(callContext('Character("a, b", ')).toMatchObject({ activeArg: 1 })
    expect(callContext('renpy.jump(')).toMatchObject({ name: 'renpy.jump', activeArg: 0 })
    expect(callContext('Character("Eileen")')).toBeNull()
  })

  it('uses a keyword argument when the name matches', () => {
    const ctx = callContext('Character("Eileen", what_color=')
    expect(ctx).not.toBeNull()
    expect(activeParam(ctx!, [{ name: 'name' }, { name: 'color' }, { name: 'what_color' }])).toBe(2)
    expect(activeParam(callContext('Dissolve(')!, [{ name: 'time' }])).toBe(0)
  })
})

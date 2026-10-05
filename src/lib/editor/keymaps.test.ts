import { describe, expect, it } from 'vitest'
import { keymapExtension, setKeymapSave } from './keymaps'

describe('keymapExtension', () => {
  it('adds nothing for the default keymap', async () => {
    expect(await keymapExtension('default')).toEqual([])
  })

  it('loads vim and emacs once, and :w calls the latest save', async () => {
    const calls: string[] = []
    setKeymapSave(() => calls.push('first'))
    const vim = await keymapExtension('vim')
    const again = await keymapExtension('vim')
    expect(vim).toBe(again)
    expect(Array.isArray(vim)).toBe(true)

    const { Vim } = await import('@replit/codemirror-vim')
    const cm = {
      operation(fn: () => void) {
        fn()
      },
      state: { vim: { visualMode: false } },
      getCursor: () => ({ line: 0, ch: 0 }),
    }
    Vim.handleEx(cm as never, 'w')
    setKeymapSave(() => calls.push('second'))
    Vim.handleEx(cm as never, 'w')
    expect(calls).toEqual(['first', 'second'])

    const emacs = await keymapExtension('emacs')
    expect(emacs).toBeTruthy()
    expect(await keymapExtension('emacs')).toBe(emacs)
  })
})

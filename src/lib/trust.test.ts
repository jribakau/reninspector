import { beforeEach, describe, expect, it } from 'vitest'
import { closeDialog, dialog } from './dialog.svelte'
import { app } from './model.svelte'
import type { ProjectInfo } from './types'
import { ensureTrusted, forgetTrust, isTrusted } from './trust.svelte'

function open(root: string) {
  app.info = { root } as ProjectInfo
}

beforeEach(() => {
  localStorage.clear()
  closeDialog(false)
  open('C:/games/demo')
})

describe('trust', () => {
  it('asks once, then remembers the project', async () => {
    expect(isTrusted()).toBe(false)
    const first = ensureTrusted()
    expect(dialog.current?.ok).toBe('Trust this game')
    closeDialog(true)
    expect(await first).toBe(true)
    expect(isTrusted()).toBe(true)
    expect(await ensureTrusted()).toBe(true)
    expect(dialog.current).toBeNull()
  })

  it('stays untrusted on cancel', async () => {
    const asked = ensureTrusted()
    closeDialog(false)
    expect(await asked).toBe(false)
    expect(isTrusted()).toBe(false)
  })

  it('is per project and can be revoked', async () => {
    const asked = ensureTrusted()
    closeDialog(true)
    await asked
    open('C:/games/other')
    expect(isTrusted()).toBe(false)
    open('C:/games/demo')
    forgetTrust()
    expect(isTrusted()).toBe(false)
  })

  it('does not trust a project that closed while the dialog was open', async () => {
    const asked = ensureTrusted()
    open('C:/games/other')
    closeDialog(true)
    expect(await asked).toBe(false)
    expect(isTrusted('C:/games/demo')).toBe(false)
  })
})

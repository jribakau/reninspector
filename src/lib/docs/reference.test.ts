import { describe, expect, it } from 'vitest'
import { docNow, docsStarting, loadDocs } from './reference'

describe('renpy reference', () => {
  it('loads statements and functions, and records the documentation licence', async () => {
    const docs = await loadDocs()
    expect(docs.size).toBeGreaterThan(30)
    expect(docNow('label')?.signature).toBe('label name:')
    expect(docNow('Character')?.params[0].name).toBe('name')
    expect(docNow('renpy.jump')?.url).toContain('renpy.org')
    expect(docsStarting('renpy.').some((e) => e.name === 'renpy.call')).toBe(true)
    const file = await import('./renpy-reference.json')
    expect(String((file.default ?? file).license)).toContain('MIT')
  })
})

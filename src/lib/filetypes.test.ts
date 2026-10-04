import { describe, expect, it } from 'vitest'
import { extOf, fileLook } from './filetypes'

describe('fileLook', () => {
  it('maps Ren\'Py scripts', () => {
    expect(fileLook('script.rpy')).toEqual({ icon: 'renpy', tone: 'script' })
    expect(fileLook('lib.RPYM').tone).toBe('script')
  })

  it('maps media by kind', () => {
    expect(fileLook('bg.PNG').tone).toBe('image')
    expect(fileLook('theme.ogg').icon).toBe('audio')
    expect(fileLook('intro.webm').icon).toBe('video')
  })

  it('maps archives, config, docs and unknowns', () => {
    expect(fileLook('archive.rpa').icon).toBe('archive')
    expect(fileLook('options.json').icon).toBe('config')
    expect(fileLook('README.md').icon).toBe('markdown')
    expect(fileLook('log.txt').icon).toBe('file-text')
    expect(fileLook('thing.bin')).toEqual({ icon: 'file', tone: 'plain' })
  })

  it('treats dotfiles as having no extension', () => {
    expect(extOf('.gitignore')).toBe('')
    expect(extOf('a.b.c')).toBe('c')
  })
})

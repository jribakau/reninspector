import { beforeEach, describe, expect, it } from 'vitest'
import {
  DEFAULTS,
  SETTINGS_KEY,
  coerce,
  exportSettings,
  importSettings,
  loadSettings,
  resetAll,
  resetLayout,
  setSetting,
  settings,
} from './settings.svelte'

beforeEach(() => {
  localStorage.clear()
  loadSettings()
})

describe('defaults', () => {
  it('start at the documented values', () => {
    expect(settings.fontSize).toBe(13)
    expect(settings.indentWidth).toBe(4)
    expect(settings.autosave).toBe('off')
    expect(settings.maxTabs).toBe(16)
    expect(settings.maxRecent).toBe(8)
    expect(settings.spell).toBe(true)
    expect(settings.wrap).toBe(false)
  })
})

describe('legacy migration', () => {
  it('moves the old separate keys into the settings key and removes them', () => {
    localStorage.clear()
    localStorage.setItem('vnide.theme', 'light')
    localStorage.setItem('vnide.font', '17')
    localStorage.setItem('vnide.wrap', '1')
    localStorage.setItem('vnide.spell', '0')
    localStorage.setItem('vnide.follow', '0')
    localStorage.setItem('vnide.stageAnimate', '1')
    localStorage.setItem('vnide.followCaret', '0')
    localStorage.setItem('vnide.diff.split', '1')
    localStorage.setItem('vnide.scm.tree', '1')
    loadSettings()
    expect(settings.theme).toBe('light')
    expect(settings.fontSize).toBe(17)
    expect(settings.wrap).toBe(true)
    expect(settings.spell).toBe(false)
    expect(settings.followGame).toBe(false)
    expect(settings.stageAnimate).toBe(true)
    expect(settings.followCaret).toBe(false)
    expect(settings.diffSplit).toBe(true)
    expect(settings.gitTree).toBe(true)
    expect(localStorage.getItem('vnide.theme')).toBeNull()
    expect(localStorage.getItem('vnide.scm.tree')).toBeNull()
    expect(JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}')).toMatchObject({ theme: 'light', fontSize: 17 })
  })

  it('prefers the settings key once it exists', () => {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify({ fontSize: 15 }))
    localStorage.setItem('vnide.font', '20')
    loadSettings()
    expect(settings.fontSize).toBe(15)
    expect(localStorage.getItem('vnide.font')).toBe('20')
  })

  it('survives a corrupt settings value', () => {
    localStorage.setItem(SETTINGS_KEY, '{nope')
    loadSettings()
    expect(settings.fontSize).toBe(DEFAULTS.fontSize)
  })
})

describe('setSetting', () => {
  it('clamps numbers to their range', () => {
    setSetting('fontSize', 99)
    expect(settings.fontSize).toBe(22)
    setSetting('fontSize', 1)
    expect(settings.fontSize).toBe(11)
    setSetting('maxTabs', 1000)
    expect(settings.maxTabs).toBe(64)
  })

  it('snaps numbers to their step', () => {
    setSetting('lineHeight', 1.52)
    expect(settings.lineHeight).toBe(1.5)
    setSetting('autosaveDelay', 1300)
    expect(settings.autosaveDelay).toBe(1500)
  })

  it('rejects values of the wrong type or outside an enum', () => {
    expect(setSetting('wrap', 'yes' as never)).toBe(false)
    expect(setSetting('theme', 'purple' as never)).toBe(false)
    expect(setSetting('indentWidth', 3 as never)).toBe(false)
    expect(setSetting('fontSize', Number.NaN)).toBe(false)
    expect(settings.wrap).toBe(false)
    expect(settings.theme).toBe('dark')
    expect(settings.indentWidth).toBe(4)
  })

  it('accepts valid enum values, including numeric ones', () => {
    expect(setSetting('indentWidth', 2)).toBe(true)
    expect(settings.indentWidth).toBe(2)
    expect(setSetting('autosave', 'blur')).toBe(true)
    expect(settings.autosave).toBe('blur')
    expect(setSetting('keymap', 'vim')).toBe(true)
    expect(settings.keymap).toBe('vim')
    expect(setSetting('keymap', 'modal' as never)).toBe(false)
    expect(settings.keymap).toBe('vim')
  })

  it('trims text', () => {
    setSetting('fontFamily', '  Fira Code  ')
    expect(settings.fontFamily).toBe('Fira Code')
  })

  it('saves only the values that differ from their defaults', () => {
    setSetting('wrap', true)
    expect(JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}')).toEqual({ wrap: true })
    setSetting('wrap', false)
    expect(localStorage.getItem(SETTINGS_KEY)).toBeNull()
  })

  it('survives a reload', () => {
    setSetting('indentWidth', 8)
    setSetting('autosave', 'delay')
    loadSettings()
    expect(settings.indentWidth).toBe(8)
    expect(settings.autosave).toBe('delay')
  })
})

describe('coerce', () => {
  it('returns undefined for unknown keys', () => {
    expect(coerce('nonsense' as never, true)).toBeUndefined()
  })
})

describe('resetAll', () => {
  it('restores every default and clears storage', () => {
    setSetting('wrap', true)
    setSetting('theme', 'light')
    resetAll()
    expect(settings.wrap).toBe(false)
    expect(settings.theme).toBe('dark')
    expect(localStorage.getItem(SETTINGS_KEY)).toBeNull()
  })
})

describe('import and export', () => {
  it('round trips', () => {
    setSetting('fontSize', 18)
    setSetting('spell', false)
    const text = exportSettings()
    resetAll()
    const result = importSettings(text)
    expect(result.skipped).toBe(0)
    expect(settings.fontSize).toBe(18)
    expect(settings.spell).toBe(false)
  })

  it('accepts a bare object and skips unknown keys and bad values', () => {
    const result = importSettings(JSON.stringify({ wrap: true, fontSize: 'big', mystery: 1, theme: 'light' }))
    expect(result).toEqual({ applied: 2, skipped: 2 })
    expect(settings.wrap).toBe(true)
    expect(settings.theme).toBe('light')
    expect(settings.fontSize).toBe(13)
  })

  it('rejects text that is not a settings object', () => {
    expect(() => importSettings('not json')).toThrow()
    expect(() => importSettings('[1,2]')).toThrow()
    expect(() => importSettings('42')).toThrow()
  })
})

describe('resetLayout', () => {
  it('forgets saved panel sizes only', () => {
    localStorage.setItem('vnide.w.side', '320')
    localStorage.setItem('vnide.h.bottom', '240')
    localStorage.setItem('vnide.ex.tree', '0')
    localStorage.setItem('vnide.recent', '[]')
    resetLayout()
    expect(localStorage.getItem('vnide.w.side')).toBeNull()
    expect(localStorage.getItem('vnide.h.bottom')).toBeNull()
    expect(localStorage.getItem('vnide.ex.tree')).toBeNull()
    expect(localStorage.getItem('vnide.recent')).toBe('[]')
  })
})

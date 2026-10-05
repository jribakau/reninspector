import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from './api'
import { isStageLiteral, loadStageVars, refreshStage, resetStage, resetStagePins, setStagePin, stageUi, stageVars, unlistedPins } from './stage.svelte'
import type { StageEstimate } from './types'

beforeEach(() => {
  localStorage.clear()
  stageVars.root = ''
  stageVars.overrides = {}
})

describe('stage pins', () => {
  it('accepts literals the preview can read', () => {
    expect(isStageLiteral('12')).toBe(true)
    expect(isStageLiteral(' 6.5 ')).toBe(true)
    expect(isStageLiteral('True')).toBe(true)
    expect(isStageLiteral('False')).toBe(true)
    expect(isStageLiteral('None')).toBe(true)
    expect(isStageLiteral('"home"')).toBe(true)
    expect(isStageLiteral("'n'")).toBe(true)
    expect(isStageLiteral('time')).toBe(false)
    expect(isStageLiteral('pick()')).toBe(false)
  })

  it('saves pins per project and resets them', () => {
    loadStageVars('game-a')
    setStagePin('time', '12')
    setStagePin('place', '"home"')
    expect(stageVars.overrides).toEqual({ time: '12', place: '"home"' })

    loadStageVars('game-b')
    expect(stageVars.overrides).toEqual({})
    setStagePin('time', '22')

    loadStageVars('game-a')
    expect(stageVars.overrides).toEqual({ time: '12', place: '"home"' })

    setStagePin('time', null)
    expect(stageVars.overrides).toEqual({ place: '"home"' })
    resetStagePins()
    expect(stageVars.overrides).toEqual({})
    expect(localStorage.getItem('vnide.stagePins.game-a')).toBeNull()

    loadStageVars('game-b')
    expect(stageVars.overrides).toEqual({ time: '22' })
  })

  it('keeps a pin that the current line does not list', () => {
    loadStageVars('game-a')
    setStagePin('time', '12')
    setStagePin('place', '"home"')
    expect(unlistedPins(stageVars.overrides, ['time'])).toEqual(['place'])
    expect(unlistedPins(stageVars.overrides, [])).toEqual(['place', 'time'])
  })

  it('drops a stored value that is not a literal', () => {
    localStorage.setItem('vnide.stagePins.game-a', JSON.stringify({ time: '12', place: 'home' }))
    loadStageVars('game-a')
    expect(stageVars.overrides).toEqual({ time: '12' })
  })
})

const bg = {
  width: 1280,
  height: 720,
  sprites: [{ picture: { kind: 'file', path: 'images/bg.png' } }],
  say: null,
  choices: [],
  gui: {},
  via: 'start',
  decisions: 0,
  assumptions: [],
  notes: [],
  vars: [],
} as unknown as StageEstimate

describe('stage images', () => {
  beforeEach(() => {
    let n = 0
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      writable: true,
      value: () => `blob:test-${++n}`,
    })
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      writable: true,
      value: () => {},
    })
    resetStage('game-a')
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('forgets the previous project picture when the same path is opened again', async () => {
    vi.spyOn(api, 'stageAt').mockResolvedValue(bg)
    const read = vi.spyOn(await import('./api'), 'readAsset').mockResolvedValue(new Uint8Array([1]).buffer)

    void refreshStage('script.rpy', 1)
    await vi.waitFor(() => expect(stageUi.images['images/bg.png']).toBe('blob:test-1'))

    resetStage('game-b')
    expect(stageUi.estimate).toBeNull()
    expect(stageUi.images).toEqual({})

    read.mockResolvedValue(new Uint8Array([2]).buffer)
    void refreshStage('script.rpy', 1)
    await vi.waitFor(() => expect(stageUi.images['images/bg.png']).toBe('blob:test-2'))
    expect(read).toHaveBeenCalledTimes(2)
  })

  it('ignores an image read that finishes after the project changes', async () => {
    vi.spyOn(api, 'stageAt').mockResolvedValue(bg)
    let release: (buf: ArrayBuffer) => void = () => {}
    vi.spyOn(await import('./api'), 'readAsset').mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve
        }),
    )

    void refreshStage('script.rpy', 1)
    await vi.waitFor(() => expect(stageUi.estimate).toBe(bg))

    resetStage('game-b')
    release(new Uint8Array([1]).buffer)
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(stageUi.estimate).toBeNull()
    expect(stageUi.images).toEqual({})
  })
})

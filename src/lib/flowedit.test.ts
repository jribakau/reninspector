import { describe, expect, it } from 'vitest'
import {
  emptyStage,
  insertSpot,
  isBeatCard,
  specFromStage,
  spanOf,
  spotLine,
  stageFromSpec,
  stageProblem,
} from './flowedit'
import type { BeatLine, GNode } from './types'

function node(over: Partial<GNode>): GNode {
  return {
    id: 'n1',
    kind: 'dialogue',
    line: 10,
    endLine: 10,
    says: 0,
    body: '',
    beats: [],
    ...over,
  } as unknown as GNode
}

function beat(line: number, cmd = 'show'): BeatLine {
  return { line, endLine: line, cmd, text: 'eileen', editable: true }
}

describe('staging form', () => {
  it('requires an image for show, scene, and hide', () => {
    for (const cmd of ['show', 'scene', 'hide'] as const) {
      expect(stageProblem(emptyStage(cmd))).not.toBe('')
      expect(stageProblem({ ...emptyStage(cmd), image: 'eileen happy' })).toBe('')
    }
  })

  it('requires a transition for with and a file for play', () => {
    expect(stageProblem(emptyStage('with'))).not.toBe('')
    expect(stageProblem({ ...emptyStage('with'), with: 'dissolve' })).toBe('')
    expect(stageProblem(emptyStage('play'))).not.toBe('')
    expect(stageProblem({ ...emptyStage('play'), file: 'a.ogg' })).toBe('')
    expect(stageProblem(emptyStage('stop'))).toBe('')
    expect(stageProblem(emptyStage('pause'))).toBe('')
  })

  it('round-trips a show statement through the form', () => {
    const form = { ...emptyStage('show'), image: ' eileen happy ', at: 'left', with: 'dissolve' }
    const spec = specFromStage(form)
    expect(spec).toEqual({ kind: 'show', image: 'eileen happy', at: 'left', with: 'dissolve' })
    expect(stageFromSpec(spec)).toMatchObject({ cmd: 'show', image: 'eileen happy', at: 'left', with: 'dissolve' })
  })

  it('round-trips audio statements', () => {
    const play = specFromStage({ ...emptyStage('play'), file: 'theme.ogg', fadein: '1.0' })
    expect(play).toMatchObject({ kind: 'play', channel: 'music', file: 'theme.ogg', fadein: '1.0', looped: true })
    expect(stageFromSpec(play)).toMatchObject({ cmd: 'play', file: 'theme.ogg', looped: true })
    const stop = specFromStage({ ...emptyStage('stop'), fadeout: '0.5' })
    expect(stageFromSpec(stop)).toMatchObject({ cmd: 'stop', fadeout: '0.5' })
  })

  it('does not read non-staging statements', () => {
    expect(stageFromSpec({ kind: 'jump', target: 'x' } as never)).toBeNull()
  })
})

describe('insert spots', () => {
  it('offers before and after a plain statement', () => {
    const jump = node({ kind: 'jump', line: 7, endLine: 7 })
    expect(insertSpot(jump, 'above')).toMatchObject({ line: 7, place: 'before' })
    expect(insertSpot(jump, 'below')).toMatchObject({ line: 7, place: 'after' })
  })

  it('only allows adding inside a choice', () => {
    const choice = node({ kind: 'choice', line: 4, endLine: 6 })
    expect(insertSpot(choice, 'above')).toBeNull()
    expect(insertSpot(choice, 'below')).toMatchObject({ line: 4, place: 'into', places: ['into'] })
  })

  it('anchors a beat card to its first and last statement', () => {
    const card = node({ beats: [beat(3), beat(4), beat(5)], line: 3, endLine: 5 })
    expect(isBeatCard(card)).toBe(true)
    const above = insertSpot(card, 'above')!
    const below = insertSpot(card, 'below')!
    expect(above.line).toBe(3)
    expect(below.line).toBe(5)
    expect(spotLine(below, 'before')).toBe(3)
    expect(spotLine(below, 'after')).toBe(5)
  })

  it('has no spot for cards that are not statements', () => {
    expect(insertSpot(node({ kind: 'label' as GNode['kind'] }), 'below')).toBeNull()
  })
})

describe('spanOf', () => {
  it('counts source lines', () => {
    expect(spanOf(node({ line: 5, endLine: 9 }))).toBe(5)
    expect(spanOf(node({ line: 5, endLine: 5 }))).toBe(1)
  })
})

import type { BeatLine, GNode, StmtSpec } from './types'

export type Place = 'before' | 'after' | 'into'
export type StageCmd = 'scene' | 'show' | 'hide' | 'with' | 'play' | 'stop' | 'pause'
export type InsertKind = 'say' | 'stage' | 'choice' | 'jump' | 'call' | 'return'

export const STAGE_CMDS: { id: StageCmd; label: string }[] = [
  { id: 'scene', label: 'Scene (new background)' },
  { id: 'show', label: 'Show' },
  { id: 'hide', label: 'Hide' },
  { id: 'with', label: 'Transition (with)' },
  { id: 'play', label: 'Play audio' },
  { id: 'stop', label: 'Stop audio' },
  { id: 'pause', label: 'Pause' },
]

export const POSITIONS = [
  'left',
  'right',
  'center',
  'truecenter',
  'topleft',
  'topright',
  'top',
  'offscreenleft',
  'offscreenright',
]

export const TRANSITIONS = [
  'dissolve',
  'fade',
  'None',
  'move',
  'moveinleft',
  'moveinright',
  'moveintop',
  'moveinbottom',
  'moveoutleft',
  'moveoutright',
  'ease',
  'easeinleft',
  'easeinright',
  'zoomin',
  'zoomout',
  'pixellate',
  'blinds',
  'squares',
  'wipeleft',
  'wiperight',
  'wipeup',
  'wipedown',
  'slideleft',
  'slideright',
  'slideup',
  'slidedown',
  'irisin',
  'irisout',
  'vpunch',
  'hpunch',
]

export const CHANNELS = ['music', 'sound', 'audio', 'voice']

/** Everything the staging form can hold. Unused fields stay empty. */
export interface StageForm {
  cmd: StageCmd
  image: string
  at: string
  with: string
  channel: string
  file: string
  fadein: string
  fadeout: string
  looped: boolean
  secs: string
}

export function emptyStage(cmd: StageCmd = 'show'): StageForm {
  return {
    cmd,
    image: '',
    at: '',
    with: '',
    channel: 'music',
    file: '',
    fadein: '',
    fadeout: '',
    looped: cmd === 'play',
    secs: '',
  }
}

export function stageFromSpec(spec: StmtSpec): StageForm | null {
  const f = emptyStage()
  switch (spec.kind) {
    case 'scene':
    case 'show':
      return { ...f, cmd: spec.kind, image: spec.image, at: spec.at ?? '', with: spec.with ?? '' }
    case 'hide':
      return { ...f, cmd: 'hide', image: spec.image, with: spec.with ?? '' }
    case 'with':
      return { ...f, cmd: 'with', with: spec.transition }
    case 'play':
      return { ...f, cmd: 'play', channel: spec.channel, file: spec.file, fadein: spec.fadein ?? '', looped: !!spec.looped }
    case 'stop':
      return { ...f, cmd: 'stop', channel: spec.channel, fadeout: spec.fadeout ?? '' }
    case 'pause':
      return { ...f, cmd: 'pause', secs: spec.secs ?? '' }
    default:
      return null
  }
}

/** What is missing from the form, or an empty string when it can be written. */
export function stageProblem(f: StageForm): string {
  switch (f.cmd) {
    case 'scene':
    case 'show':
    case 'hide':
      return f.image.trim() ? '' : 'Name the image.'
    case 'with':
      return f.with.trim() ? '' : 'Pick a transition.'
    case 'play':
      return f.file.trim() ? '' : 'Pick an audio file.'
    default:
      return ''
  }
}

export function specFromStage(f: StageForm): StmtSpec {
  switch (f.cmd) {
    case 'scene':
      return { kind: 'scene', image: f.image.trim(), at: f.at.trim(), with: f.with.trim() }
    case 'show':
      return { kind: 'show', image: f.image.trim(), at: f.at.trim(), with: f.with.trim() }
    case 'hide':
      return { kind: 'hide', image: f.image.trim(), with: f.with.trim() }
    case 'with':
      return { kind: 'with', transition: f.with.trim() }
    case 'play':
      return { kind: 'play', channel: f.channel.trim(), file: f.file.trim(), fadein: f.fadein.trim(), looped: f.looped }
    case 'stop':
      return { kind: 'stop', channel: f.channel.trim(), fadeout: f.fadeout.trim() }
    default:
      return { kind: 'pause', secs: f.secs.trim() }
  }
}

/** Where an insert lands and which sides the writer may pick. */
export interface InsertSpot {
  /** Line the new statement is anchored to. */
  line: number
  place: Place
  /** Sides offered in the editor. */
  places: Place[]
  /** A different anchor line for a side, when the card spans several statements. */
  lineFor?: Partial<Record<Place, number>>
}

/** What the flow editor is showing. */
export type EditorRequest =
  | { mode: 'node'; node: GNode; cond: string }
  | { mode: 'beat'; node: GNode; beat: BeatLine }
  | { mode: 'insert'; spot: InsertSpot; kind?: InsertKind; speaker?: string }

export function requestKey(r: EditorRequest): string {
  if (r.mode === 'node') return `node:${r.node.id}:${r.node.line}`
  if (r.mode === 'beat') return `beat:${r.beat.line}`
  return `insert:${r.spot.line}:${r.spot.place}:${r.kind ?? ''}`
}

export function spotLine(spot: InsertSpot, place: Place): number {
  return spot.lineFor?.[place] ?? spot.line
}

/** The node is a statement the flow can move, copy, delete, or insert next to. */
export function isStatementNode(n: GNode): boolean {
  switch (n.kind) {
    case 'dialogue':
      return !!n.body || n.beats.length > 0
    case 'choice':
    case 'menu':
    case 'jump':
    case 'call':
    case 'return':
      return true
    default:
      return false
  }
}

export function isBeatCard(n: GNode): boolean {
  return n.kind === 'dialogue' && !n.body && n.beats.length > 0
}

export function insertSpot(n: GNode, side: 'above' | 'below'): InsertSpot | null {
  if (!isStatementNode(n)) return null
  if (n.kind === 'choice') {
    return side === 'below' ? { line: n.line, place: 'into', places: ['into'] } : null
  }
  const place: Place = side === 'above' ? 'before' : 'after'
  const places: Place[] = ['before', 'after']
  if (isBeatCard(n)) {
    const first = n.beats[0].line
    const last = n.beats[n.beats.length - 1].line
    return { line: place === 'before' ? first : last, place, places, lineFor: { before: first, after: last } }
  }
  return { line: n.line, place, places }
}

/** Number of source lines a delete would remove. */
export function spanOf(n: GNode): number {
  return Math.max(1, n.endLine - n.line + 1)
}

export function beatLabel(b: BeatLine): string {
  return b.text ? `${b.cmd} ${b.text}` : b.cmd
}

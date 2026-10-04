import type { Text } from '@codemirror/state'

/** The parts of a merge `Chunk` these helpers read. */
export interface ChunkRange {
  fromA: number
  toA: number
  fromB: number
  toB: number
}

/**
 * Chunk ends point one past the last line, so the line break is part of the range.
 * Text for one side, with its closing break when the other side keeps a line after the range.
 */
function sideText(doc: Text, from: number, to: number, otherEndsInsideDoc: boolean): string {
  let text = doc.sliceString(from, Math.max(from, to - 1))
  if (from !== to && otherEndsInsideDoc) text += '\n'
  return text
}

/** Change that puts side A's lines back over side B's range. */
export function revertEdit(a: Text, b: Text, chunk: ChunkRange): { from: number; to: number; insert: string } {
  return {
    from: Math.min(b.length, chunk.fromB),
    to: Math.min(b.length, chunk.toB),
    insert: sideText(a, chunk.fromA, chunk.toA, chunk.toB <= b.length),
  }
}

/** Side A's text with only this chunk taken from side B. */
export function takeChunk(a: Text, b: Text, chunk: ChunkRange): string {
  const middle = sideText(b, chunk.fromB, chunk.toB, chunk.toA <= a.length)
  return a.sliceString(0, chunk.fromA) + middle + a.sliceString(Math.min(a.length, chunk.toA))
}

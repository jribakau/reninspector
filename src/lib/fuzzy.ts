/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License.
 *
 *  The character scorer (`fuzzyScore`) is adapted from Visual Studio Code
 *  `src/vs/base/common/filters.ts`. File and symbol ordering, which prefers a
 *  match in the name over a match only in the path, follows
 *  `src/vs/base/common/fuzzyScorer.ts`.
 *--------------------------------------------------------------------------------------------*/

/** A half-open range of matched characters. */
export interface FuzzySpan {
  start: number
  end: number
}

export interface ScoredMatch {
  /** Higher is a better match. */
  score: number
  matches: FuzzySpan[]
}

interface ScoreOptions {
  /** A match may start in the middle of a word, at a lower score. */
  firstMatchCanBeWeak: boolean
  boostFullMatch: boolean
}

const MAX = 128
const DIAG = 1
const LEFT = 2
const LEFT_LEFT = 3

/** A name match outranks a match that only hits the path. */
const LABEL_SCORE = 1 << 16
const LABEL_PREFIX_SCORE = 1 << 17
/** The query is the whole path. */
const PATH_IDENTITY_SCORE = 1 << 18
/** A kind-only match stays below every name or path match. */
const KIND_SCORE = -1_000_000

const WEAK: ScoreOptions = { firstMatchCanBeWeak: true, boostFullMatch: true }

const minPos = row(MAX + 1)
const maxPos = row(MAX + 1)
const diag = table()
const scores = table()
const arrows = table()

function row(n: number): number[] {
  return Array.from({ length: n }, () => 0)
}

function table(): number[][] {
  return Array.from({ length: MAX + 1 }, () => row(MAX + 1))
}

function clear(rows: number, cols: number) {
  for (let r = 0; r <= rows; r++) {
    diag[r].fill(0, 0, cols + 1)
    scores[r].fill(0, 0, cols + 1)
    arrows[r].fill(0, 0, cols + 1)
  }
  minPos.fill(0, 0, rows + 1)
  maxPos.fill(0, 0, rows + 1)
}

function isSeparator(code: number): boolean {
  switch (code) {
    case 95: // _
    case 45: // -
    case 46: // .
    case 32: // space
    case 47: // /
    case 92: // \
    case 39: // '
    case 34: // "
    case 58: // :
    case 36: // $
    case 60: // <
    case 62: // >
    case 40: // (
    case 41: // )
    case 91: // [
    case 93: // ]
    case 123: // {
    case 125: // }
      return true
    default:
      return false
  }
}

function isSeparatorAt(value: string, index: number): boolean {
  if (index < 0 || index >= value.length) return false
  return isSeparator(value.charCodeAt(index))
}

function isWhitespaceAt(value: string, index: number): boolean {
  if (index < 0 || index >= value.length) return false
  const code = value.charCodeAt(index)
  return code === 32 || code === 9
}

function isUpperAt(pos: number, word: string, wordLow: string): boolean {
  return word[pos] !== wordLow[pos]
}

function patternInWord(
  patternLow: string,
  patternPos: number,
  patternLen: number,
  wordLow: string,
  wordPos: number,
  wordLen: number,
): boolean {
  while (patternPos < patternLen && wordPos < wordLen) {
    if (patternLow[patternPos] === wordLow[wordPos]) {
      minPos[patternPos] = wordPos
      patternPos += 1
    }
    wordPos += 1
  }
  return patternPos === patternLen
}

function fillMax(patternLen: number, wordLen: number, patternStart: number, wordStart: number, patternLow: string, wordLow: string) {
  let patternPos = patternLen - 1
  let wordPos = wordLen - 1
  while (patternPos >= patternStart && wordPos >= wordStart) {
    if (patternLow[patternPos] === wordLow[wordPos]) {
      maxPos[patternPos] = wordPos
      patternPos--
    }
    wordPos--
  }
}

function charScore(
  pattern: string,
  patternLow: string,
  patternPos: number,
  patternStart: number,
  word: string,
  wordLow: string,
  wordPos: number,
  wordLen: number,
  wordStart: number,
  newMatchStart: boolean,
  firstStrong: { on: boolean },
): number {
  if (patternLow[patternPos] !== wordLow[wordPos]) return Number.MIN_SAFE_INTEGER

  let score = 1
  let gap = false
  if (wordPos === patternPos - patternStart) {
    score = pattern[patternPos] === word[wordPos] ? 7 : 5
  } else if (isUpperAt(wordPos, word, wordLow) && (wordPos === 0 || !isUpperAt(wordPos - 1, word, wordLow))) {
    score = pattern[patternPos] === word[wordPos] ? 7 : 5
    gap = true
  } else if (isSeparatorAt(wordLow, wordPos) && (wordPos === 0 || !isSeparatorAt(wordLow, wordPos - 1))) {
    score = 5
  } else if (isSeparatorAt(wordLow, wordPos - 1) || isWhitespaceAt(wordLow, wordPos - 1)) {
    score = 5
    gap = true
  }

  if (score > 1 && patternPos === patternStart) firstStrong.on = true

  if (!gap) {
    gap = isUpperAt(wordPos, word, wordLow) || isSeparatorAt(wordLow, wordPos - 1) || isWhitespaceAt(wordLow, wordPos - 1)
  }

  if (patternPos === patternStart) {
    if (wordPos > wordStart) score -= gap ? 3 : 5
  } else if (newMatchStart) {
    score += gap ? 2 : 0
  } else {
    score += gap ? 0 : 1
  }

  if (wordPos + 1 === wordLen) score -= gap ? 3 : 5
  return score
}

/**
 * Score `pattern` against `word`. Letters may be skipped. Matches on a
 * prefix, a capital, or the character after `_` / `-` / `.` / `/` score
 * higher. The returned list is `[score, wordStart, ...match positions]`
 * with the positions stored last-match-first, matching VS Code.
 */
function fuzzyScore(
  pattern: string,
  patternLow: string,
  patternStart: number,
  word: string,
  wordLow: string,
  wordStart: number,
  options: ScoreOptions,
): number[] | undefined {
  const patternLen = Math.min(pattern.length, MAX)
  const wordLen = Math.min(word.length, MAX)
  if (patternStart >= patternLen || wordStart >= wordLen || patternLen - patternStart > wordLen - wordStart) return undefined

  clear(patternLen, wordLen)
  if (!patternInWord(patternLow, patternStart, patternLen, wordLow, wordStart, wordLen)) return undefined
  fillMax(patternLen, wordLen, patternStart, wordStart, patternLow, wordLow)

  const firstStrong = { on: false }

  let row = 1
  for (let patternPos = patternStart; patternPos < patternLen; row++, patternPos++) {
    const minWord = minPos[patternPos]
    const maxWord = maxPos[patternPos]
    const nextMax = patternPos + 1 < patternLen ? maxPos[patternPos + 1] : wordLen

    for (let wordPos = minWord, column = minWord - wordStart + 1; wordPos < nextMax; column++, wordPos++) {
      let score = Number.MIN_SAFE_INTEGER
      if (wordPos <= maxWord) {
        score = charScore(
          pattern, patternLow, patternPos, patternStart,
          word, wordLow, wordPos, wordLen, wordStart,
          diag[row - 1][column - 1] === 0,
          firstStrong,
        )
      }

      let diagScore = 0
      const canDiag = score !== Number.MIN_SAFE_INTEGER
      if (canDiag) diagScore = score + scores[row - 1][column - 1]

      const canLeft = wordPos > minWord
      const leftScore = canLeft ? scores[row][column - 1] + (diag[row][column - 1] > 0 ? -5 : 0) : 0
      const canLeftLeft = wordPos > minWord + 1 && diag[row][column - 1] > 0
      const leftLeftScore = canLeftLeft ? scores[row][column - 2] + (diag[row][column - 2] > 0 ? -5 : 0) : 0

      if (canLeftLeft && (!canLeft || leftLeftScore >= leftScore) && (!canDiag || leftLeftScore >= diagScore)) {
        scores[row][column] = leftLeftScore
        arrows[row][column] = LEFT_LEFT
        diag[row][column] = 0
      } else if (canLeft && (!canDiag || leftScore >= diagScore)) {
        scores[row][column] = leftScore
        arrows[row][column] = LEFT
        diag[row][column] = 0
      } else if (canDiag) {
        scores[row][column] = diagScore
        arrows[row][column] = DIAG
        diag[row][column] = diag[row - 1][column - 1] + 1
      } else {
        return undefined
      }
    }
  }

  if (!firstStrong.on && !options.firstMatchCanBeWeak) return undefined

  row--
  // The last pattern row always walks through to `wordLen`, so the last written
  // column is `wordLen - wordStart`.
  let column = wordLen - wordStart
  const result: number[] = [scores[row][column], wordStart]
  let backwards = 0

  while (row >= 1) {
    let diagColumn = column
    while (diagColumn >= 1) {
      const arrow = arrows[row][diagColumn]
      if (arrow === LEFT_LEFT) diagColumn -= 2
      else if (arrow === LEFT) diagColumn -= 1
      else break
    }

    if (
      backwards > 1
      && patternLow[patternStart + row - 1] === wordLow[wordStart + column - 1]
      && !isUpperAt(diagColumn + wordStart - 1, word, wordLow)
      && backwards + 1 > diag[row][diagColumn]
    ) {
      diagColumn = column
    }

    backwards = diagColumn === column ? backwards + 1 : 1
    row--
    column = diagColumn - 1
    result.push(column)
  }

  if (wordLen - wordStart === patternLen && options.boostFullMatch) result[0] += 2

  const lastColumn = result[2] + 1
  result[0] -= lastColumn - patternLen
  return result
}

function spansFrom(scored: number[]): FuzzySpan[] {
  const wordPos = scored[1]
  const spans: FuzzySpan[] = []
  for (let i = scored.length - 1; i > 1; i--) {
    const pos = scored[i] + wordPos
    const last = spans[spans.length - 1]
    if (last && last.end === pos) last.end = pos + 1
    else spans.push({ start: pos, end: pos + 1 })
  }
  return spans
}

function mergeSpans(spans: FuzzySpan[]): FuzzySpan[] {
  const sorted = [...spans].sort((a, b) => a.start - b.start || a.end - b.end)
  const out: FuzzySpan[] = []
  for (const span of sorted) {
    const last = out[out.length - 1]
    if (!last || span.start > last.end) out.push({ start: span.start, end: span.end })
    else last.end = Math.max(last.end, span.end)
  }
  return out
}

function shift(spans: readonly FuzzySpan[], offset: number): FuzzySpan[] {
  if (!offset) return spans.map((span) => ({ start: span.start, end: span.end }))
  return spans.map((span) => ({ start: span.start + offset, end: span.end + offset }))
}

/** Matched ranges, or null. `weak` allows a match that starts mid-word. */
export function matchWord(pattern: string, word: string, weak = false): FuzzySpan[] | null {
  if (!pattern || !word) return null
  const patternLow = pattern.toLowerCase()
  const wordLow = word.toLowerCase()
  if (patternLow.length !== pattern.length || wordLow.length !== word.length) return null
  const scored = fuzzyScore(pattern, patternLow, 0, word, wordLow, 0, {
    firstMatchCanBeWeak: weak,
    boostFullMatch: true,
  })
  return scored ? spansFrom(scored) : null
}

function scoreWord(pattern: string, word: string): ScoredMatch | null {
  if (!pattern || !word) return null
  const patternLow = pattern.toLowerCase()
  const wordLow = word.toLowerCase()
  if (patternLow.length !== pattern.length || wordLow.length !== word.length) return null
  const scored = fuzzyScore(pattern, patternLow, 0, word, wordLow, 0, WEAK)
  if (!scored) return null
  return { score: scored[0], matches: spansFrom(scored) }
}

function queryPieces(query: string): string[] {
  return query.trim().split(/\s+/).filter(Boolean)
}

function slash(path: string): { dir: string; name: string } {
  const at = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  if (at < 0) return { dir: '', name: path }
  return { dir: path.slice(0, at), name: path.slice(at + 1) }
}

function samePath(query: string, path: string): boolean {
  return query.replaceAll('\\', '/').toLowerCase() === path.replaceAll('\\', '/').toLowerCase()
}

function scoreName(piece: string, name: string, offset: number): ScoredMatch | null {
  const hit = scoreWord(piece, name)
  if (!hit) return null
  const prefix = name.toLowerCase().startsWith(piece.toLowerCase())
  let score = (prefix ? LABEL_PREFIX_SCORE : LABEL_SCORE) + hit.score
  if (prefix && name.length) score += Math.round((piece.length / name.length) * 100)
  return { score, matches: shift(hit.matches, offset) }
}

/** Score a project path. A hit in the file name outranks a hit only in a folder. */
export function scoreFile(query: string, path: string): ScoredMatch | null {
  const pieces = queryPieces(query)
  if (!pieces.length || !path) return null
  if (samePath(query.trim(), path)) return { score: PATH_IDENTITY_SCORE, matches: [{ start: 0, end: path.length }] }

  let score = 0
  const matches: FuzzySpan[] = []
  const { dir, name } = slash(path)
  const nameAt = dir ? dir.length + 1 : 0
  for (const raw of pieces) {
    const piece = raw.replaceAll('\\', '/')
    if (!piece.includes('/')) {
      const named = scoreName(piece, name, nameAt)
      if (named) {
        score += named.score
        matches.push(...named.matches)
        continue
      }
    }
    const hit = scoreWord(piece, path)
    if (!hit) return null
    score += hit.score
    matches.push(...hit.matches)
  }
  return { score, matches: mergeSpans(matches) }
}

export const SYMBOL_SEP = ' · '

/** The symbol row, in the same order the scorer highlights. */
export function symbolLabel(name: string, kind: string, path: string): string {
  return `${name}${SYMBOL_SEP}${kind}${SYMBOL_SEP}${path}`
}

/** Score a symbol. The name outranks the path, which outranks the kind. */
export function scoreSymbol(query: string, name: string, kind: string, path: string): ScoredMatch | null {
  const pieces = queryPieces(query)
  if (!pieces.length || !name) return null
  const kindAt = name.length + SYMBOL_SEP.length
  const pathAt = kindAt + kind.length + SYMBOL_SEP.length
  let score = 0
  const matches: FuzzySpan[] = []
  for (const piece of pieces) {
    const named = scoreName(piece, name, 0)
    if (named) {
      score += named.score
      matches.push(...named.matches)
      continue
    }
    const inPath = path ? scoreWord(piece.replaceAll('\\', '/'), path) : null
    if (inPath) {
      score += inPath.score
      matches.push(...shift(inPath.matches, pathAt))
      continue
    }
    const inKind = scoreWord(piece, kind)
    if (!inKind) return null
    score += KIND_SCORE + inKind.score
    matches.push(...shift(inKind.matches, kindAt))
  }
  return { score, matches: mergeSpans(matches) }
}

/** Score a single label, such as a command name. Every word of the query must match. */
export function scoreLabel(query: string, label: string): ScoredMatch | null {
  const pieces = queryPieces(query)
  if (!pieces.length || !label) return null
  let score = 0
  const matches: FuzzySpan[] = []
  for (const piece of pieces) {
    const hit = scoreWord(piece, label)
    if (!hit) return null
    score += hit.score
    matches.push(...hit.matches)
  }
  return { score, matches: mergeSpans(matches) }
}

/** Split `text` into plain and matched runs. */
export function highlightPieces(text: string, matches: readonly FuzzySpan[]): { text: string; hit: boolean }[] {
  if (!matches.length) return [{ text, hit: false }]
  const out: { text: string; hit: boolean }[] = []
  let cursor = 0
  for (const span of matches) {
    const start = Math.max(span.start, cursor)
    const end = Math.min(Math.max(span.end, start), text.length)
    if (start > cursor) out.push({ text: text.slice(cursor, start), hit: false })
    if (end > start) out.push({ text: text.slice(start, end), hit: true })
    cursor = end
  }
  if (cursor < text.length) out.push({ text: text.slice(cursor), hit: false })
  return out.length ? out : [{ text, hit: false }]
}

/** Keep the `cap` best matches. Higher score first, then `tie`, then input order. */
export function rankMatches<T>(
  items: readonly T[],
  score: (item: T) => ScoredMatch | null,
  tie: (a: T, b: T) => number,
  cap: number,
): { item: T; hit: ScoredMatch }[] {
  const scored: { item: T; hit: ScoredMatch; index: number }[] = []
  items.forEach((item, index) => {
    const hit = score(item)
    if (hit) scored.push({ item, hit, index })
  })
  scored.sort((a, b) => b.hit.score - a.hit.score || tie(a.item, b.item) || a.index - b.index)
  return scored.slice(0, cap).map(({ item, hit }) => ({ item, hit }))
}

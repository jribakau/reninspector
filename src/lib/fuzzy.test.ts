import { describe, expect, it } from 'vitest'
import {
  highlightPieces,
  matchWord,
  rankMatches,
  scoreFile,
  scoreLabel,
  scoreSymbol,
  symbolLabel,
  type FuzzySpan,
} from './fuzzy'

function marked(word: string, matches: FuzzySpan[] | null): string | undefined {
  if (!matches) return undefined
  let out = ''
  let pos = 0
  for (const match of matches) {
    out += word.slice(pos, match.start)
    out += `^${word.slice(match.start, match.end).split('').join('^')}`
    pos = match.end
  }
  return out + word.slice(pos)
}

describe('matchWord', () => {
  const cases: [string, string, string | undefined, boolean?][] = [
    ['tit', 'win.tit', 'win.^t^i^t'],
    ['title', 'win.title', 'win.^t^i^t^l^e'],
    ['WordCla', 'WordCharacterClassifier', '^W^o^r^dCharacter^C^l^assifier'],
    ['BK', 'the_black_knight', 'the_^black_^knight'],
    ['bkn', 'the_black_knight', 'the_^black_^k^night'],
    ['fob', 'foobar', '^f^oo^bar'],
    ['foobar', 'foobar', '^f^o^o^b^a^r'],
    ['fo', 'barfoo', undefined],
    ['fo', 'bar_foo', 'bar_^f^oo'],
    ['fo', 'bar_Foo', 'bar_^F^oo'],
    ['fo', 'bar/foo', 'bar/^f^oo'],
    ['ob', 'foobar', undefined],
    ['TEdit', 'text_edit', '^text_^e^d^i^t'],
    ['TEdit', 'TextEdit', '^Text^E^d^i^t'],
    ['is', 'ImportStatement', '^Import^Statement'],
    ['gp', 'Git: Pull', '^Git: ^Pull'],
    ['Three', 'HTMLHRElement', undefined],
    ['Three', 'HTMLHRElement', 'H^TML^H^R^El^ement', true],
    ['tor', 'constructor', 'construc^t^o^r', true],
    ['d2bad', 'day2_cafe_bad_end', '^day^2_cafe_^b^a^d_end', true],
  ]

  it.each(cases)('%s against %s', (pattern, word, expected, weak) => {
    expect(marked(word, matchWord(pattern, word, weak ?? false))).toBe(expected)
  })

  it('does not depend on the previous query', () => {
    expect(marked('foobar', matchWord('ob', 'foobar'))).toBeUndefined()
    expect(marked('day2_cafe_bad_end', matchWord('d2bad', 'day2_cafe_bad_end', true))).toBe('^day^2_cafe_^b^a^d_end')
    expect(marked('foobar', matchWord('ob', 'foobar'))).toBeUndefined()
    expect(marked('bar_foo', matchWord('fo', 'bar_foo'))).toBe('bar_^f^oo')
  })

  it('ranks a boundary above the middle of a word', () => {
    const boundary = scoreLabel('fo', 'bar_foo')
    const middle = scoreLabel('fo', 'barfoo')
    expect(boundary && middle && boundary.score > middle.score).toBe(true)
  })
})

describe('scoreFile', () => {
  it('matches a name with gaps and highlights those letters', () => {
    const hit = scoreFile('d2bad', 'game/day2_cafe_bad_end.rpy')
    expect(hit).not.toBeNull()
    expect(highlightPieces('game/day2_cafe_bad_end.rpy', hit!.matches).filter((p) => p.hit).map((p) => p.text)).toEqual([
      'd', '2', 'bad',
    ])
  })

  it('prefers the file name over a folder', () => {
    const named = scoreFile('script', 'other/script.rpy')
    const folder = scoreFile('script', 'script/notes.txt')
    expect(named && folder && named.score > folder.score).toBe(true)
  })

  it('prefers a prefix over a later fuzzy hit', () => {
    const exact = scoreFile('script', 'game/script.rpy')
    const later = scoreFile('script', 'game/my_script_helpers.rpy')
    expect(exact && later && exact.score > later.score).toBe(true)
  })

  it('prefers the shorter prefix', () => {
    const short = scoreFile('window', 'ui/window.rpy')
    const long = scoreFile('window', 'ui/windowActions.rpy')
    expect(short && long && short.score > long.score).toBe(true)
  })

  it('puts an exact path above a partial name', () => {
    const exact = scoreFile('game/script.rpy', 'game/script.rpy')
    const partial = scoreFile('game/script.rpy', 'game/script_extra.rpy')
    expect(exact && partial && exact.score > partial.score).toBe(true)
  })

  it('matches inside a word, not only at a boundary', () => {
    expect(scoreFile('cript', 'game/script.rpy')).not.toBeNull()
  })

  it('requires every word', () => {
    expect(scoreFile('day2 bad', 'game/day2_cafe_bad_end.rpy')).not.toBeNull()
    expect(scoreFile('day2 bad', 'game/day2_cafe.rpy')).toBeNull()
    expect(scoreFile('zzz', 'game/day2_cafe_bad_end.rpy')).toBeNull()
  })

  it('treats a backslash like a slash', () => {
    expect(scoreFile('game\\day2', 'game/day2/script.rpy')).not.toBeNull()
  })
})

describe('scoreSymbol', () => {
  it('highlights the name, not the kind or the path', () => {
    const label = symbolLabel('day2_cafe_bad_end', 'label', 'game/script.rpy')
    const hit = scoreSymbol('d2bad', 'day2_cafe_bad_end', 'label', 'game/script.rpy')
    expect(hit).not.toBeNull()
    const hits = highlightPieces(label, hit!.matches).filter((p) => p.hit).map((p) => p.text)
    expect(hits).toEqual(['d', '2', 'bad'])
    expect(hit!.matches.every((span) => span.end <= 'day2_cafe_bad_end'.length)).toBe(true)
  })

  it('ranks a name match above a path or kind match', () => {
    const name = scoreSymbol('start', 'start', 'label', 'game/other.rpy')
    const path = scoreSymbol('start', 'begin', 'label', 'game/start.rpy')
    const kind = scoreSymbol('label', 'begin', 'label', 'game/other.rpy')
    expect(name && path && kind).toBeTruthy()
    expect(name!.score).toBeGreaterThan(path!.score)
    expect(path!.score).toBeGreaterThan(kind!.score)
  })
})

describe('scoreLabel', () => {
  it('matches a command from its initials', () => {
    const hit = scoreLabel('gtf', 'Go to file')
    expect(hit).not.toBeNull()
    expect(highlightPieces('Go to file', hit!.matches).filter((p) => p.hit).map((p) => p.text).join('')).toBe('Gtf')
  })
})

describe('rankMatches', () => {
  it('orders by score and keeps input order on a tie', () => {
    const files = ['script/notes.txt', 'other/script.rpy', 'game/my_script.rpy']
    const ranked = rankMatches(files, (path) => scoreFile('script', path), () => 0, 10)
    expect(ranked.map((r) => r.item)).toEqual(['other/script.rpy', 'game/my_script.rpy', 'script/notes.txt'])
  })
})

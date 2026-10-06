import { PYTHON_HEADER, pythonLine } from './python'

/** A bracketed or backslash-continued statement that never closes stops being followed here. */
const MAX_CONTINUATION = 200

/**
 * A Python-only copy of a Ren'Py script with the same line count and the same
 * columns for every token that is kept, so language-server positions map onto
 * the real file without a translation table.
 *
 * `$ x` keeps its columns (the `$` becomes a space). A python header becomes
 * `if 1:` padded out to the same length, so the indented body stays valid.
 * `define` and `default` keep the assignment and blank the keyword. Lines that
 * continue one of those statements (an open bracket or a trailing backslash)
 * are kept too, so a multi-line `Character(...)` stays valid. Everything else
 * becomes an empty line.
 *
 * `python2` rewrites the few Python 2 forms a Python 3 server cannot parse.
 * Replacements keep the line length, so columns of the tokens that remain still
 * match the source. A form that cannot be rewritten in place is blanked.
 */
export function toVirtual(text: string, python2 = false): string {
  const lines = text.split('\n')
  let block: number | null = null
  let depth = 0
  let backslash = false
  let followed = 0
  /** First empty module-level line. A compat import can sit here without shifting any Python token. */
  let anchor: number | null = null
  let firstPython = -1
  const out = lines.map((line, idx) => {
    const inBlock = block != null
    const next = pythonLine(line, block)
    block = next.indent
    const keep = (result: string, header = false) => {
      const rewritten = python2 && result && !header ? rewritePy2(result) : result
      if (rewritten && firstPython < 0) firstPython = idx
      if (python2 && anchor == null && firstPython < 0 && !inBlock && depth === 0 && !backslash && rewritten === '') {
        anchor = idx
      }
      return rewritten
    }
    if (next.inline) {
      const kept = rewriteDollar(line)
      // Only a `$` statement can run on over several lines; a python block keeps every line anyway.
      const open = next.indent == null ? scan(kept, 0) : { depth: 0, backslash: false }
      depth = open.depth
      backslash = open.backslash
      followed = 0
      return keep(kept)
    }
    if ((depth > 0 || backslash) && followed < MAX_CONTINUATION) {
      followed += 1
      const open = scan(line, depth)
      depth = open.depth
      backslash = open.backslash
      return keep(line)
    }
    depth = 0
    backslash = false
    followed = 0
    if (isHeader(line, next.indent)) return keep(rewriteHeader(line), true)
    // A line that closed the block is back at the top level (`define` after `python:`).
    const kept = next.indent == null ? rewriteDefine(line) : null
    if (kept == null) return keep('')
    const open = scan(kept, 0)
    depth = open.depth
    backslash = open.backslash
    return keep(kept)
  })
  if (python2) placeCompat(out, anchor, firstPython)
  return out.join('\n')
}

/** Names a Ren'Py 7 script may use that Python 3 does not define. */
export const PY2_COMPAT_IMPORT =
  'from renpy7_compat import xrange, unicode, basestring, long, unichr, raw_input'

/**
 * The import has to run before the first Python line, and it has to leave the
 * following indented body attached to `if 1:`. An empty line above the code
 * takes the import. A file that starts with a python block puts it on that
 * header instead (`import ...; if 1:`).
 */
function placeCompat(out: string[], anchor: number | null, firstPython: number) {
  if (anchor != null && (firstPython < 0 || anchor < firstPython)) {
    out[anchor] = PY2_COMPAT_IMPORT
    return
  }
  const header = out.findIndex((line, i) => line.startsWith('if 1:') && (firstPython < 0 || i <= firstPython))
  if (header >= 0) out[header] = `${PY2_COMPAT_IMPORT}; if 1:`
}

/** Net open brackets after `line`, and whether it ends with a line continuation. Strings and comments are skipped. */
function scan(line: string, depth: number): { depth: number; backslash: boolean } {
  let d = depth
  let string: string | null = null
  for (let i = 0; i < line.length; i++) {
    const c = line[i]
    if (string) {
      if (c === '\\') i += 1
      else if (c === string) string = null
      continue
    }
    if (c === '"' || c === "'") string = c
    else if (c === '#') break
    else if (c === '(' || c === '[' || c === '{') d += 1
    else if (c === ')' || c === ']' || c === '}') d = Math.max(0, d - 1)
  }
  return { depth: d, backslash: string == null && /\\\s*$/.test(line) }
}

function leading(line: string): number {
  return line.match(/^ */)?.[0].length ?? 0
}

/** A header sets the block indent to its own column. A blank line inside a block does not. */
function isHeader(line: string, after: number | null): boolean {
  if (after == null || after !== leading(line)) return false
  return PYTHON_HEADER.test(line.trim())
}

function rewriteHeader(line: string): string {
  const repl = 'if 1:'
  if (line.length <= repl.length) return repl
  return repl + ' '.repeat(line.length - repl.length)
}

/** `$ x = 1` becomes `  x = 1`. A `$` that is not the statement marker is left alone. */
function rewriteDollar(line: string): string {
  if (!line.trimStart().startsWith('$')) return line
  const at = line.indexOf('$')
  return line.slice(0, at) + ' ' + line.slice(at + 1)
}

const DEFINE = /^(\s*)(define|default)(\s+-?\d+)?(\s+)(\S)/

/** Blank `define` / `default` (and an init priority) so the assignment stays in column. */
function rewriteDefine(line: string): string | null {
  const match = DEFINE.exec(line)
  if (!match) return null
  const cut = match[0].length - match[5].length
  return ' '.repeat(cut) + line.slice(cut)
}

function blank(line: string): string {
  return ' '.repeat(line.length)
}

/**
 * Python 2 syntax, rewritten so a Python 3 parser accepts the line.
 * Every change keeps `line.length`. A backtick blanks the line. An `except`
 * binder is blanked too: `as` does not fit in the space `,` occupies.
 */
function rewritePy2(line: string): string {
  const buf = [...line]
  let i = 0
  let string: string | null = null
  let depth = 0
  let stmt = true
  const n = buf.length
  const isWord = (c: string | undefined) => !!c && /[A-Za-z0-9_]/.test(c)

  const readWord = (at: number) => {
    let j = at
    while (j < n && isWord(buf[j])) j += 1
    return { word: buf.slice(at, j).join(''), end: j }
  }

  while (i < n) {
    const c = buf[i]
    if (string) {
      if (c === '\\') {
        i += 2
        continue
      }
      if (c === string) string = null
      i += 1
      continue
    }
    if (c === '#') break
    if (c === '"' || c === "'") {
      if (i >= 2 && (buf[i - 1] === 'r' || buf[i - 1] === 'R') && (buf[i - 2] === 'u' || buf[i - 2] === 'U') && !isWord(buf[i - 3])) {
        buf[i - 2] = ' '
      }
      string = c
      stmt = false
      i += 1
      continue
    }
    if (c === '`') return blank(line)
    if (c === '(' || c === '[' || c === '{') {
      depth += 1
      stmt = false
      i += 1
      continue
    }
    if (c === ')' || c === ']' || c === '}') {
      depth = Math.max(0, depth - 1)
      i += 1
      continue
    }
    if (depth === 0 && (c === ';' || c === ':')) {
      stmt = true
      i += 1
      continue
    }
    if (c === ' ' || c === '\t') {
      i += 1
      continue
    }
    if (depth === 0 && c === '<' && buf[i + 1] === '>') {
      buf[i] = '!'
      buf[i + 1] = '='
      stmt = false
      i += 2
      continue
    }
    if (/[0-9]/.test(c)) {
      let j = i + 1
      while (j < n && /[0-9.]/.test(buf[j])) j += 1
      if ((buf[j] === 'L' || buf[j] === 'l') && !isWord(buf[j + 1])) {
        buf[j] = ' '
        j += 1
      }
      stmt = false
      i = j
      continue
    }
    if (/[A-Za-z_]/.test(c)) {
      const { word, end } = readWord(i)
      if (depth === 0 && stmt && (word === 'print' || word === 'exec')) {
        let k = end
        while (k < n && (buf[k] === ' ' || buf[k] === '\t')) k += 1
        if (buf[k] !== '(') {
          const stop = statementEnd(buf, end)
          for (let t = i; t < stop; t += 1) buf[t] = ' '
          i = stop
          stmt = true
          continue
        }
      }
      if (depth === 0 && stmt && word === 'except') {
        // `except E, name:` is two characters shorter than `except E as name:`.
        // Blank the binder and keep the colon, so the handler stays attached and
        // every token before the comma keeps its column.
        const comma = exceptComma(buf, end)
        if (comma != null) {
          let stop = comma
          while (stop < n && buf[stop] !== ':') stop += 1
          for (let t = comma; t < stop; t += 1) buf[t] = ' '
          i = stop
          stmt = true
          continue
        }
        stmt = false
        i = end
        continue
      }
      if (depth === 0 && stmt && word === 'raise') {
        const comma = raiseComma(buf, end)
        if (comma != null) {
          const stop = statementEnd(buf, comma)
          for (let t = comma; t < stop; t += 1) buf[t] = ' '
          i = stop
          stmt = true
          continue
        }
      }
      stmt = false
      i = end
      continue
    }
    stmt = false
    i += 1
  }
  const next = buf.join('')
  return next.length === line.length ? next : blank(line)
}

/** Index of the next top-level `;` or `#`, or the end of the line. `from` is outside a string. */
function statementEnd(buf: string[], from: number): number {
  let depth = 0
  let string: string | null = null
  for (let i = from; i < buf.length; i += 1) {
    const c = buf[i]
    if (string) {
      if (c === '\\') {
        i += 1
        continue
      }
      if (c === string) string = null
      continue
    }
    if (c === '#') return i
    if (c === '"' || c === "'") string = c
    else if (c === '(' || c === '[' || c === '{') depth += 1
    else if (c === ')' || c === ']' || c === '}') depth = Math.max(0, depth - 1)
    else if (depth === 0 && c === ';') return i
  }
  return buf.length
}

/** The binder comma in `except E, name`. `null` when the clause is already `except E:` or `except E as name:`. */
function exceptComma(buf: string[], from: number): number | null {
  let depth = 0
  let string: string | null = null
  for (let i = from; i < buf.length; i += 1) {
    const c = buf[i]
    if (string) {
      if (c === '\\') {
        i += 1
        continue
      }
      if (c === string) string = null
      continue
    }
    if (c === '#') return null
    if (c === '"' || c === "'") string = c
    else if (c === '(' || c === '[' || c === '{') depth += 1
    else if (c === ')' || c === ']' || c === '}') depth = Math.max(0, depth - 1)
    else if (depth === 0 && c === ':') return null
    else if (depth === 0 && c === ',') return i
  }
  return null
}

/** The first top-level comma of a `raise` statement, if it has one. */
function raiseComma(buf: string[], from: number): number | null {
  let depth = 0
  let string: string | null = null
  for (let i = from; i < buf.length; i += 1) {
    const c = buf[i]
    if (string) {
      if (c === '\\') {
        i += 1
        continue
      }
      if (c === string) string = null
      continue
    }
    if (c === '#' || (depth === 0 && (c === ';' || c === ':'))) return null
    if (c === '"' || c === "'") string = c
    else if (c === '(' || c === '[' || c === '{') depth += 1
    else if (c === ')' || c === ']' || c === '}') depth = Math.max(0, depth - 1)
    else if (depth === 0 && c === ',') return i
  }
  return null
}

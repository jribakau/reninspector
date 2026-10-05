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
 */
export function toVirtual(text: string): string {
  const lines = text.split('\n')
  let block: number | null = null
  let depth = 0
  let backslash = false
  let followed = 0
  const out = lines.map((line) => {
    const next = pythonLine(line, block)
    block = next.indent
    if (next.inline) {
      const kept = rewriteDollar(line)
      // Only a `$` statement can run on over several lines; a python block keeps every line anyway.
      const open = next.indent == null ? scan(kept, 0) : { depth: 0, backslash: false }
      depth = open.depth
      backslash = open.backslash
      followed = 0
      return kept
    }
    if ((depth > 0 || backslash) && followed < MAX_CONTINUATION) {
      followed += 1
      const open = scan(line, depth)
      depth = open.depth
      backslash = open.backslash
      return line
    }
    depth = 0
    backslash = false
    followed = 0
    if (isHeader(line, next.indent)) return rewriteHeader(line)
    // A line that closed the block is back at the top level (`define` after `python:`).
    const kept = next.indent == null ? rewriteDefine(line) : null
    if (kept == null) return ''
    const open = scan(kept, 0)
    depth = open.depth
    backslash = open.backslash
    return kept
  })
  return out.join('\n')
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

export type QuoteStyle = 'keep' | 'double' | 'single'

export interface FormatOptions {
  /** Spaces written for each tab outside a string. Ren'Py rejects tab characters. */
  indent: number
  quotes: QuoteStyle
}

export interface TextEdit {
  from: number
  to: number
  insert: string
}

const DEFAULTS: FormatOptions = { indent: 4, quotes: 'keep' }

/**
 * Edits that turn tabs outside strings into spaces, drop trailing whitespace,
 * and (when asked) normalise simple quotes. Text inside triple quotes is left as it is.
 * `range` limits which lines change; string state is still tracked from the start.
 */
export function formatEdits(text: string, options: Partial<FormatOptions> = {}, range?: { fromLine: number; toLine: number }): TextEdit[] {
  const opts: FormatOptions = { ...DEFAULTS, ...options }
  const lines = text.split('\n')
  const edits: TextEdit[] = []
  let offset = 0
  let triple: '"' | "'" | null = null
  for (let i = 0; i < lines.length; i++) {
    const original = lines[i]
    const next = formatLine(original, triple, opts)
    triple = next.triple
    const inRange = !range || (i + 1 >= range.fromLine && i + 1 <= range.toLine)
    if (inRange && next.line !== original) {
      edits.push({ from: offset, to: offset + original.length, insert: next.line })
    }
    offset += original.length
    if (i < lines.length - 1) offset += 1
  }
  return edits
}

export function formatRenpy(text: string, options: Partial<FormatOptions> = {}): string {
  const edits = formatEdits(text, options)
  if (!edits.length) return text
  let out = ''
  let at = 0
  for (const edit of edits) {
    out += text.slice(at, edit.from) + edit.insert
    at = edit.to
  }
  return out + text.slice(at)
}

function formatLine(line: string, triple: '"' | "'" | null, opts: FormatOptions): { line: string; triple: '"' | "'" | null } {
  if (triple) {
    const close = triple.repeat(3)
    const at = line.indexOf(close)
    if (at < 0) return { line, triple }
    const head = line.slice(0, at + 3)
    const rest = formatLine(line.slice(at + 3), null, opts)
    return { line: head + rest.line, triple: rest.triple }
  }

  let out = ''
  let i = 0
  let openString = false
  while (i < line.length) {
    const c = line[i]
    if (c === '"' || c === "'") {
      if (line.startsWith(c.repeat(3), i)) {
        const closeAt = line.indexOf(c.repeat(3), i + 3)
        if (closeAt < 0) {
          out += line.slice(i)
          return { line: out, triple: c }
        }
        out += line.slice(i, closeAt + 3)
        i = closeAt + 3
        continue
      }
      const start = i
      i += 1
      let body = ''
      let simple = true
      while (i < line.length) {
        if (line[i] === '\\') {
          simple = false
          body += line.slice(i, i + 2)
          i += 2
          continue
        }
        if (line[i] === c) break
        if (line[i] === '{' || line[i] === '[') simple = false
        body += line[i]
        i += 1
      }
      const closed = i < line.length && line[i] === c
      if (closed) i += 1
      else openString = true
      const want = opts.quotes === 'double' ? '"' : opts.quotes === 'single' ? "'" : c
      if (closed && simple && want !== c && !body.includes(want) && !body.includes('\\') && !body.includes('{') && !body.includes('[')) {
        out += want + body + want
      } else {
        out += line.slice(start, i)
      }
      continue
    }
    if (c === '#') {
      out += line.slice(i)
      break
    }
    if (c === '\t') {
      out += ' '.repeat(Math.max(1, opts.indent))
      i += 1
      continue
    }
    out += c
    i += 1
  }
  if (openString) return { line: out, triple: null }
  return { line: out.replace(/[ \t]+$/g, ''), triple: null }
}

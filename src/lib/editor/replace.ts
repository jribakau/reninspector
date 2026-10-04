export interface ReplaceOpts {
  query: string
  replacement: string
  matchCase: boolean
  wholeWord: boolean
  useRegex: boolean
}

function pattern(opts: ReplaceOpts): RegExp | null {
  try {
    const flags = opts.matchCase ? 'g' : 'gi'
    if (opts.useRegex) return new RegExp(opts.query, flags)
    const escaped = opts.query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    const body = opts.wholeWord ? `\\b${escaped}\\b` : escaped
    return new RegExp(body, flags)
  } catch {
    return null
  }
}

/** Replaces matches on the given 1-based lines and leaves every other line as it was. */
export function replaceLines(text: string, lineNumbers: number[], opts: ReplaceOpts): string | null {
  const wanted = new Set(lineNumbers)
  const lines = text.split('\n')
  for (let i = 0; i < lines.length; i++) {
    if (!wanted.has(i + 1)) continue
    const re = pattern(opts)
    if (!re) return null
    const next = opts.useRegex ? lines[i].replace(re, opts.replacement) : lines[i].replace(re, () => opts.replacement)
    lines[i] = next
  }
  return lines.join('\n')
}

/** How one result line would read after the replacement, or null when it would not change. */
export function previewLine(line: string, opts: ReplaceOpts): string | null {
  const next = replaceLines(line, [1], opts)
  if (next == null || next === line) return null
  return next.length > 180 ? `${next.slice(0, 180)}…` : next
}

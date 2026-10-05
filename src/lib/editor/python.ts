/** Whether a line is inside a Python statement, and the indent of an open `python:` block. */
export interface PyState {
  /** Indent of the `python:` / `init python:` block still open after this line. */
  indent: number | null
  /** This line is Python (`$ ...`, or a line indented under a python block). */
  inline: boolean
}

/** `python:`, `init python:`, `python early:` and the other header forms. */
export const PYTHON_HEADER = /^(?:init(?:\s+-?\d+)?\s+)?python(?:\s+(?:early|hide))*\s*(?:in\s+[\w.]+)?\s*:/

/**
 * Advance the Python-block state by one source line.
 * A blank line or a comment does not end a block. The header line itself is not inline.
 */
export function pythonLine(line: string, indent: number | null): PyState {
  const spaces = line.match(/^ */)?.[0].length ?? 0
  const body = line.trim()
  if (indent != null && (!body || body.startsWith('#') || spaces > indent)) {
    return { indent, inline: !!body && !body.startsWith('#') && spaces > indent }
  }
  if (PYTHON_HEADER.test(body)) return { indent: spaces, inline: false }
  if (/^\$(\s|$)/.test(body)) return { indent: null, inline: true }
  return { indent: null, inline: false }
}

/** True when `lineNo` (1-based) is a `$` line or a line inside a python block. */
export function inPython(doc: { lines: number; line: (n: number) => { text: string } }, lineNo: number): boolean {
  let indent: number | null = null
  let inline = false
  const last = Math.max(0, Math.min(lineNo, doc.lines))
  for (let n = 1; n <= last; n++) {
    const next = pythonLine(doc.line(n).text, indent)
    indent = next.indent
    inline = next.inline
  }
  return inline
}

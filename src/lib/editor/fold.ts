import type { EditorState } from '@codemirror/state'

/** Fold a Ren'Py block whose header line ends with `:`. Comments do not close it. */
export function renpyFold(state: EditorState, lineStart: number) {
  const line = state.doc.lineAt(lineStart)
  const header = line.text.trim()
  if (!header.endsWith(':') || header.startsWith('#')) return null
  const indent = line.text.match(/^ */)?.[0].length ?? 0
  let end = line.to
  for (let n = line.number + 1; n <= state.doc.lines; n++) {
    const next = state.doc.line(n)
    const text = next.text.trim()
    if (!text || text.startsWith('#')) continue
    const ind = next.text.match(/^ */)?.[0].length ?? 0
    if (ind <= indent) break
    end = next.to
  }
  if (end <= line.to) return null
  return { from: line.to, to: end }
}

/** Indent of a new line: the previous line's indent, plus one step after a `:`. */
export function renpyIndent(indentWidth: () => number) {
  return (context: { state: EditorState }, pos: number): number => {
    const doc = context.state.doc
    const line = doc.lineAt(pos)
    if (line.number <= 1) return 0
    const prev = doc.line(line.number - 1).text
    const base = prev.match(/^ */)?.[0].length ?? 0
    return prev.trimEnd().endsWith(':') ? base + indentWidth() : base
  }
}

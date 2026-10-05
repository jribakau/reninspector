/** One row of a file outline: a label, menu, screen, transform, or style. */
export interface OutlineEntry {
  id: string
  kind: string
  name: string
  line: number
  endLine: number
}

/** Source order. A later entry on the same line stays later. */
export function mergeOutline(entries: OutlineEntry[]): OutlineEntry[] {
  return entries
    .slice()
    .sort((a, b) => a.line - b.line || a.endLine - b.endLine || a.name.localeCompare(b.name))
}

/**
 * Entries whose range contains `line`, outermost first.
 * A range that starts later, or ends sooner, is inside a wider one.
 */
export function enclosing(entries: OutlineEntry[], line: number): OutlineEntry[] {
  return entries
    .filter((e) => e.line <= line && line <= e.endLine)
    .sort((a, b) => a.line - b.line || b.endLine - a.endLine)
}

/** `file:line` for a breakpoint. The path uses forward slashes. */
export function breakKey(file: string, line: number): string {
  return `${file.replaceAll('\\', '/')}:${line}`
}

/** Line numbers of breakpoints in `file`. */
export function linesFor(file: string, points: readonly string[]): number[] {
  const prefix = `${file.replaceAll('\\', '/')}:`
  const out: number[] = []
  for (const point of points) {
    if (!point.startsWith(prefix)) continue
    const n = Number(point.slice(prefix.length))
    if (Number.isInteger(n) && n > 0) out.push(n)
  }
  return out
}

/** Add or remove one breakpoint. Keeps at most 200. */
export function togglePoint(points: readonly string[], file: string, line: number): string[] {
  if (line < 1) return [...points]
  const id = breakKey(file, line)
  if (points.includes(id)) return points.filter((p) => p !== id)
  return [...points, id].slice(0, 200)
}

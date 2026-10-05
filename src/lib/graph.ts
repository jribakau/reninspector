export function wrapText(text: string, chars: number): string[] {
  const words = text.replace(/\s+/g, ' ').trim().split(' ').filter(Boolean)
  const lines: string[] = []
  let cur = ''
  for (const word of words) {
    const next = cur ? `${cur} ${word}` : word
    if (cur && next.length > chars) {
      lines.push(cur)
      cur = word
    } else cur = next
  }
  if (cur) lines.push(cur)
  return lines.slice(0, 6)
}

export function rangeText(line: number, endLine: number): string {
  return endLine > line ? `lines ${line}–${endLine}` : `line ${line}`
}

export interface EdgePath {
  d: string
  index: number
}

export interface EdgeKind {
  from: number
  to: number
  kind: string
}

/** Group edge paths by kind, and pull edges that touch the selection into one hot path. */
export function packEdges(
  ids: readonly number[],
  selected: number | null,
  layoutEdges: readonly EdgePath[],
  graphEdges: readonly EdgeKind[],
): { batches: { kind: string; d: string }[]; hot: string } {
  const batches: { kind: string; d: string }[] = []
  const parts = new Map<string, string[]>()
  let hot = ''
  for (const i of ids) {
    const e = layoutEdges[i]
    const ge = e ? graphEdges[e.index] : undefined
    if (!e || !ge) continue
    if (selected !== null && (ge.from === selected || ge.to === selected)) {
      hot = hot ? `${hot} ${e.d}` : e.d
      continue
    }
    const bucket = parts.get(ge.kind)
    if (bucket) bucket.push(e.d)
    else parts.set(ge.kind, [e.d])
  }
  for (const [kind, ds] of parts) batches.push({ kind, d: ds.join(' ') })
  return { batches, hot }
}

export interface HitBox {
  id: number
  x: number
  y: number
  w: number
  h: number
}

/** First node box under the point, skipping `skip`. `accept` drops boxes that cannot be the target. */
export function nodeAt(
  nodes: readonly HitBox[],
  x: number,
  y: number,
  skip: number,
  accept?: (box: HitBox) => boolean,
): HitBox | null {
  for (const p of nodes) {
    if (p.id === skip || x < p.x || x > p.x + p.w || y < p.y || y > p.y + p.h) continue
    if (accept && !accept(p)) continue
    return p
  }
  return null
}

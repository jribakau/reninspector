import { app } from './model.svelte'
import type { CatalogView, DiagReport, Diagnostic, FileInfo, MapNode, ProjectInfo, ProjectMap, Symbol } from './types'

let nodeIndex = new Map<string, MapNode>()
let fileByPath = new Map<string, FileInfo>()
let fileIndexByPath = new Map<string, number>()

interface LabelSpan {
  id: string
  line: number
  endLine: number
  order: number
}
/** Non-missing labels per file index, sorted by start line. */
let labelsByFile: LabelSpan[][] = []
let diagnosticsByPath = new Map<string, Diagnostic[]>()
const noDiagnostics: Diagnostic[] = []
let symbolsByName = new Map<string, Symbol>()

export function lookupSymbol(name: string): Symbol | undefined {
  return symbolsByName.get(name)
}

export function symbolsOf(kind: string): Symbol[] {
  return (app.catalog?.symbols ?? []).filter((s) => s.kind === kind)
}

export function nodeByName(name: string): MapNode | undefined {
  return nodeIndex.get(name)
}

export function hasNode(name: string): boolean {
  return nodeIndex.has(name)
}

export function hasFile(path: string): boolean {
  return fileByPath.has(path)
}

export function fileInfo(path: string): FileInfo | undefined {
  return fileByPath.get(path)
}

export function diagnosticsFor(path: string | null | undefined): Diagnostic[] {
  if (!path) return noDiagnostics
  return diagnosticsByPath.get(path) ?? noDiagnostics
}

export function fileOfNode(n: MapNode): string | null {
  return app.info?.files[n.file]?.path ?? null
}

export function setInfo(info: ProjectInfo | null) {
  const byPath = new Map<string, FileInfo>()
  const byIndex = new Map<string, number>()
  if (info) {
    for (let i = 0; i < info.files.length; i++) {
      const file = info.files[i]
      byPath.set(file.path, file)
      byIndex.set(file.path, i)
    }
  }
  fileByPath = byPath
  fileIndexByPath = byIndex
  app.info = info
}

export function setDiag(diag: DiagReport | null) {
  const byPath = new Map<string, Diagnostic[]>()
  if (diag) {
    for (const d of diag.items) {
      if (!d.path) continue
      const list = byPath.get(d.path)
      if (list) list.push(d)
      else byPath.set(d.path, [d])
    }
  }
  diagnosticsByPath = byPath
  app.diag = diag
}

export function setCatalog(catalog: CatalogView | null) {
  const byName = new Map<string, Symbol>()
  if (catalog) {
    for (const s of catalog.symbols) {
      if (!byName.has(s.name)) byName.set(s.name, s)
    }
  }
  symbolsByName = byName
  app.catalog = catalog
}

export function setMap(map: ProjectMap) {
  nodeIndex = new Map(map.nodes.map((n) => [n.id, n]))
  const byFile: LabelSpan[][] = []
  for (let order = 0; order < map.nodes.length; order++) {
    const n = map.nodes[order]
    if (n.kind === 'missing' || n.file < 0) continue
    const list = byFile[n.file] ?? (byFile[n.file] = [])
    list.push({ id: n.id, line: n.line, endLine: n.endLine, order })
  }
  for (const list of byFile) {
    list?.sort((a, b) => a.line - b.line || a.order - b.order)
  }
  labelsByFile = byFile
  app.map = map
}

/** Labels, menus, and screens defined in one script, in source order. */
export function nodesInFile(file: string): MapNode[] {
  const fi = fileIndexByPath.get(file)
  if (fi === undefined) return []
  const list = labelsByFile[fi]
  if (!list) return []
  const out: MapNode[] = []
  for (const span of list) {
    const node = nodeIndex.get(span.id)
    if (node) out.push(node)
  }
  return out
}

/** Innermost label whose source range contains the given line. */
export function labelAt(file: string, line: number): MapNode | null {
  const fi = fileIndexByPath.get(file)
  if (fi === undefined) return null
  const list = labelsByFile[fi]
  if (!list || list.length === 0) return null
  let lo = 0
  let hi = list.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (list[mid].line <= line) lo = mid + 1
    else hi = mid
  }
  // Spans are sorted by start line, so the first containing span walking
  // backward is the innermost one (greatest start line, then latest definition).
  for (let i = lo - 1; i >= 0; i--) {
    const span = list[i]
    if (span.line <= line && line <= span.endLine) return nodeIndex.get(span.id) ?? null
  }
  return null
}

export interface DocParam {
  name: string
  doc: string
}

export interface DocEntry {
  name: string
  kind: 'statement' | 'function' | 'transition' | 'action'
  signature: string
  summary: string
  params: DocParam[]
  url: string
}

interface DocFile {
  license: string
  entries: DocEntry[]
}

let loaded: Map<string, DocEntry> | null = null
let pending: Promise<Map<string, DocEntry>> | null = null

export function loadDocs(): Promise<Map<string, DocEntry>> {
  pending ??= import('./renpy-reference.json').then((mod) => {
    const file = (mod.default ?? mod) as DocFile
    const map = new Map<string, DocEntry>()
    for (const entry of file.entries) map.set(entry.name, entry)
    loaded = map
    return map
  })
  return pending
}

export function docNow(name: string): DocEntry | undefined {
  return loaded?.get(name)
}

/** Functions, transitions and actions whose name starts with `prefix`. */
export function docsStarting(prefix: string): DocEntry[] {
  if (!loaded || !prefix) return []
  const out: DocEntry[] = []
  for (const entry of loaded.values()) {
    if (entry.kind === 'statement') continue
    if (entry.name.startsWith(prefix)) out.push(entry)
  }
  return out.slice(0, 40)
}

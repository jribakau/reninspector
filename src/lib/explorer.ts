export interface TreeEntry {
  path: string
  name: string
  dir: boolean
}

export interface TreeRow<T extends TreeEntry> {
  entry: T
  depth: number
  isNew?: boolean
  renaming?: boolean
}

export interface TreeEdit {
  mode: 'new' | 'rename'
  dir: boolean
  parent?: string
  path?: string
}

/** Depth-first rows for the folders that are open, plus the inline new-name row. */
export function flattenTree<T extends TreeEntry>(
  children: Record<string, T[] | undefined>,
  expanded: ReadonlySet<string>,
  edit: TreeEdit | null,
  newKey: string,
): TreeRow<T>[] {
  const out: TreeRow<T>[] = []
  const walk = (path: string, depth: number) => {
    if (edit?.mode === 'new' && edit.parent === path) {
      out.push({ entry: { path: newKey, name: '', dir: edit.dir } as T, depth, isNew: true })
    }
    for (const entry of children[path] ?? []) {
      out.push({ entry, depth, renaming: edit?.mode === 'rename' && edit.path === entry.path })
      if (entry.dir && expanded.has(entry.path)) walk(entry.path, depth + 1)
    }
  }
  walk('', 0)
  return out
}

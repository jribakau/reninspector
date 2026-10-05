import { isUnder, parentOf } from './treepaths'

/** Folder a dragged entry may land in. Null means the drop is not allowed. */
export function dropFolder(entry: { path: string; dir: boolean }, dest: string | null): string | null {
  if (dest === null) return null
  if (parentOf(entry.path) === dest) return null
  if (entry.dir && isUnder(dest, entry.path)) return null
  return dest
}

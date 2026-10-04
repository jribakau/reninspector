// Pure path helpers for explorer file operations. Tree paths are relative to the project folder.
// Script tabs hold paths relative to `game/`; every other tab holds a tree path.

export type TabFate = { kind: 'keep' } | { kind: 'move'; path: string } | { kind: 'close' }

const SCRIPT_EXT = /\.(rpy|rpym)$/i

export function isScriptName(name: string): boolean {
  return SCRIPT_EXT.test(name)
}

export function baseName(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1)
}

export function parentOf(path: string): string {
  const cut = path.lastIndexOf('/')
  return cut < 0 ? '' : path.slice(0, cut)
}

export function joinPath(parent: string, name: string): string {
  return parent ? `${parent}/${name}` : name
}

/** True when `path` is `folder` or sits somewhere below it. */
export function isUnder(path: string, folder: string): boolean {
  if (!folder) return true
  const p = path.toLowerCase()
  const f = folder.toLowerCase()
  return p === f || p.startsWith(`${f}/`)
}

/** What happens to an open tab when `from` is renamed or moved to `to`, or deleted when `to` is null. */
export function tabFate(tabPath: string, isScript: boolean, game: string, from: string, to: string | null): TabFate {
  const tree = isScript ? joinPath(game, tabPath) : tabPath
  if (!isUnder(tree, from)) return { kind: 'keep' }
  if (to === null) return { kind: 'close' }
  const next = to + tree.slice(from.length)
  if (isScript && isScriptName(next) && (!game || isUnder(next, game))) {
    return { kind: 'move', path: game ? next.slice(game.length + 1) : next }
  }
  return { kind: 'move', path: next }
}

/** The tree path of an entry after `from` moved to `to`. */
export function movedPath(path: string, from: string, to: string): string {
  return isUnder(path, from) ? to + path.slice(from.length) : path
}

import type { GitChange } from './types'

export interface GitFolder {
  name: string
  path: string
  files: GitChange[]
  children: GitFolder[]
}

export function treeOf(changes: GitChange[]): GitFolder[] {
  const root: GitFolder = { name: '', path: '', files: [], children: [] }
  for (const change of changes) {
    const parts = change.path.replaceAll('\\', '/').split('/')
    let node = root
    for (let i = 0; i < parts.length - 1; i++) {
      const path = parts.slice(0, i + 1).join('/')
      let child = node.children.find((item) => item.path === path)
      if (!child) {
        child = { name: parts[i], path, files: [], children: [] }
        node.children.push(child)
      }
      node = child
    }
    node.files.push(change)
  }
  const folders = compress(root.children)
  if (!root.files.length) return folders
  return [{ name: '', path: '', files: root.files, children: [] }, ...folders]
}

function compress(nodes: GitFolder[]): GitFolder[] {
  return nodes.map((node) => {
    let current = node
    const names = [current.name]
    while (current.files.length === 0 && current.children.length === 1) {
      current = current.children[0]
      names.push(current.name)
    }
    return { name: names.join('/'), path: current.path, files: current.files, children: compress(current.children) }
  })
}

export function filesUnder(node: GitFolder): GitChange[] {
  const out = [...node.files]
  for (const child of node.children) out.push(...filesUnder(child))
  return out
}

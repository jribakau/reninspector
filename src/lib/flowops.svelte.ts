import { isBeatCard, isStatementNode, spanOf } from './flowedit'
import { app } from './model.svelte'
import { sceneEdit } from './scene.svelte'
import type { GNode } from './types'

/** True while the keyboard belongs to the flow rather than the code editor. */
export const flowPane = $state({ active: false })

if (typeof document !== 'undefined') {
  document.addEventListener('focusin', (e) => {
    const t = e.target
    if (t instanceof HTMLElement && t.closest('.cm-editor')) flowPane.active = false
  })
}

/** Cards that can be copied, moved, or deleted as one statement. */
export function canRearrange(n: GNode): boolean {
  return isStatementNode(n) && !isBeatCard(n)
}

export async function duplicateNode(file: string, n: GNode): Promise<boolean> {
  return sceneEdit({ op: 'duplicate-stmt', path: file, line: n.line })
}

export async function moveNode(file: string, n: GNode, dir: -1 | 1): Promise<boolean> {
  return sceneEdit({ op: 'move-stmt', path: file, line: n.line, dir })
}

/** Deletes a card after asking, when it is more than a few lines. */
export async function deleteNode(file: string, n: GNode): Promise<boolean> {
  if (spanOf(n) > 3 && !confirm(`Delete ${spanOf(n)} lines from ${file}? Undo can bring them back.`)) return false
  return sceneEdit({ op: 'delete-stmt', path: file, line: n.line })
}

export async function deleteLine(file: string, line: number): Promise<boolean> {
  return sceneEdit({ op: 'delete-stmt', path: file, line })
}

/** Which nodes of the open graph sit on `line`, smallest first. */
export function nodeAtLine(nodes: GNode[], line: number): GNode | null {
  let best: GNode | null = null
  for (const n of nodes) {
    if (n.line <= line && line <= n.endLine) {
      if (!best || n.endLine - n.line < best.endLine - best.line) best = n
    }
  }
  return best
}

export function flowCanUndo(): boolean {
  return flowPane.active && app.sceneCanUndo
}

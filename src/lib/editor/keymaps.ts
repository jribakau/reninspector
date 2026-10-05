import type { Extension } from '@codemirror/state'

export type EditorKeymap = 'default' | 'vim' | 'emacs'

/** Latest save, so `:w` keeps working after the editor is recreated. */
let saveFn: () => void = () => {}
let vimEx = false
const cache = new Map<EditorKeymap, Extension>()

export function setKeymapSave(fn: () => void) {
  saveFn = fn
}

/** The extension if this keymap has already been loaded, otherwise nothing. */
export function keymapNow(mode: EditorKeymap): Extension {
  return cache.get(mode) ?? []
}

/** Vim or Emacs keybindings. The default mode adds none of its own. */
export async function keymapExtension(mode: EditorKeymap): Promise<Extension> {
  if (mode === 'default') return []
  const hit = cache.get(mode)
  if (hit) return hit
  const ext = mode === 'vim' ? await loadVim() : await loadEmacs()
  cache.set(mode, ext)
  return ext
}

async function loadVim(): Promise<Extension> {
  const { vim, Vim } = await import('@replit/codemirror-vim')
  if (!vimEx) {
    Vim.defineEx('write', 'w', () => saveFn())
    vimEx = true
  }
  return vim({ status: true })
}

async function loadEmacs(): Promise<Extension> {
  const { emacs } = await import('@replit/codemirror-emacs')
  return emacs()
}

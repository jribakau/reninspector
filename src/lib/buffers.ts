let bufferSave: ((path: string) => Promise<boolean>) | null = null
/** Save-all and autosave report once, instead of once per file. */
let quietSaveNotices = 0

export function pushQuietSave() {
  quietSaveNotices += 1
}

export function popQuietSave() {
  quietSaveNotices = Math.max(0, quietSaveNotices - 1)
}

export function saveNoticesQuiet(): boolean {
  return quietSaveNotices > 0
}

export function registerBufferSave(fn: ((path: string) => Promise<boolean>) | null) {
  bufferSave = fn
}

/** Writes one open buffer. False when the save failed or there is nothing to write. */
export async function saveBuffer(path: string): Promise<boolean> {
  if (!bufferSave) return false
  return bufferSave(path)
}

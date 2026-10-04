let bufferSave: ((path: string) => Promise<boolean>) | null = null

export function registerBufferSave(fn: ((path: string) => Promise<boolean>) | null) {
  bufferSave = fn
}

/** Writes one open buffer. False when the save failed or there is nothing to write. */
export async function saveBuffer(path: string): Promise<boolean> {
  if (!bufferSave) return false
  return bufferSave(path)
}

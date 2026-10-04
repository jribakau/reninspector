/** A saved splitter size. Rejects missing, NaN, and non-positive values. */
export function storedSize(key: string, fallback: number): number {
  const n = Number(localStorage.getItem(key))
  return Number.isFinite(n) && n > 0 ? n : fallback
}

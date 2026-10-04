import { app } from './model.svelte'
import type { Diagnostic } from './types'

export const problemUi = $state({
  error: true,
  warning: true,
  info: false,
  filter: '',
})

/** Problems the panel is showing, in path order. F8 walks this same list. */
export function visibleProblems(): Diagnostic[] {
  const q = problemUi.filter.trim().toLowerCase()
  return (app.diag?.items ?? [])
    .filter((d) => {
      const on = d.severity === 'error' ? problemUi.error : d.severity === 'warning' ? problemUi.warning : problemUi.info
      if (!on) return false
      if (!d.path) return false
      if (!q) return true
      return d.message.toLowerCase().includes(q) || d.path.toLowerCase().includes(q) || d.code.includes(q)
    })
    .slice()
    .sort((a, b) => a.path.localeCompare(b.path) || a.line - b.line)
}
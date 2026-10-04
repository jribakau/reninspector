import { ask } from './dialog.svelte'
import { app } from './model.svelte'

const trustKey = (root: string) => `vnide.trust.${root}`

/** Bumped on every change so effects that read `isTrusted` run again. */
const trustState = $state({ rev: 0 })

export function isTrusted(root = app.info?.root): boolean {
  void trustState.rev
  return !!root && localStorage.getItem(trustKey(root)) === '1'
}

export function markTrusted(root: string) {
  localStorage.setItem(trustKey(root), '1')
  trustState.rev += 1
}

export function forgetTrust() {
  const root = app.info?.root
  if (!root) return
  localStorage.removeItem(trustKey(root))
  trustState.rev += 1
  app.notice = 'The IDE will ask again before it runs this game\u2019s code.'
}

/**
 * Asks once per project before anything starts the game's own engine. Running,
 * live preview, replay, engine checks, lint, stage images and builds all execute
 * the game's scripts and Python code.
 */
export async function ensureTrusted(): Promise<boolean> {
  const root = app.info?.root
  if (!root) return false
  if (isTrusted(root)) return true
  const ok = await ask({
    title: 'Run this game\u2019s code?',
    note:
      'Running, live preview, engine checks, lint, stage images and builds start the game\u2019s own engine. ' +
      'That runs its scripts and Python code with your user rights. Only allow this for a game you made or trust.',
    ok: 'Trust this game',
    fields: [],
  })
  if (!ok || app.info?.root !== root) return false
  markTrusted(root)
  return true
}

import { api, errorText } from './api'
import { app } from './model.svelte'
import { openBottom } from './nav.svelte'
import { reloadAll } from './reload.svelte'
import { ensureTrusted } from './trust.svelte'

/**
 * Runs the game's own bundled engine headless to list every label and screen it
 * really loads. It executes the game's init code, so it can touch the game folder
 * (log, cache, saves are backed up and restored).
 */
export async function checkWithEngine() {
  if (!app.info || app.busy || !(await ensureTrusted())) return
  app.error = ''
  app.busy = 'Asking the game\u2019s engine\u2026'
  try {
    const info = await api.engineDump(app.launcher)
    await reloadAll(info)
    const e = info.engine
    app.notice = e
      ? e.labelsAvailable
        ? `Engine loaded ${e.labels.toLocaleString()} labels and ${e.screens.toLocaleString()} screens in ${(e.durationMs / 1000).toFixed(1)} s.`
        : `Engine ran, but this Ren\u2019Py version does not report labels. ${e.notes.join(' ')}`
      : 'Engine check finished.'
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

/** Runs the engine's own `lint` and merges its findings into Problems. */
export async function runEngineLint() {
  if (!app.info || app.busy || !(await ensureTrusted())) return
  app.error = ''
  app.busy = 'Running Ren\u2019Py lint\u2026'
  try {
    const info = await api.engineLint(app.launcher)
    await reloadAll(info)
    app.notice = `Lint reported ${info.lintCount ?? 0} finding${info.lintCount === 1 ? '' : 's'}.`
    openBottom('problems')
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function toggleAutoreload() {
  if (!app.info) return
  try {
    app.autoreload = await api.setAutoreload(!app.autoreload)
    app.notice = app.autoreload
      ? 'Auto-reload is on for the next launch. The game picks up saved scripts after it starts with this file in game/.'
      : 'Auto-reload is off.'
  } catch (e) {
    app.error = errorText(e)
  }
}

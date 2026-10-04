import { app } from './model.svelte'
import { saveAll } from './nav.svelte'
import { settings } from './settings.svelte'

let started = false

/** Saves changed scripts as the Autosave setting asks. Starts once. */
export function startAutosave() {
  if (started) return
  started = true

  $effect.root(() => {
    $effect(() => {
      if (settings.autosave !== 'delay') return
      if (!app.dirtyFiles.length) return
      const delay = settings.autosaveDelay
      let timer: ReturnType<typeof setTimeout>
      const fire = () => {
        // Never save in the middle of a parse, save, or other long task.
        if (app.busy) timer = setTimeout(fire, delay)
        else void saveAll({ quiet: true })
      }
      timer = setTimeout(fire, delay)
      return () => clearTimeout(timer)
    })
  })

  window.addEventListener('blur', () => {
    if (settings.autosave !== 'blur' || app.busy || !app.dirtyFiles.length) return
    void saveAll({ quiet: true })
  })
}

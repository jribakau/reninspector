import { api, errorText, pickFolder, pickSavePath, pickZipPath } from './api'
import { askText } from './dialog.svelte'
import { app } from './model.svelte'
import { pullEditStatus, reloadAll } from './reload.svelte'
import { notify } from './toast.svelte'

async function refreshAfterArchive(notice: string) {
  const info = await api.projectInfo()
  if (info) await reloadAll(info)
  await pullEditStatus()
  notify(notice, 'ok')
}

export async function bakePatch(options: { toggles?: boolean } = {}) {
  if (!app.info || app.busy) return
  const pending = app.info.files.filter((f) => f.origin === 'override').length
  if (!pending && !options.toggles) {
    app.error = 'Nothing to bake. Save an archived script first; that writes a loose file the game uses immediately.'
    return
  }
  const ask = pending
    ? `Fold ${pending} loose override${pending === 1 ? '' : 's'} into a patch archive? The game's original archives are not modified. Undo puts the loose files back.`
    : 'Write the mod toggles into the patch? The game\'s original archives are not modified.'
  if (!confirm(ask)) {
    return
  }
  app.error = ''
  app.busy = 'Baking patch…'
  try {
    const report = await api.patchBake()
    await refreshAfterArchive(`Baked ${report.files} files into ${report.patch}.`)
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function undoPatch() {
  if (!app.info || app.busy) return
  if (!confirm('Undo the last bake or patch removal?')) return
  app.error = ''
  app.busy = 'Undoing patch…'
  try {
    const report = await api.patchUndo()
    await refreshAfterArchive(`Restored ${report.patch}.`)
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function removePatch() {
  if (!app.info || app.busy) return
  const patch = app.info.archives.find((a) => a.isPatch)
  if (!patch) {
    app.error = 'There is no patch archive to remove.'
    return
  }
  if (!confirm(`Remove ${patch.path}? The game goes back to its original archives. Undo can put the patch back.`)) return
  app.error = ''
  app.busy = 'Removing patch…'
  try {
    await api.patchRemove()
    await refreshAfterArchive(`Removed ${patch.path}.`)
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function extractArchive(archive: string, names: string[] | null) {
  if (!app.info || app.busy) return
  const dest = await pickFolder('Extract archive to')
  if (!dest) return
  app.error = ''
  app.busy = 'Extracting…'
  try {
    let files = 0
    try {
      files = await api.archiveExtract(archive, dest, names, false)
    } catch (e) {
      const msg = errorText(e)
      if (msg.includes('inside game') && confirm(`${msg}\n\nExtract anyway?`)) {
        files = await api.archiveExtract(archive, dest, names, true)
        await refreshAfterArchive(`Extracted ${files} files into game/. They override the archive.`)
        return
      }
      throw e
    }
    notify(`Extracted ${files} files to ${dest}.`, 'ok')
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function buildArchive() {
  if (!app.info || app.busy) return
  const src = await pickFolder('Folder to pack into an archive')
  if (!src) return
  const out = await pickSavePath('Save archive as', 'archive.rpa')
  if (!out) return
  app.error = ''
  app.busy = 'Building archive…'
  try {
    const files = await api.archiveBuild(src, out)
    const info = await api.projectInfo()
    if (info) await reloadAll(info)
    notify(`Wrote ${files} files to ${out}.`, 'ok')
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function exportMod(layout: 'rpa' | 'loose', includeToggles = false) {
  if (!app.info || app.busy) return
  if (!app.info.archives.some((a) => a.isPatch)) {
    app.error = 'Bake a patch before exporting a mod.'
    return
  }
  const notes = await askText('Mod notes', '', 'Continue')
  if (notes === null) return
  const dest = await pickZipPath('Export mod', 'mod.zip')
  if (!dest) return
  app.error = ''
  app.busy = 'Exporting mod…'
  try {
    const notice = await api.modExport(dest, layout, includeToggles, notes)
    notify(notice, 'ok')
  } catch (e) {
    app.error = errorText(e)
  } finally {
    app.busy = ''
  }
}

export async function cancelArchiveJob() {
  await api.archiveCancel().catch(() => {})
  app.notice = 'Cancelling…'
}

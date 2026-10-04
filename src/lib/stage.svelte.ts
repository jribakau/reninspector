import { api, errorText, onStageImages, readAsset, readLiveShot } from './api'
import { setInfo } from './indexes.svelte'
import { app } from './model.svelte'
import { isTrusted } from './trust.svelte'
import type { StageEstimate, StagePicture } from './types'

export const stageUi = $state({
  estimate: null as StageEstimate | null,
  error: '',
  images: {} as Record<string, string>,
  /** Pictures the backend could not read, with the reason. Reset for each estimate. */
  failedReads: {} as Record<string, string>,
  shot: null as { seq: number; file: string; line: number; url: string } | null,
  preferEstimate: false,
  pending: null as { file: string; line: number } | null,
  holdLive: false,
  shotError: '',
  shotStale: false,
  imageStatus: '',
  imageNotes: [] as string[],
  imageError: '',
  /** The engine could fill in images, but this game is not trusted to run yet. */
  needsTrust: false,
  imageTick: 0,
  reloadGen: 0,
})

const urls = new Map<string, string>()
let generation = 0
let shotSeq = 0
let inflight = false
let wanted: { file: string; line: number } | null = null

export function stageShowsLive(): boolean {
  const shot = stageUi.shot
  const cursor = app.cursor
  if (stageUi.preferEstimate || !shot || !cursor || stageUi.shotError) return false
  if (!app.live.running) return false
  if (shot.file === cursor.file && shot.line === cursor.line) return true
  // Follow moves the caret before the next screenshot arrives. Keep the last frame
  // for a moment instead of flashing the estimate.
  return stageUi.holdLive && app.followGame
}

export function stageSummary(estimate: StageEstimate): string {
  if (estimate.via === 'label') return 'this label only'
  if (estimate.decisions === 1) return 'via 1 menu choice'
  if (estimate.decisions > 1) return `via ${estimate.decisions} menu choices`
  return 'from the start'
}

function pathsOf(picture: StagePicture, out: string[]) {
  if (picture.kind === 'file' && picture.path) out.push(picture.path)
  else if (picture.kind === 'layers') {
    for (const layer of picture.layers) out.push(layer.path)
  } else if (picture.kind === 'frames') {
    for (const frame of picture.frames.slice(0, 32)) pathsOf(frame.picture, out)
  }
}

function picturePaths(estimate: StageEstimate): string[] {
  const out: string[] = []
  for (const sprite of estimate.sprites) pathsOf(sprite.picture, out)
  const gui = estimate.gui
  if (estimate.say) {
    if (gui.textbox) out.push(gui.textbox)
    if (gui.namebox && estimate.say.who) out.push(gui.namebox)
  }
  if (estimate.choices.length && gui.choice) out.push(gui.choice)
  return out
}

function publishImages() {
  stageUi.images = Object.fromEntries(urls)
}

async function syncImages(estimate: StageEstimate, seq: number) {
  const want = new Set(picturePaths(estimate))
  for (const path of [...urls.keys()]) {
    if (want.has(path)) continue
    const url = urls.get(path)
    if (url) URL.revokeObjectURL(url)
    urls.delete(path)
  }
  publishImages()
  stageUi.failedReads = {}
  for (const path of want) {
    if (urls.has(path)) continue
    try {
      const buf = await readAsset(path)
      if (seq !== generation) return
      const url = URL.createObjectURL(new Blob([buf]))
      if (seq !== generation) {
        URL.revokeObjectURL(url)
        return
      }
      urls.set(path, url)
      publishImages()
    } catch (e) {
      if (seq !== generation) return
      stageUi.failedReads = { ...stageUi.failedReads, [path]: errorText(e) }
    }
  }
}

export async function refreshStage(file: string, line: number) {
  wanted = { file, line }
  if (inflight) return
  inflight = true
  void pumpStage()
}

async function pumpStage() {
  try {
    while (wanted) {
      const want = wanted
      wanted = null
      const seq = ++generation
      stageUi.pending = want
      try {
        const estimate = await api.stageAt(want.file, want.line)
        if (wanted || seq !== generation) continue
        stageUi.estimate = estimate
        stageUi.error = ''
        stageUi.pending = null
        void syncImages(estimate, seq)
      } catch (e) {
        if (wanted || seq !== generation) continue
        stageUi.error = errorText(e)
        stageUi.pending = null
      }
    }
  } finally {
    inflight = false
    if (wanted) {
      inflight = true
      void pumpStage()
    }
  }
}

export function noteLiveShot(shot: { seq: number; file: string; line: number }) {
  if (shot.seq === shotSeq) return
  shotSeq = shot.seq
  void readLiveShot()
    .then((buf) => {
      if (shot.seq !== shotSeq) return
      if (stageUi.shot) URL.revokeObjectURL(stageUi.shot.url)
      stageUi.shot = {
        ...shot,
        url: URL.createObjectURL(new Blob([buf], { type: 'image/png' })),
      }
      stageUi.shotError = ''
      stageUi.shotStale = false
      stageUi.holdLive = false
    })
    .catch(() => {
      if (shot.seq !== shotSeq) return
      stageUi.shotError = "Couldn't load the live screenshot."
      stageUi.shotStale = true
      stageUi.holdLive = false
    })
}

export function clearLiveShot() {
  if (stageUi.shot) URL.revokeObjectURL(stageUi.shot.url)
  stageUi.shot = null
  shotSeq = 0
}

export function stageScriptsReloaded() {
  stageUi.reloadGen += 1
}

/** Cursor, save, and live-screenshot effects. Call once from the workspace, which stays mounted. */
export function bindStage() {
  let inflight = false
  let asked = ''

  void onStageImages((info) => {
    if (app.info?.root !== info.root) return
    setInfo(info)
    stageUi.imageError = ''
    asked = ''
    const cursor = app.cursor
    if (cursor && app.stageOpen && !app.sceneExpanded) void refreshStage(cursor.file, cursor.line)
  })

  $effect(() => {
    const open = app.stageOpen && !app.sceneExpanded
    const info = app.info
    const seq = app.changeSeq
    const reloadGen = stageUi.reloadGen
    void stageUi.imageTick
    const launcher = app.launcher || info?.launcher
    const summary = info?.stageImages ?? null
    const live = app.live.running
    stageUi.imageNotes = summary?.notes ?? []
    if (stageUi.imageError) stageUi.imageNotes = [...stageUi.imageNotes, stageUi.imageError]
    stageUi.needsTrust = false
    if (!open || !info) {
      stageUi.imageStatus = ''
      return
    }
    if (!launcher) {
      stageUi.imageStatus = 'Scripts only (no engine found)'
      return
    }
    if (summary && !summary.stale) {
      asked = ''
      stageUi.imageError = ''
      stageUi.imageStatus = summary.notes.some((n) => n.startsWith('From the running game'))
        ? 'Images from the running game'
        : 'Images from the engine'
      return
    }
    if (!live && !isTrusted(info.root)) {
      stageUi.needsTrust = true
      stageUi.imageStatus = 'Scripts only'
      return
    }
    const token = `${info.root}:${seq}:${reloadGen}:${live ? 'live' : 'headless'}`
    if (asked === token) {
      if (stageUi.imageStatus !== 'Reload the game to refresh images') {
        stageUi.imageStatus = 'Asking the engine...'
      }
      return
    }
    stageUi.imageStatus = live ? 'Asking the engine…' : 'Waiting to ask the engine…'
    const root = info.root
    const timer = setTimeout(() => {
      if (inflight || app.info?.root !== root) return
      inflight = true
      asked = token
      stageUi.imageStatus = 'Asking the engine…'
      const run = live
        ? api.liveImages()
        : api.stageImages(app.launcher).then((next) => {
            if (app.info?.root !== root) return
            setInfo(next)
            stageUi.imageError = ''
            asked = ''
            const cursor = app.cursor
            if (cursor && app.stageOpen && !app.sceneExpanded) void refreshStage(cursor.file, cursor.line)
          })
      void run
        .catch((e) => {
          const msg = errorText(e)
          if (msg.includes('Reload the game')) {
            stageUi.imageError = ''
            stageUi.imageStatus = 'Reload the game to refresh images'
          } else {
            stageUi.imageError = msg
          }
        })
        .finally(() => {
          inflight = false
          stageUi.imageTick += 1
        })
    }, live ? 0 : 2000)
    return () => clearTimeout(timer)
  })

  $effect(() => {
    const cursor = app.cursor
    const open = app.stageOpen && !app.sceneExpanded
    void app.changeSeq
    if (!open || !cursor) return
    const file = cursor.file
    const line = cursor.line
    const timer = setTimeout(() => void refreshStage(file, line), 200)
    let hold: ReturnType<typeof setTimeout> | undefined
    if (app.followGame && app.live.running && stageUi.shot && !stageUi.preferEstimate) {
      stageUi.holdLive = true
      hold = setTimeout(() => {
        stageUi.holdLive = false
      }, 700)
    }
    return () => {
      clearTimeout(timer)
      if (hold) clearTimeout(hold)
    }
  })

  let shotsOn: boolean | null = null
  $effect(() => {
    const on = app.stageOpen && !app.sceneExpanded && app.live.running
    if (on !== shotsOn) {
      shotsOn = on
      void api.liveShots(on).catch(() => {})
    }
    if (!app.live.running) clearLiveShot()
  })
}

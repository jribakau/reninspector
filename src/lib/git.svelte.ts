import { untrack } from 'svelte'
import { api, errorText } from './api'
import { app } from './model.svelte'
import type { GitBranch, GitStatus } from './types'

export interface GitDecor {
  /** Project-relative file path to a status letter (M, A, D, R, U). */
  files: Map<string, string>
  /** Folders that contain a change. */
  dirs: Set<string>
}

const noDecor: GitDecor = { files: new Map(), dirs: new Set() }

export const git = $state({
  status: null as GitStatus | null,
  err: '',
  branches: [] as GitBranch[],
  /** Bumped after every status refresh so the editor and diffs can reload from the index. */
  seq: 0,
  picker: false,
  /** Bumped to open Source Control on a file's history. */
  timeline: 0,
  /** File whose history the panel shows. Empty means the open file. */
  historyFile: '',
  /** Replaced as a whole on each refresh, so readers need no deep reactivity. */
  decor: noDecor as GitDecor,
})

let ticket = 0

function letterOf(status: string): string {
  if (status === '?') return 'U'
  return status.slice(0, 1).toUpperCase() || 'M'
}

export function decorOf(status: GitStatus | null): GitDecor {
  if (!status || !status.changes.length) return noDecor
  const files = new Map<string, string>()
  const dirs = new Set<string>()
  for (const change of status.changes) {
    const path = change.path.replaceAll('\\', '/')
    if (!files.has(path) || !change.staged) files.set(path, letterOf(change.status))
    const parts = path.split('/')
    for (let i = 1; i < parts.length; i++) dirs.add(parts.slice(0, i).join('/'))
  }
  return { files, dirs }
}

async function refreshGit() {
  const root = app.info?.root ?? ''
  const mine = ++ticket
  if (!root) {
    git.status = null
    git.err = ''
    git.branches = []
    git.decor = noDecor
    git.seq += 1
    return
  }
  try {
    const status = await api.gitStatus()
    if (mine !== ticket || (app.info?.root ?? '') !== root) return
    const branches = await api.gitBranches().catch(() => [] as GitBranch[])
    if (mine !== ticket || (app.info?.root ?? '') !== root) return
    git.status = status
    git.branches = branches
    git.decor = decorOf(status)
    git.err = ''
    git.seq += 1
  } catch (e) {
    if (mine !== ticket || (app.info?.root ?? '') !== root) return
    git.status = null
    git.branches = []
    git.decor = noDecor
    git.err = errorText(e)
    git.seq += 1
  }
}

let loading = false
let again = false

/** One status read at a time. A refresh requested while one is running becomes a single follow-up. */
export async function loadGit() {
  if (loading) {
    again = true
    return
  }
  loading = true
  try {
    do {
      again = false
      await refreshGit()
    } while (again)
  } finally {
    loading = false
    if (again) void loadGit()
  }
}

/** One watcher for the whole window. Call it from the root component. */
export function watchGit() {
  $effect(() => {
    void app.changeSeq
    void app.info?.root
    // loadGit writes git.seq, which this effect must not read, or it retriggers itself.
    untrack(() => void loadGit())
  })
}

export function openBranchPicker() {
  git.picker = true
}
